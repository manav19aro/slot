//! Explicit HTTPS pack downloads and manual softcore sync. No background requests.
use crate::pack::{hash_gba_rom, Achievement, GbaPack};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    io::Read,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

const API: &str = "https://retroachievements.org/dorequest.php";
const MEDIA: &str = "https://media.retroachievements.org/Badge/";
const MAX_RESPONSE: usize = 8 * 1024 * 1024;

// Deliberately not Debug: account tokens must not appear in logs.
#[derive(Clone, Default, Deserialize, Serialize)]
pub struct Account {
    pub username: String,
    pub token: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Cancelled,
    Network,
    Login,
    Unsupported,
    InvalidPack,
    InvalidUnlocks,
    LocalPack,
    Upload,
    Storage,
    RomChanged,
}
impl Error {
    pub fn message(&self) -> &'static str {
        match self {
            Self::Cancelled => "Download cancelled",
            Self::Network => "Could not reach RetroAchievements. Check Wi-Fi and the device clock",
            Self::Login => "Sign-in failed. Check your RetroAchievements details",
            Self::Unsupported => "No achievements found for this exact ROM",
            Self::InvalidPack => "The downloaded pack could not be validated",
            Self::InvalidUnlocks => "RetroAchievements returned invalid progress data",
            Self::Upload => {
                "Could not upload progress. Saved unlocks will be retried on the next Sync"
            }
            Self::LocalPack => "Could not read the saved achievement set. Download it again",
            Self::Storage => "Could not save to the SD card",
            Self::RomChanged => "The game changed during the download. Try again",
        }
    }
}

pub struct Prepared {
    pub title: String,
    pub count: usize,
    pub missing_badges: usize,
}

fn check(cancel: &AtomicBool) -> Result<(), Error> {
    if cancel.load(Ordering::Relaxed) {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}
fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .https_only(true)
        .redirects(0)
        .timeout_connect(Duration::from_secs(8))
        .timeout_read(Duration::from_secs(10))
        .timeout(Duration::from_secs(25))
        .user_agent("slot-offline-achievements/1.0")
        .build()
}
fn body(response: ureq::Response, maximum: usize, cancel: &AtomicBool) -> Result<Vec<u8>, Error> {
    let mut reader = response.into_reader();
    let mut result = Vec::new();
    let mut chunk = [0; 8192];
    loop {
        check(cancel)?;
        let count = reader.read(&mut chunk).map_err(|_| Error::Network)?;
        if count == 0 {
            break;
        }
        if result.len() + count > maximum {
            return Err(Error::InvalidPack);
        }
        result.extend_from_slice(&chunk[..count]);
    }
    Ok(result)
}
fn request(
    agent: &ureq::Agent,
    fields: &[(&str, &str)],
    cancel: &AtomicBool,
) -> Result<Value, Error> {
    check(cancel)?;
    let response = agent
        .post(API)
        .send_form(fields)
        .map_err(|_| Error::Network)?;
    let bytes = body(response, MAX_RESPONSE, cancel)?;
    let result: Value = serde_json::from_slice(&bytes).map_err(|_| Error::InvalidPack)?;
    if result.get("Success").and_then(Value::as_bool) != Some(true) {
        return Err(Error::Login);
    }
    Ok(result)
}

pub fn login(username: &str, password: &str, cancel: &AtomicBool) -> Result<Account, Error> {
    let result = request(
        &agent(),
        &[("r", "login2"), ("u", username), ("p", password)],
        cancel,
    )?;
    let token = result
        .get("Token")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or(Error::Login)?;
    Ok(Account {
        username: result
            .get("User")
            .and_then(Value::as_str)
            .unwrap_or(username)
            .into(),
        token: token.into(),
    })
}

