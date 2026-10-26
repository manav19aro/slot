//! Local achievement rules and events for a single emulation worker.
use std::sync::{Arc, Mutex};

use slot_achievements::GbaAchievements;
use slot_retro::RetroCore;

#[derive(Clone, Debug)]
pub struct LocalAchievement {
    pub id: u32,
    pub title: String,
    pub rule: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AchievementEvent {
    Unlocked(u32),
    /// Evaluation stopped; the caller must explicitly install rules again.
    EvaluationStopped(String),
}

#[derive(Clone, Debug)]
pub struct ProgressConfig {
    pub directory: std::path::PathBuf,
    pub game_id: u32,
    pub rom_hash: String,
}

pub(crate) type Events = Arc<Mutex<Vec<AchievementEvent>>>;

pub(crate) struct AchievementSession {
    runtime: Option<GbaAchievements>,
    progress: Option<slot_achievements::progress::ProgressStore>,
    titles: std::collections::BTreeMap<u32, String>,
    events: Events,
}

impl AchievementSession {
    pub(crate) fn new(events: Events) -> Self {
        Self {
            runtime: None,
            progress: None,
            titles: std::collections::BTreeMap::new(),
            events,
        }
    }

    /// Build transactionally; an invalid set leaves the previous evaluator intact.
    /// Empty rules disable evaluation. Installing a set starts a fresh session.
    pub(crate) fn install(
        &mut self,
        rules: Vec<LocalAchievement>,
        config: Option<ProgressConfig>,
    ) -> Result<(), String> {
        let progress = config
            .map(|config| {
                slot_achievements::progress::ProgressStore::open(
                    &config.directory,
                    config.game_id,
                    &config.rom_hash,
                )
            })
            .transpose()
            .map_err(|error| format!("cannot load achievement history: {error}"))?;
        let rules: Vec<_> = rules
            .into_iter()
            .filter(|a| {
                if slot_achievements::is_service_warning(a.id) {
                    eprintln!(
                        "slot: achievements: service notice {:?} excluded from unlock tracking",
                        a.title
                    );
                    false
                } else {
                    true
                }
            })
            .collect();
        let mut titles = std::collections::BTreeMap::new();
        let runtime = if rules.is_empty() {
            None
        } else {
            let mut runtime = GbaAchievements::new().map_err(|e| e.to_string())?;
            if let Some(store) = &progress {
                for id in store.history().unlocks.keys() {
                    runtime.restore_unlock(*id);
                }
                eprintln!(
                    "slot: achievements: restored {} earned IDs",
                    store.history().unlocks.len()
                );
            }
            let mut ids = std::collections::BTreeSet::new();
            for achievement in rules {
                if !ids.insert(achievement.id) {
                    return Err(format!("duplicate achievement ID {}", achievement.id));
                }
                runtime
                    .activate(achievement.id, &achievement.rule)
                    .map_err(|e| e.to_string())?;
                titles.insert(achievement.id, achievement.title);
            }
            Some(runtime)
        };
        self.runtime = runtime;
        self.progress = progress;
        self.titles = titles;
        eprintln!(
            "slot: achievements: {} local rules active",
            self.titles.len()
        );
        // Already emitted events belong to this game and must remain available
        // until consumed, even if the caller disables or replaces the rules.
        Ok(())
    }

    pub(crate) fn measured_progress(&self) -> Vec<(u32, String)> {
        self.runtime.as_ref().map(|runtime| runtime.measured_progress()).unwrap_or_default()
    }

    pub(crate) fn active(&self) -> bool {
        self.runtime.is_some()
    }

    pub(crate) fn import_unlocks(
        &mut self,
        imported: &slot_achievements::download::Imported,
    ) -> Result<usize, String> {
        let store = self
            .progress
            .as_mut()
            .ok_or("No active achievement history")?;
        if store.history().game_id != imported.game_id
            || store.history().rom_hash != imported.rom_hash
        {
            return Err("The running game changed. Try again".into());
        }
        let added = store
            .merge_confirmed(&imported.ids, &imported.username, imported.confirmed_at)
            .map_err(|_| "Could not save imported progress")?;
        if let Some(runtime) = self.runtime.as_mut() {
            for id in &imported.ids {
                runtime.restore_unlock(*id);
            }
        }
        Ok(added)
    }

