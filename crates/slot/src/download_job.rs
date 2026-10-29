//! Saved setup and explicit operations sharing a temporary Wi-Fi connection.
use serde::{Deserialize, Serialize};
use slot_achievements::download::{self, Account, Prepared};
use slot_ui::{DownloadMode, DownloadSetup};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{channel, Receiver},
        Arc,
    },
};

#[derive(Default, Serialize, Deserialize)]
struct Settings {
    ssid: String,
    wifi_password: String,
    account: Account,
}
#[derive(Serialize, Deserialize)]
struct Wifi {
    ssid: String,
    wifi_password: String,
}
fn settings_path(root: &Path) -> PathBuf {
    root.join("Config/achievements-download.json")
}
fn read<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(32769)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > 32768 {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}
fn settings(root: &Path) -> Settings {
    // Read the first implementation's settings so existing sign-ins keep working.
    let mut saved: Settings = read(&settings_path(root)).unwrap_or_default();
    if let Some(wifi) = read::<Wifi>(&root.join("Config/wifi.json")) {
        saved.ssid = wifi.ssid;
        saved.wifi_password = wifi.wifi_password;
    }
    if let Some(account) = read::<Account>(&root.join("Config/retroachievements.json")) {
        saved.account = account;
    }
    saved
}
fn save(root: &Path, name: &str, value: &impl Serialize) -> Result<(), String> {
    let path = root.join("Config").join(name);
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|_| "Could not save settings")?;
    let bytes = serde_json::to_vec(value).map_err(|_| "Could not save settings")?;
    slot_store::atomic_write(&path, &bytes).map_err(|_| "Could not save settings")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}