fn number(value: &Value, key: &str) -> Result<u32, Error> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .ok_or(Error::InvalidPack)
}
fn string(value: &Value, key: &str) -> Result<String, Error> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or(Error::InvalidPack)
}
fn make_pack(response: Value, hash: &str, game_id: u32) -> Result<GbaPack, Error> {
    let patch = response.get("PatchData").ok_or(Error::InvalidPack)?;
    if number(patch, "ConsoleID")? != 5 || number(patch, "ID")? != game_id {
        return Err(Error::InvalidPack);
    }
    let entries = patch
        .get("Achievements")
        .and_then(Value::as_array)
        .ok_or(Error::InvalidPack)?;
    let mut achievements = Vec::new();
    for entry in entries {
        let id = number(entry, "ID")?;
        if crate::is_service_warning(id) || number(entry, "Flags")? != 3 {
            continue;
        }
        achievements.push(Achievement {
            id,
            title: string(entry, "Title")?,
            description: string(entry, "Description")?,
            points: number(entry, "Points")?,
            rule: string(entry, "MemAddr")?,
            badge: entry
                .get("BadgeName")
                .and_then(Value::as_str)
                .map(str::to_owned),
        });
    }
    if achievements.is_empty() {
        return Err(Error::Unsupported);
    }
    let pack = GbaPack {
        version: 1,
        console_id: 5,
        game_id,
        title: string(patch, "Title")?,
        rom_hash: hash.into(),
        achievements,
    };
    pack.validate(hash).map_err(|_| Error::InvalidPack)?;
    Ok(pack)
}
fn valid_badge(bytes: &[u8]) -> bool {
    let mut decoder = png::Decoder::new(bytes);
    decoder.set_limits(png::Limits { bytes: 1024 * 1024 });
    let Ok(mut reader) = decoder.read_info() else {
        return false;
    };
    let info = reader.info();
    if info.width == 0 || info.height == 0 || info.width > 256 || info.height > 256 {
        return false;
    }
    let mut buffer = vec![0; reader.output_buffer_size()];
    reader.next_frame(&mut buffer).is_ok()
}

pub fn prepare(
    root: &Path,
    rom: &Path,
    account: &Account,
    cancel: &AtomicBool,
    mut progress: impl FnMut(String),
) -> Result<Prepared, Error> {
    prepare_with(root, rom, account, cancel, &mut progress, &Live(agent()))
}