    pub(crate) fn reset_progress(&mut self) {
        if let Some(runtime) = self.runtime.as_mut() {
            runtime.reset_progress();
        }
    }

    pub(crate) fn frame(&mut self, core: &mut dyn RetroCore) {
        let Some(runtime) = self.runtime.as_mut() else {
            return;
        };
        let result = runtime.evaluate_frame(|address, buffer| core.read_memory(address, buffer));
        let mut events = self.events.lock().unwrap_or_else(|e| e.into_inner());
        match result {
            Ok(ids) => {
                if !ids.is_empty() {
                    if let Some(store) = self.progress.as_mut() {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_secs())
                            .unwrap_or(0);
                        if let Err(error) = store.record(&ids, now) {
                            let message = format!("unlock history could not be saved: {error}");
                            eprintln!("slot: achievements: {message}");
                            events.push(AchievementEvent::EvaluationStopped(message));
                            self.runtime = None;
                            return;
                        }
                    }
                }
                for id in ids {
                    eprintln!(
                        "slot: achievements: unlocked {id} {:?}",
                        self.titles
                            .get(&id)
                            .map(String::as_str)
                            .unwrap_or("unknown")
                    );
                    events.push(AchievementEvent::Unlocked(id));
                }
            }
            Err(error) => {
                eprintln!("slot: achievements: evaluation stopped: {error}");
                events.push(AchievementEvent::EvaluationStopped(error.to_string()));
                self.runtime = None;
            }
        }
    }
}

pub(crate) fn browser(
    root: &std::path::Path,
    rom: &std::path::Path,
    supported: bool,
) -> slot_ui::AchievementView {
    use slot_ui::{AchievementRow, AchievementView};
    if !supported {
        return AchievementView::message(
            "Achievements",
            "Available for single-player GBA games using mGBA",
        );
    }
    let pack = match slot_achievements::pack::load_for_rom(&root.join("Achievements/GBA"), rom) {
        Ok(Some(pack)) => pack,
        Ok(None) => {
            return AchievementView::message(
                "Achievements",
                "No downloaded achievement pack for this game",
            )
        }
        Err(error) => {
            eprintln!("slot: achievement browser: {error}");
            return AchievementView::message(
                "Achievements",
                "Could not read the local achievement pack",
            );
        }
    };
    let history = match slot_achievements::progress::ProgressStore::open(
        &root.join("Achievements/Progress/GBA"),
        pack.game_id,
        &pack.rom_hash,
    ) {
        Ok(history) => history,
        Err(error) => {
            eprintln!("slot: achievement browser: {error}");
            return AchievementView::message(
                &pack.title,
                "Could not read saved achievement progress",
            );
        }
    };
    let rows = pack
        .achievements
        .into_iter()
        .map(|a| {
            let record = history.history().unlocks.get(&a.id);
            let status = if record.is_some() {
                "Unlocked"
            } else {
                "Locked"
            };
            AchievementRow {
                id: a.id,
                progress: None,
                badge: load_badge(root, a.badge.as_deref()),
                title: a.title,
                description: a.description,
                points: a.points,
                status,
                earned: record.is_some(),
            }
        })
        .collect();
    AchievementView {
        game: pack.title,
        rows,
        selected: 0,
        details: false,
        message: String::new(),
        can_download: false,
        can_sync: false,
    }
}

/// Cache-only image access. Badge names never become arbitrary filesystem paths.
fn load_badge(root: &std::path::Path, badge: Option<&str>) -> Option<slot_ui::BadgeImage> {
    let name = badge.filter(|name| {
        !name.is_empty() && name.len() <= 16 && name.bytes().all(|c| c.is_ascii_digit())
    })?;
    let path = root.join("Achievements/Badges").join(format!("{name}.png"));
    if std::fs::metadata(&path).ok()?.len() > 256 * 1024 {
        return None;
    }
    // Read dimensions before decoding to reject oversized or malformed cached assets.
    let file = std::io::BufReader::new(std::fs::File::open(&path).ok()?);
    let reader = png::Decoder::new(file).read_info().ok()?;
    let info = reader.info();
    if info.width == 0 || info.height == 0 || info.width > 256 || info.height > 256 {
        return None;
    }
    let (pixels, w, h) = slot_ui::decode_png(&path)?;
    let mut rgba = vec![0; 64 * 64 * 4];
    for y in 0..64u32 {
        for x in 0..64u32 {
            let source = (((y * h / 64) * w + x * w / 64) * 4) as usize;
            let target = ((y * 64 + x) * 4) as usize;
            rgba[target..target + 4].copy_from_slice(&pixels[source..source + 4]);
        }
    }
    Some(slot_ui::BadgeImage { rgba })
}