pub fn save_wifi(root: &Path, setup: &DownloadSetup) -> Result<(), String> {
    save(
        root,
        "wifi.json",
        &Wifi {
            ssid: setup.ssid.clone(),
            wifi_password: setup.wifi_password.clone(),
        },
    )
}
pub fn setup(root: &Path) -> DownloadSetup {
    let saved = settings(root);
    DownloadSetup {
        ssid: saved.ssid,
        wifi_password: saved.wifi_password,
        username: saved.account.username,
        password: String::new(),
        saved_sign_in: !saved.account.token.is_empty(),
    }
}
#[derive(Default)]
pub struct BatchSummary {
    pub downloaded: usize,
    pub unsupported: usize,
    pub failed: usize,
    pub duplicates: usize,
    pub missing_badges: usize,
}
impl BatchSummary {
    pub fn message(&self) -> String {
        format!(
            "{} packs saved. {} unsupported, {} failed, {} duplicates. {} badges missing.",
            self.downloaded, self.unsupported, self.failed, self.duplicates, self.missing_badges
        )
    }
}
pub enum Outcome {
    Single(Prepared),
    Login,
    All(BatchSummary),
    SyncedGame(download::Imported),
    SyncedAll {
        imported: usize,
        games: usize,
        summary: download::SyncSummary,
    },
}
pub enum Progress {
    At(String),
    Finished(Result<Outcome, String>),
}
pub struct Job {
    pub rx: Receiver<Progress>,
    cancel: Arc<AtomicBool>,
}
impl Job {
    pub fn spawn(
        root: PathBuf,
        roms: Vec<PathBuf>,
        mut setup: DownloadSetup,
        mode: DownloadMode,
    ) -> std::io::Result<Self> {
        let (tx, rx) = channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        std::thread::Builder::new()
            .name("slot-pack-download".into())
            .spawn(move || {
                let result = (|| {
                    if !slot_achievements::mode::enabled(&root).unwrap_or(false) {
                        return Err("Achievements are disabled".into());
                    }
                    if mode != DownloadMode::Login && roms.is_empty() {
                        return Err("No GBA games found on the card".into());
                    }
                    if matches!(mode, DownloadMode::SyncGame | DownloadMode::SyncAll)
                        && slot_achievements::mode::read(&root).unwrap_or_default() != slot_achievements::mode::Mode::SyncEnabled {
                        return Err("Progress import is disabled in Local mode".into());
                    }
                    let _ = tx.send(Progress::At("Connecting to Wi-Fi".into()));
                    let _radio = Radio::connect(&root, &setup, &stop)?;
                    let saved = settings(&root);
                    let account = if setup.password.is_empty() {
                        if saved.account.username != setup.username
                            || saved.account.token.is_empty()
                        {
                            return Err(
                                "Set your RetroAchievements Login in Achievements settings".into(),
                            );
                        }
                        saved.account
                    } else {
                        let _ = tx.send(Progress::At("Signing in to RetroAchievements".into()));
                        let result = download::login(&setup.username, &setup.password, &stop)
                            .map_err(|e| e.message().to_owned());
                        setup.password.clear();
                        result?
                    };
                    if stop.load(Ordering::Relaxed) {
                        return Err("Download cancelled".into());
                    }
                    if mode == DownloadMode::Login {
                        save(&root, "retroachievements.json", &account)?;
                        return Ok(Outcome::Login);
                    }
                    if mode == DownloadMode::SyncGame {
                        let _ = tx.send(Progress::At("Syncing achievements".into()));
                        return download::sync_progress(&root, &roms[0], &account, &stop)
                            .map(Outcome::SyncedGame).map_err(|e| e.message().to_owned());
                    }
                    if mode == DownloadMode::SyncAll {
                        let mut seen = std::collections::BTreeSet::new();
                        let mut imported = 0; let mut games = 0;
                        let mut summary = download::SyncSummary::default();
                        for (index, rom) in roms.iter().enumerate() {
                            if stop.load(Ordering::Relaxed) { return Err("Download cancelled".into()); }
                            let pack = match slot_achievements::pack::load_for_rom(&root.join("Achievements/GBA"), rom) {
                                Ok(Some(pack)) => pack, Ok(None) => continue,
                                Err(_) => return Err(format!("Could not read a local pack. {imported} achievements imported so far")),
                            };
                            if !seen.insert(pack.rom_hash) { continue; }
                            let _ = tx.send(Progress::At(format!("Syncing progress: game {} / {}", index + 1, roms.len())));
                            let fetched = download::sync_progress(&root, rom, &account, &stop).map_err(|e| format!("{}. {}. Sync again to retry", e.message(), summary.message(imported)))?;
                            if stop.load(Ordering::Relaxed) { return Err("Download cancelled".into()); }
                            summary.uploaded += fetched.summary.uploaded;
                            summary.retry_uploads |= fetched.summary.retry_uploads;
                            imported += fetched.save(&root).map_err(|_| format!("Could not save local progress. {}. Sync again to retry", summary.message(imported)))?;
                            games += 1;
                        }
                        return Ok(Outcome::SyncedAll { imported, games, summary });
                    }
                    if mode == DownloadMode::All {
                        let mut current = 0;
                        let total = roms.len();
                        let summary = run_batch(&roms, &stop, |rom| {
                            current += 1;
                            download::prepare(&root, rom, &account, &stop, |line| {
                                let _ = tx.send(Progress::At(format!(
                                    "Game {current} / {total}: {line}"
                                )));
                            })
                        })?;
                        return Ok(Outcome::All(summary));
                    }
                    download::prepare(&root, &roms[0], &account, &stop, |line| {
                        let _ = tx.send(Progress::At(line));
                    })
                    .map(Outcome::Single)
                    .map_err(|e| e.message().to_owned())
                    // Radio drops before Finished is sent.
                })();
                let _ = tx.send(Progress::Finished(result));
            })?;
        Ok(Self { rx, cancel })
    }
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        self.cancel();
    }
}
fn run_batch(
    roms: &[PathBuf],
    cancel: &AtomicBool,
    mut prepare: impl FnMut(&Path) -> Result<Prepared, download::Error>,
) -> Result<BatchSummary, String> {
    let mut summary = BatchSummary::default();
    let mut seen = std::collections::BTreeSet::new();
    for rom in roms {
        if cancel.load(Ordering::Relaxed) {
            return Err("Download cancelled".into());
        }
        let hash = match slot_achievements::pack::hash_gba_rom(rom) {
            Ok(hash) => hash,
            Err(_) => {
                summary.failed += 1;
                continue;
            }
        };
        if !seen.insert(hash) {
            summary.duplicates += 1;
            continue;
        }
        match prepare(rom) {
            Ok(pack) => {
                summary.downloaded += 1;
                summary.missing_badges += pack.missing_badges;
            }
            Err(download::Error::Unsupported) => summary.unsupported += 1,
            Err(download::Error::Cancelled) => return Err("Download cancelled".into()),
            Err(
                error @ (download::Error::Network
                | download::Error::Login
                | download::Error::Storage),
            ) => return Err(format!("{} {}", error.message(), summary.message())),
            Err(_) => summary.failed += 1,
        }
    }
    Ok(summary)
}