trait Transport {
    fn request(&self, fields: &[(&str, &str)], cancel: &AtomicBool) -> Result<Value, Error>;
    fn badge(&self, name: &str, cancel: &AtomicBool) -> Result<Vec<u8>, Error>;
}
struct Live(ureq::Agent);
impl Transport for Live {
    fn request(&self, fields: &[(&str, &str)], cancel: &AtomicBool) -> Result<Value, Error> {
        request(&self.0, fields, cancel)
    }
    fn badge(&self, name: &str, cancel: &AtomicBool) -> Result<Vec<u8>, Error> {
        let response = self
            .0
            .get(&format!("{MEDIA}{name}.png"))
            .call()
            .map_err(|_| Error::Network)?;
        body(response, 256 * 1024, cancel)
    }
}
fn prepare_with(
    root: &Path,
    rom: &Path,
    account: &Account,
    cancel: &AtomicBool,
    progress: &mut impl FnMut(String),
    transport: &impl Transport,
) -> Result<Prepared, Error> {
    check(cancel)?;
    progress("Identifying this ROM".into());
    let hash = hash_gba_rom(rom).map_err(|_| Error::InvalidPack)?;
    let identity = transport.request(&[("r", "gameid"), ("m", &hash)], cancel)?;
    let game_id = number(&identity, "GameID")?;
    if game_id == 0 {
        return Err(Error::Unsupported);
    }
    progress("Downloading achievement definitions".into());
    let response = transport.request(
        &[
            ("r", "patch"),
            ("u", &account.username),
            ("t", &account.token),
            ("m", &hash),
        ],
        cancel,
    )?;
    let pack = make_pack(response, &hash, game_id)?;
    let badges: BTreeSet<_> = pack
        .achievements
        .iter()
        .filter_map(|a| a.badge.as_deref())
        .filter(|name| {
            !name.is_empty() && name.len() <= 16 && name.bytes().all(|c| c.is_ascii_digit())
        })
        .collect();
    let cache = root.join("Achievements/Badges");
    std::fs::create_dir_all(&cache).map_err(|_| Error::Storage)?;
    let mut missing_badges = 0;
    for (index, name) in badges.iter().enumerate() {
        check(cancel)?;
        progress(format!("Saving badges: {} / {}", index + 1, badges.len()));
        let path = cache.join(format!("{name}.png"));
        if std::fs::metadata(&path).is_ok_and(|m| m.len() <= 256 * 1024)
            && std::fs::read(&path).is_ok_and(|bytes| valid_badge(&bytes))
        {
            continue;
        }
        let bytes = transport.badge(name, cancel);
        match bytes {
            Ok(bytes) if valid_badge(&bytes) => {
                slot_store::atomic_write(&path, &bytes).map_err(|_| Error::Storage)?
            }
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            _ => missing_badges += 1,
        }
    }
    check(cancel)?;
    if hash_gba_rom(rom).map_err(|_| Error::RomChanged)? != hash {
        return Err(Error::RomChanged);
    }
    let directory = root.join("Achievements/GBA");
    std::fs::create_dir_all(&directory).map_err(|_| Error::Storage)?;
    let bytes = serde_json::to_vec_pretty(&pack).map_err(|_| Error::InvalidPack)?;
    if bytes.len() > MAX_RESPONSE {
        return Err(Error::InvalidPack);
    }
    check(cancel)?;
    slot_store::atomic_write(&directory.join(format!("{hash}.json")), &bytes)
        .map_err(|_| Error::Storage)?;
    Ok(Prepared {
        title: pack.title,
        count: pack.achievements.len(),
        missing_badges,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    struct Fake {
        patch: Value,
        calls: RefCell<Vec<String>>,
        badge_fails: bool,
        unknown: bool,
    }
    impl Transport for Fake {
        fn request(&self, fields: &[(&str, &str)], cancel: &AtomicBool) -> Result<Value, Error> {
            check(cancel)?;
            let method = fields.iter().find(|(key, _)| *key == "r").unwrap().1;
            self.calls.borrow_mut().push(method.into());
            match method {
                "gameid" => Ok(serde_json::json!({"GameID": if self.unknown { 0 } else { 42 }})),
                "patch" => Ok(self.patch.clone()),
                _ => panic!("unexpected request: {method}"),
            }
        }
        fn badge(&self, _: &str, cancel: &AtomicBool) -> Result<Vec<u8>, Error> {
            check(cancel)?;
            if self.badge_fails {
                return Err(Error::Network);
            }
            let mut output = Vec::new();
            let mut encoder = png::Encoder::new(&mut output, 64, 64);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&[0, 200, 0, 255].repeat(64 * 64))
                .unwrap();
            Ok(output)
        }
    }
    fn fake() -> Fake {
        Fake {
            patch: serde_json::json!({"PatchData": {"ID":42,"ConsoleID":5,"Title":"Test Game","Achievements":[
                {"ID":1,"Flags":3,"Title":"First","Description":"Test","Points":5,"MemAddr":"0xH008000=3","BadgeName":"123"},
                {"ID":2,"Flags":5,"Title":"Unofficial"}, {"ID":101000001}
            ]}}),
            calls: RefCell::new(vec![]),
            badge_fails: false,
            unknown: false,
        }
    }
    #[test]
    fn explicit_preparation_saves_only_official_rules_and_badges_and_preserves_history() {
        let root = tempfile::tempdir().unwrap();
        let rom = root.path().join("game.gba");
        std::fs::write(&rom, b"abc").unwrap();
        let hash = hash_gba_rom(&rom).unwrap();
        let history = root.path().join("Achievements/Progress/GBA");
        std::fs::create_dir_all(&history).unwrap();
        std::fs::write(history.join(format!("{hash}.json")), b"existing history").unwrap();
        let fake = fake();
        let result = prepare_with(
            root.path(),
            &rom,
            &Account::default(),
            &AtomicBool::new(false),
            &mut |_| {},
            &fake,
        )
        .unwrap();
        assert_eq!(result.count, 1);
        assert_eq!(result.missing_badges, 0);
        assert_eq!(
            GbaPack::read(
                &root.path().join(format!("Achievements/GBA/{hash}.json")),
                &hash
            )
            .unwrap()
            .achievements
            .len(),
            1
        );
        assert!(root.path().join("Achievements/Badges/123.png").exists());
        assert_eq!(
            std::fs::read(history.join(format!("{hash}.json"))).unwrap(),
            b"existing history"
        );
        assert_eq!(*fake.calls.borrow(), ["gameid", "patch"]);
        assert!(
            !root.path().join("Settings").exists(),
            "pack preparation must not save credentials"
        );
    }
    #[test]
    fn invalid_rules_and_wrong_game_ids_keep_the_existing_pack() {
        let root = tempfile::tempdir().unwrap();
        let rom = root.path().join("game.gba");
        std::fs::write(&rom, b"abc").unwrap();
        let hash = hash_gba_rom(&rom).unwrap();
        let directory = root.path().join("Achievements/GBA");
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join(format!("{hash}.json"));
        std::fs::write(&path, b"old pack").unwrap();
        for wrong_id in [true, false] {
            let mut fake = fake();
            if wrong_id {
                fake.patch["PatchData"]["ID"] = 7.into();
            } else {
                fake.patch["PatchData"]["Achievements"][0]["MemAddr"] = "not a rule".into();
            }
            assert!(matches!(
                prepare_with(
                    root.path(),
                    &rom,
                    &Account::default(),
                    &AtomicBool::new(false),
                    &mut |_| {},
                    &fake
                ),
                Err(Error::InvalidPack)
            ));
            assert_eq!(std::fs::read(&path).unwrap(), b"old pack");
        }
    }
    #[test]
    fn cancellation_and_unknown_roms_do_not_write_packs() {
        let root = tempfile::tempdir().unwrap();
        let rom = root.path().join("game.gba");
        std::fs::write(&rom, b"abc").unwrap();
        let mut fake = fake();
        assert!(matches!(
            prepare_with(
                root.path(),
                &rom,
                &Account::default(),
                &AtomicBool::new(true),
                &mut |_| {},
                &fake
            ),
            Err(Error::Cancelled)
        ));
        assert!(fake.calls.borrow().is_empty());
        fake.unknown = true;
        assert!(matches!(
            prepare_with(
                root.path(),
                &rom,
                &Account::default(),
                &AtomicBool::new(false),
                &mut |_| {},
                &fake
            ),
            Err(Error::Unsupported)
        ));
        assert!(!root.path().join("Achievements").exists());
    }
    #[test]
    fn badge_failure_keeps_the_valid_pack_and_reports_missing_images() {
        let root = tempfile::tempdir().unwrap();
        let rom = root.path().join("game.gba");
        std::fs::write(&rom, b"abc").unwrap();
        let mut fake = fake();
        fake.badge_fails = true;
        let result = prepare_with(
            root.path(),
            &rom,
            &Account::default(),
            &AtomicBool::new(false),
            &mut |_| {},
            &fake,
        )
        .unwrap();
        assert_eq!((result.count, result.missing_badges), (1, 1));
        assert!(!valid_badge(b"not an image"));
    }
}

/// Counts for this manual operation; downloaded IDs are counted on local merge.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct SyncSummary {
    pub uploaded: usize,
    pub retry_uploads: bool,
}
impl SyncSummary {
    pub fn message(self, downloaded: usize) -> String {
        let counts = format!("{} uploaded · {downloaded} downloaded", self.uploaded);
        if self.retry_uploads {
            format!("{counts} · Some uploads failed; retry Sync")
        } else {
            counts
        }
    }
}