pub(crate) fn notice(
    root: &std::path::Path,
    rom: &std::path::Path,
    id: u32,
) -> slot_ui::AchievementNotice {
    let achievement = slot_achievements::pack::load_for_rom(&root.join("Achievements/GBA"), rom)
        .ok()
        .flatten()
        .and_then(|pack| pack.achievements.into_iter().find(|a| a.id == id));
    match achievement {
        Some(a) => slot_ui::AchievementNotice {
            badge: load_badge(root, a.badge.as_deref()),
            title: a.title,
        },
        None => slot_ui::AchievementNotice {
            title: format!("Achievement {id}"),
            badge: None,
        },
    }
}

#[cfg(test)]
mod browser_tests {
    use super::*;

    #[test]
    fn browser_combines_cached_definitions_and_saved_unlocks() {
        let root = tempfile::tempdir().unwrap();
        let rom = root.path().join("game.gba");
        std::fs::write(&rom, b"abc").unwrap();
        let hash = slot_achievements::pack::hash_gba_rom(&rom).unwrap();
        let cache = root.path().join("Achievements/GBA");
        std::fs::create_dir_all(&cache).unwrap();
        let pack = slot_achievements::pack::GbaPack {
            version: 1,
            console_id: 5,
            game_id: 1,
            title: "Test game".into(),
            rom_hash: hash.clone(),
            achievements: vec![slot_achievements::pack::Achievement {
                id: 1,
                title: "Test achievement".into(),
                description: "Test description".into(),
                points: 5,
                rule: "0xH008000=3".into(),
                badge: None,
            }],
        };
        // Use the pack's serializer through its JSON representation supplied below.
        std::fs::write(cache.join(format!("{hash}.json")), format!(
            "{{\"version\":1,\"console_id\":5,\"game_id\":1,\"title\":\"Test game\",\"rom_hash\":\"{hash}\",\"achievements\":[{{\"id\":1,\"title\":\"Test achievement\",\"description\":\"Test description\",\"points\":5,\"rule\":\"0xH008000=3\"}}]}}"
        )).unwrap();
        assert_eq!(browser(root.path(), &rom, true).earned(), 0);
        let mut progress = slot_achievements::progress::ProgressStore::open(
            &root.path().join("Achievements/Progress/GBA"),
            pack.game_id,
            &hash,
        )
        .unwrap();
        progress.record(&[1], 123).unwrap();
        let view = browser(root.path(), &rom, true);
        assert_eq!(view.earned(), 1);
        assert_eq!(view.rows[0].status, "Unlocked");
        assert_eq!(view.rows[0].points, 5);
        assert_eq!(notice(root.path(), &rom, 1).title, "Test achievement");
        assert_eq!(notice(root.path(), &rom, 999).title, "Achievement 999");
        assert!(browser(root.path(), &rom, false).rows.is_empty());
    }
}

#[cfg(test)]
mod badge_tests {
    use super::*;
    #[test]
    fn missing_corrupt_and_unsafe_badge_names_fall_back() {
        let root = tempfile::tempdir().unwrap();
        let cache = root.path().join("Achievements/Badges");
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::write(cache.join("123.png"), b"broken").unwrap();
        for name in [
            None,
            Some("123"),
            Some("456"),
            Some("../123"),
            Some("/123"),
            Some(""),
        ] {
            assert!(load_badge(root.path(), name).is_none());
        }
    }
    #[test]
    fn cached_badges_decode_to_a_small_fixed_size() {
        let root = tempfile::tempdir().unwrap();
        let cache = root.path().join("Achievements/Badges");
        std::fs::create_dir_all(&cache).unwrap();
        let file = std::fs::File::create(cache.join("123.png")).unwrap();
        let mut encoder = png::Encoder::new(file, 64, 64);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&[220, 40, 30, 255].repeat(64 * 64))
            .unwrap();
        assert_eq!(
            load_badge(root.path(), Some("123")).unwrap().rgba,
            [220, 40, 30, 255].repeat(64 * 64)
        );
    }
}