struct Radio {
    #[cfg(feature = "device")]
    root: PathBuf,
}
impl Radio {
    fn connect(root: &Path, setup: &DownloadSetup, cancel: &AtomicBool) -> Result<Self, String> {
        #[cfg(not(feature = "device"))]
        {
            let _ = (root, setup, cancel);
            Ok(Self {})
        }
        #[cfg(feature = "device")]
        {
            use std::io::Write;
            use std::os::unix::{
                fs::{OpenOptionsExt, PermissionsExt},
                process::CommandExt,
            };
            let radio = Self { root: root.into() };
            let directory = std::env::var_os("SLOT_RUN")
                .map(PathBuf::from)
                .unwrap_or_else(|| "/run".into())
                .join("slot-download");
            std::fs::create_dir_all(directory.join("wpa"))
                .map_err(|_| "Could not prepare Wi-Fi")?;
            std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))
                .map_err(|_| "Could not prepare Wi-Fi")?;
            let conf = wifi_config(setup, &directory.join("wpa"));
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(directory.join("network.conf"))
                .map_err(|_| "Could not prepare Wi-Fi")?;
            file.write_all(conf.as_bytes())
                .map_err(|_| "Could not prepare Wi-Fi")?;
            let mut command = std::process::Command::new("/bin/sh");
            command
                .arg(root.join("System/slotdownload.sh"))
                .arg("up")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());
            // Cancel the connector and its DHCP child together; daemon cleanup runs on Drop.
            unsafe {
                command.pre_exec(|| {
                    if libc::setpgid(0, 0) == -1 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
            let mut child = command.spawn().map_err(|_| "Could not start Wi-Fi")?;
            loop {
                if cancel.load(Ordering::Relaxed) {
                    unsafe {
                        libc::kill(-(child.id() as i32), libc::SIGTERM);
                    }
                    let _ = child.wait();
                    return Err("Download cancelled".into());
                }
                match child.try_wait() {
                    Ok(Some(status)) if status.success() => return Ok(radio),
                    Ok(Some(_)) | Err(_) => {
                        return Err(
                            "Could not connect. Check your Wi-Fi network and password".into()
                        )
                    }
                    Ok(None) => std::thread::sleep(std::time::Duration::from_millis(50)),
                }
            }
        }
    }
}
#[cfg(feature = "device")]
impl Drop for Radio {
    fn drop(&mut self) {
        let _ = std::process::Command::new("/bin/sh")
            .arg(self.root.join("System/slotdownload.sh"))
            .arg("down")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}
#[cfg(any(feature = "device", test))]
fn wifi_config(setup: &DownloadSetup, control: &Path) -> String {
    let ssid: String = setup.ssid.bytes().map(|b| format!("{b:02x}")).collect();
    let auth = if setup.wifi_password.is_empty() {
        "key_mgmt=NONE".to_owned()
    } else if setup.wifi_password.len() == 64
        && setup.wifi_password.bytes().all(|b| b.is_ascii_hexdigit())
    {
        format!("key_mgmt=WPA-PSK\npsk={}", setup.wifi_password)
    } else {
        format!(
            "key_mgmt=WPA-PSK\npsk=\"{}\"",
            setup
                .wifi_password
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
        )
    };
    format!(
        "ctrl_interface={}\nnetwork={{\nscan_ssid=1\nssid={}\n{}\n}}\n",
        control.display(),
        ssid,
        auth
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wifi_config_encodes_ssid_and_escapes_password_without_shell_expansion() {
        let setup = DownloadSetup {
            ssid: "test\"\nnetwork={".into(),
            wifi_password: "pass\"word\\x".into(),
            ..Default::default()
        };
        let conf = wifi_config(&setup, Path::new("/run/wpa"));
        assert!(!conf.contains(&setup.ssid));
        assert!(conf.contains("psk=\"pass\\\"word\\\\x\""));
        assert_eq!(conf.matches("network={").count(), 1);
    }
    #[test]
    fn setup_reads_a_saved_token_without_restoring_a_password() {
        let root = tempfile::tempdir().unwrap();
        let path = settings_path(root.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let saved = Settings {
            ssid: "Home".into(),
            wifi_password: "password".into(),
            account: Account {
                username: "Player".into(),
                token: "secret-token".into(),
            },
        };
        std::fs::write(&path, serde_json::to_vec(&saved).unwrap()).unwrap();
        let view = setup(root.path());
        assert!(view.saved_sign_in);
        assert!(view.password.is_empty());
        assert_eq!(view.username, "Player");
    }
}

#[cfg(test)]
mod batch_tests {
    use super::*;
    #[test]
    fn batch_saves_compatible_games_skips_unknown_and_deduplicates_roms() {
        let root = tempfile::tempdir().unwrap();
        let mut roms = Vec::new();
        for (name, content) in [
            ("a", "abc"),
            ("b", "def"),
            ("c", "ghi"),
            ("duplicate", "abc"),
        ] {
            let path = root.path().join(name);
            std::fs::write(&path, content).unwrap();
            roms.push(path);
        }
        let mut calls = 0;
        let summary = run_batch(&roms, &AtomicBool::new(false), |path| {
            calls += 1;
            match path.file_name().unwrap().to_str().unwrap() {
                "a" => Ok(Prepared {
                    title: "Game".into(),
                    count: 5,
                    missing_badges: 2,
                }),
                "b" => Err(download::Error::Unsupported),
                _ => Err(download::Error::InvalidPack),
            }
        })
        .unwrap();
        assert_eq!(calls, 3);
        assert_eq!(
            (
                summary.downloaded,
                summary.unsupported,
                summary.failed,
                summary.duplicates,
                summary.missing_badges
            ),
            (1, 1, 1, 1, 2)
        );
    }
    #[test]
    fn batch_stops_when_cancelled_without_starting_requests() {
        let result = run_batch(&["game.gba".into()], &AtomicBool::new(true), |_| {
            panic!("cancelled batch issued a request")
        });
        assert!(matches!(result, Err(message) if message == "Download cancelled"));
    }
    #[test]
    fn separate_wifi_settings_preserve_saved_login_and_legacy_settings_work() {
        let root = tempfile::tempdir().unwrap();
        let legacy = Settings {
            ssid: "Old".into(),
            wifi_password: "oldpassword".into(),
            account: Account {
                username: "Player".into(),
                token: "token".into(),
            },
        };
        save(root.path(), "achievements-download.json", &legacy).unwrap();
        assert!(setup(root.path()).saved_sign_in);
        save_wifi(
            root.path(),
            &DownloadSetup {
                ssid: "New".into(),
                wifi_password: "newpassword".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let current = setup(root.path());
        assert_eq!(current.ssid, "New");
        assert_eq!(current.username, "Player");
        assert!(current.saved_sign_in);
        assert!(current.password.is_empty());
    }
}