/// Validated service unlocks, ready for an atomic merge by the owning writer.
pub struct Imported {
    pub summary: SyncSummary,
    pub game_id: u32,
    pub rom_hash: String,
    pub ids: Vec<u32>,
    pub username: String,
    pub confirmed_at: u64,
}
impl Imported {
    pub fn save(&self, root: &Path) -> Result<usize, Error> {
        let mut store = crate::progress::ProgressStore::open(
            &root.join("Achievements/Progress/GBA"),
            self.game_id,
            &self.rom_hash,
        )
        .map_err(|_| Error::Storage)?;
        store
            .merge_confirmed(&self.ids, &self.username, self.confirmed_at)
            .map_err(|_| Error::Storage)
    }
}
fn unlock_ids(response: &Value) -> Result<Vec<u32>, Error> {
    let entries = response
        .get("UserUnlocks")
        .and_then(Value::as_array)
        .ok_or(Error::InvalidUnlocks)?;
    if entries.len() > 10000 {
        return Err(Error::InvalidUnlocks);
    }
    let mut ids = Vec::new();
    for entry in entries {
        let id = entry
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .filter(|id| *id != 0)
            .ok_or(Error::InvalidUnlocks)?;
        // RA includes a synthetic client warning in casual unlock responses.
        // It is a service notice, not an earned achievement or malformed data.
        if !crate::is_service_warning(id) {
            ids.push(id);
        }
    }
    Ok(ids)
}

pub fn fetch_unlocks(
    root: &Path,
    rom: &Path,
    account: &Account,
    cancel: &AtomicBool,
) -> Result<Imported, Error> {
    fetch_unlocks_with(root, rom, account, cancel, &Live(agent()))
}
fn fetch_unlocks_with(
    root: &Path,
    rom: &Path,
    account: &Account,
    cancel: &AtomicBool,
    transport: &impl Transport,
) -> Result<Imported, Error> {
    check(cancel)?;
    let pack = crate::pack::load_for_rom(&root.join("Achievements/GBA"), rom)
        .map_err(|_| Error::LocalPack)?
        .ok_or(Error::Unsupported)?;
    let game_id = pack.game_id.to_string();
    let mut ids = BTreeSet::new();
    // Import both softcore and hardcore earned IDs into this local softcore history.
    for hardcore in ["0", "1"] {
        let response = transport
            .request(
                &[
                    ("r", "unlocks"),
                    ("u", &account.username),
                    ("t", &account.token),
                    ("g", &game_id),
                    ("h", hardcore),
                ],
                cancel,
            )
            .map_err(|error| match error {
                Error::InvalidPack => Error::InvalidUnlocks,
                other => other,
            })?;
        ids.extend(unlock_ids(&response)?);
    }
    let known: BTreeSet<_> = pack.achievements.iter().map(|a| a.id).collect();
    ids.retain(|id| known.contains(id));
    check(cancel)?;
    if hash_gba_rom(rom).map_err(|_| Error::RomChanged)? != pack.rom_hash {
        return Err(Error::RomChanged);
    }
    Ok(Imported {
        summary: SyncSummary::default(),
        game_id: pack.game_id,
        rom_hash: pack.rom_hash,
        ids: ids.into_iter().collect(),
        username: account.username.clone(),
        confirmed_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    })
}

/// User-requested two-way sync. Upload receipts live separately from the active
/// emulator's history, so the network worker never overwrites its in-memory store.
pub fn sync_progress(
    root: &Path,
    rom: &Path,
    account: &Account,
    cancel: &AtomicBool,
) -> Result<Imported, Error> {
    sync_with(root, rom, account, cancel, &Live(agent()))
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct UploadQueue {
    version: u32,
    game_id: u32,
    rom_hash: String,
    entries: std::collections::BTreeMap<u32, UploadEntry>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct UploadEntry {
    username: String,
    earned_at: u64,
    confirmed: bool,
}
fn save_queue(path: &Path, queue: &UploadQueue) -> Result<(), Error> {
    std::fs::create_dir_all(path.parent().ok_or(Error::Storage)?).map_err(|_| Error::Storage)?;
    let bytes = serde_json::to_vec_pretty(queue).map_err(|_| Error::Storage)?;
    slot_store::atomic_write(path, &bytes).map_err(|_| Error::Storage)
}
fn signature(id: u32, username: &str, elapsed: u32) -> Result<String, Error> {
    let mut value = format!("{id}{username}0");
    if elapsed != 0 {
        value.push_str(&format!("{id}{elapsed}"));
    }
    let size = i32::try_from(value.len()).map_err(|_| Error::Upload)?;
    let mut digest = [0u8; 16];
    extern "C" {
        fn slot_rc_md5(data: *const u8, size: i32, digest: *mut u8);
    }
    // SAFETY: the synchronous C function reads size bytes and writes 16 bytes.
    unsafe {
        slot_rc_md5(value.as_ptr(), size, digest.as_mut_ptr());
    }
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}
fn sync_with(
    root: &Path,
    rom: &Path,
    account: &Account,
    cancel: &AtomicBool,
    transport: &impl Transport,
) -> Result<Imported, Error> {
    if !crate::mode::enabled(root).map_err(|_| Error::Storage)?
        || crate::mode::read(root).map_err(|_| Error::Storage)? != crate::mode::Mode::SyncEnabled
    {
        return Err(Error::Upload);
    }
    if account.username.trim().is_empty()
        || account.username.len() > 256
        || account.token.is_empty()
    {
        return Err(Error::Login);
    }
    let mut imported = fetch_unlocks_with(root, rom, account, cancel, transport)?;
    let store = crate::progress::ProgressStore::open(
        &root.join("Achievements/Progress/GBA"),
        imported.game_id,
        &imported.rom_hash,
    )
    .map_err(|_| Error::Storage)?;
    let path = root
        .join("Achievements/Uploads/GBA")
        .join(format!("{}.json", imported.rom_hash));
    let mut queue: UploadQueue = match std::fs::File::open(&path) {
        Ok(file) => {
            let mut bytes = Vec::new();
            file.take(MAX_RESPONSE as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| Error::Storage)?;
            if bytes.len() > MAX_RESPONSE {
                return Err(Error::Storage);
            }
            serde_json::from_slice(&bytes).map_err(|_| Error::Storage)?
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => UploadQueue {
            version: 1,
            game_id: imported.game_id,
            rom_hash: imported.rom_hash.clone(),
            entries: Default::default(),
        },
        Err(_) => return Err(Error::Storage),
    };
    if queue.version != 1
        || queue.game_id != imported.game_id
        || queue.rom_hash != imported.rom_hash
        || queue.entries.len() > 10000
        || queue.entries.iter().any(|(id, entry)| {
            *id == 0
                || crate::is_service_warning(*id)
                || entry.earned_at == 0
                || entry.username.trim().is_empty()
                || entry.username.len() > 256
        })
    {
        return Err(Error::Storage);
    }
    let pack = crate::pack::load_for_rom(&root.join("Achievements/GBA"), rom)
        .map_err(|_| Error::LocalPack)?
        .ok_or(Error::Unsupported)?;
    let known: BTreeSet<_> = pack.achievements.iter().map(|a| a.id).collect();
    // Explicit Sync assigns existing locally earned awards to this account once.
    // Imported awards are never uploaded to another account.
    for (id, unlock) in &store.history().unlocks {
        if known.contains(id)
            && unlock.earned_at != 0
            && matches!(
                unlock.sync,
                crate::progress::SyncStatus::Local | crate::progress::SyncStatus::PendingSync
            )
        {
            queue.entries.entry(*id).or_insert_with(|| UploadEntry {
                username: account.username.clone(),
                earned_at: unlock.earned_at,
                confirmed: false,
            });
        }
    }
    save_queue(&path, &queue)?;
    let ids: Vec<_> = queue.entries.keys().copied().collect();
    for id in ids {
        check(cancel)?;
        let entry = queue.entries.get(&id).ok_or(Error::Storage)?;
        if !known.contains(&id) || !entry.username.eq_ignore_ascii_case(&account.username) {
            continue;
        }
        if entry.confirmed || imported.ids.contains(&id) {
            if !entry.confirmed {
                queue.entries.get_mut(&id).ok_or(Error::Storage)?.confirmed = true;
                save_queue(&path, &queue)?;
            }
            if !imported.ids.contains(&id) {
                imported.ids.push(id);
            }
            continue;
        }
        if hash_gba_rom(rom).map_err(|_| Error::RomChanged)? != imported.rom_hash {
            return Err(Error::RomChanged);
        }
        let elapsed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .saturating_sub(entry.earned_at)
            .min(14 * 24 * 60 * 60) as u32;
        let signature = signature(id, &account.username, elapsed)?;
        let id_string = id.to_string();
        let elapsed_string = elapsed.to_string();
        let mut fields = vec![
            ("r", "awardachievement"),
            ("u", &account.username),
            ("t", &account.token),
            ("a", &id_string),
            ("h", "0"),
            ("m", &imported.rom_hash),
            ("v", &signature),
        ];
        if elapsed != 0 {
            fields.push(("o", &elapsed_string));
        }
        let response = match transport.request(&fields, cancel) {
            Ok(response) => response,
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            Err(_) => {
                imported.summary.retry_uploads = true;
                break;
            }
        };
        if response.get("Success").and_then(Value::as_bool) != Some(true)
            || number(&response, "AchievementID").ok() != Some(id)
        {
            imported.summary.retry_uploads = true;
            break;
        }
        imported.summary.uploaded += 1;
        queue.entries.get_mut(&id).ok_or(Error::Storage)?.confirmed = true;
        save_queue(&path, &queue)?;
        imported.ids.push(id);
    }
    imported.ids.sort_unstable();
    imported.ids.dedup();
    Ok(imported)
}

#[cfg(test)]
mod import_tests {
    use super::*;
    struct Fake(std::cell::RefCell<Vec<String>>);
    impl Transport for Fake {
        fn request(&self, fields: &[(&str, &str)], cancel: &AtomicBool) -> Result<Value, Error> {
            check(cancel)?;
            assert_eq!(fields.iter().find(|(k, _)| *k == "r").unwrap().1, "unlocks");
            let mode = fields.iter().find(|(k, _)| *k == "h").unwrap().1;
            self.0.borrow_mut().push(mode.into());
            Ok(if mode == "0" {
                serde_json::json!({"UserUnlocks":[1,999,101000001]})
            } else {
                serde_json::json!({"UserUnlocks":[1,2]})
            })
        }
        fn badge(&self, _: &str, _: &AtomicBool) -> Result<Vec<u8>, Error> {
            panic!("progress import requested an image");
        }
    }
    #[test]
    fn service_notices_are_ignored_but_malformed_unlocks_are_rejected() {
        assert_eq!(
            unlock_ids(&serde_json::json!({"UserUnlocks":[101000001]})).unwrap(),
            Vec::<u32>::new()
        );
        assert_eq!(
            unlock_ids(&serde_json::json!({"UserUnlocks":[]})).unwrap(),
            Vec::<u32>::new()
        );
        for invalid in [
            serde_json::json!([0]),
            serde_json::json!([-1]),
            serde_json::json!([4294967296u64]),
            serde_json::json!(["1"]),
            serde_json::json!([{}]),
        ] {
            assert_eq!(
                unlock_ids(&serde_json::json!({"UserUnlocks":invalid})).unwrap_err(),
                Error::InvalidUnlocks
            );
        }
    }
    #[test]
    fn imports_union_both_modes_and_filter_ids_against_the_local_pack() {
        let root = tempfile::tempdir().unwrap();
        let rom = root.path().join("game.gba");
        std::fs::write(&rom, b"abc").unwrap();
        let hash = hash_gba_rom(&rom).unwrap();
        let directory = root.path().join("Achievements/GBA");
        std::fs::create_dir_all(&directory).unwrap();
        let achievements = [1, 2]
            .into_iter()
            .map(|id| Achievement {
                id,
                title: format!("Achievement {id}"),
                description: "Test".into(),
                points: 5,
                rule: "0xH008000=3".into(),
                badge: None,
            })
            .collect();
        let pack = GbaPack {
            version: 1,
            console_id: 5,
            game_id: 42,
            title: "Test".into(),
            rom_hash: hash.clone(),
            achievements,
        };
        std::fs::write(
            directory.join(format!("{hash}.json")),
            serde_json::to_vec(&pack).unwrap(),
        )
        .unwrap();
        let fake = Fake(std::cell::RefCell::new(vec![]));
        let imported = fetch_unlocks_with(
            root.path(),
            &rom,
            &Account {
                username: "Player".into(),
                token: "token".into(),
            },
            &AtomicBool::new(false),
            &fake,
        )
        .unwrap();
        assert_eq!(imported.ids, [1, 2]);
        assert_eq!(*fake.0.borrow(), ["0", "1"]);
        assert!(!root.path().join("Achievements/Progress").exists());
        assert_eq!(imported.save(root.path()).unwrap(), 2);
        assert!(unlock_ids(&serde_json::json!({"UserUnlocks":[0]})).is_err());
        assert!(unlock_ids(&serde_json::json!({"UserUnlocks":"invalid"})).is_err());
    }
}

#[cfg(test)]
mod push_tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    struct Fake {
        awards: RefCell<Vec<u32>>,
        fail: Cell<bool>,
        wrong_id: Cell<bool>,
    }
    impl Transport for Fake {
        fn request(&self, fields: &[(&str, &str)], _: &AtomicBool) -> Result<Value, Error> {
            let get = |key| fields.iter().find(|(k, _)| *k == key).unwrap().1;
            match get("r") {
                "unlocks" => Ok(serde_json::json!({"UserUnlocks":[3,101000001]})),
                "awardachievement" => {
                    assert_eq!(get("h"), "0");
                    assert_eq!(get("m").len(), 32);
                    assert!(!get("v").is_empty());
                    if let Some((_, offset)) = fields.iter().find(|(k, _)| *k == "o") {
                        assert!(offset.parse::<u32>().unwrap() <= 14 * 24 * 60 * 60);
                    }
                    let id: u32 = get("a").parse().unwrap();
                    self.awards.borrow_mut().push(id);
                    if id == 2 && self.fail.get() {
                        return Err(Error::Network);
                    }
                    Ok(
                        serde_json::json!({"Success":true,"AchievementID":if self.wrong_id.get() {999} else {id}}),
                    )
                }
                _ => panic!("unexpected request"),
            }
        }
        fn badge(&self, _: &str, _: &AtomicBool) -> Result<Vec<u8>, Error> {
            panic!("unexpected badge")
        }
    }
    #[test]
    fn push_is_manual_softcore_account_bound_and_resumes_after_partial_failure() {
        let root = tempfile::tempdir().unwrap();
        let rom = root.path().join("game.gba");
        std::fs::write(&rom, b"abc").unwrap();
        let hash = hash_gba_rom(&rom).unwrap();
        let pack = GbaPack {
            version: 1,
            console_id: 5,
            game_id: 42,
            title: "Test".into(),
            rom_hash: hash.clone(),
            achievements: (1..=4)
                .map(|id| Achievement {
                    id,
                    title: format!("Award {id}"),
                    description: "Test".into(),
                    points: 5,
                    rule: "0xH008000=3".into(),
                    badge: None,
                })
                .collect(),
        };
        let dir = root.path().join("Achievements/GBA");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(format!("{hash}.json")),
            serde_json::to_vec(&pack).unwrap(),
        )
        .unwrap();
        let mut store = crate::progress::ProgressStore::open(
            &root.path().join("Achievements/Progress/GBA"),
            42,
            &hash,
        )
        .unwrap();
        store.record(&[1, 2, 999], 123).unwrap();
        store.merge_confirmed(&[4], "SomeoneElse", 124).unwrap();
        let fake = Fake {
            awards: RefCell::new(vec![]),
            fail: Cell::new(true),
            wrong_id: Cell::new(false),
        };
        let account = Account {
            username: "Player".into(),
            token: "token".into(),
        };
        let cancel = AtomicBool::new(false);
        assert!(sync_with(root.path(), &rom, &account, &cancel, &fake).is_err());
        assert!(fake.awards.borrow().is_empty());
        crate::mode::write(root.path(), crate::mode::Mode::SyncEnabled).unwrap();
        let partial = sync_with(root.path(), &rom, &account, &cancel, &fake).unwrap();
        assert!(partial.summary.retry_uploads);
        assert_eq!(partial.summary.uploaded, 1);
        assert_eq!(partial.ids, [1, 3]);
        assert_eq!(*fake.awards.borrow(), [1, 2]);
        let history_before = std::fs::read(
            root.path()
                .join(format!("Achievements/Progress/GBA/{hash}.json")),
        )
        .unwrap();
        fake.awards.borrow_mut().clear();
        let other = Account {
            username: "Other".into(),
            token: "token".into(),
        };
        sync_with(root.path(), &rom, &other, &cancel, &fake).unwrap();
        assert!(fake.awards.borrow().is_empty());
        fake.fail.set(false);
        fake.wrong_id.set(true);
        let partial = sync_with(root.path(), &rom, &account, &cancel, &fake).unwrap();
        assert!(partial.summary.retry_uploads);
        fake.awards.borrow_mut().clear();
        fake.wrong_id.set(false);
        let result = sync_with(root.path(), &rom, &account, &cancel, &fake).unwrap();
        assert_eq!(*fake.awards.borrow(), [2]);
        assert_eq!(result.ids, [1, 2, 3]);
        assert_eq!(result.summary.uploaded, 1);
        assert!(!result.summary.retry_uploads);
        assert_eq!(
            std::fs::read(
                root.path()
                    .join(format!("Achievements/Progress/GBA/{hash}.json"))
            )
            .unwrap(),
            history_before,
            "network worker must not overwrite active history"
        );
        assert_eq!(result.save(root.path()).unwrap(), 1);
        fake.awards.borrow_mut().clear();
        let repeated = sync_with(root.path(), &rom, &account, &cancel, &fake).unwrap();
        assert_eq!(repeated.summary.uploaded, 0);
        assert_eq!(repeated.save(root.path()).unwrap(), 0);
        assert!(fake.awards.borrow().is_empty());
        assert_eq!(store.history().unlocks[&1].earned_at, 123);
        let queue_path = root
            .path()
            .join(format!("Achievements/Uploads/GBA/{hash}.json"));
        std::fs::write(&queue_path, b"corrupt receipt").unwrap();
        assert_eq!(
            sync_with(root.path(), &rom, &account, &cancel, &fake).err(),
            Some(Error::Storage)
        );
        assert_eq!(std::fs::read(&queue_path).unwrap(), b"corrupt receipt");
        cancel.store(true, Ordering::Relaxed);
        assert_eq!(
            sync_with(root.path(), &rom, &account, &cancel, &fake).err(),
            Some(Error::Cancelled)
        );
        assert!(fake.awards.borrow().is_empty());
    }
    #[test]
    fn sync_feedback_counts_only_this_attempt_and_explains_retry() {
        assert_eq!(
            SyncSummary::default().message(0),
            "0 uploaded · 0 downloaded"
        );
        assert_eq!(
            SyncSummary {
                uploaded: 3,
                retry_uploads: false
            }
            .message(2),
            "3 uploaded · 2 downloaded"
        );
        assert_eq!(
            SyncSummary {
                uploaded: 1,
                retry_uploads: true
            }
            .message(2),
            "1 uploaded · 2 downloaded · Some uploads failed; retry Sync"
        );
    }
    #[test]
    fn signatures_match_softcore_award_protocol() {
        // Independently generated MD5 vectors for id + user + mode [+ id + offset].
        assert_eq!(
            signature(1234, "Username", 0).unwrap(),
            "ed81d6ecf825f8cbe3ae1edace098892"
        );
        assert_eq!(
            signature(1234, "Username", 17).unwrap(),
            "3fc1d9e9a8b93fd2d9b2e58be18c7c7c"
        );
    }
}
