use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use slot::achievements::{AchievementEvent, LocalAchievement};
use slot::audio::Ring;
use slot::emu::{CoreState, EmuHandle, Speed};
use slot_retro::{AvInfo, ButtonMask, CoreError, RetroCore};

struct MemoryCore {
    frame: u64,
    input: u8,
    reads: Arc<AtomicUsize>,
    video: Vec<u8>,
    memory_available: bool,
}

impl RetroCore for MemoryCore {
    fn load(&mut self, _: &Path) -> Result<(), CoreError> {
        Ok(())
    }
    fn run_frame(&mut self, input: ButtonMask) {
        self.frame += 1;
        self.input = input.0 as u8;
    }
    fn video_xrgb8888(&self) -> &[u8] {
        &self.video
    }
    fn take_audio(&mut self) -> Vec<i16> {
        Vec::new()
    }
    fn serialize(&mut self) -> Result<Vec<u8>, CoreError> {
        Ok(self.frame.to_le_bytes().to_vec())
    }
    fn unserialize(&mut self, bytes: &[u8]) -> Result<(), CoreError> {
        self.frame = u64::from_le_bytes(bytes.try_into().unwrap());
        Ok(())
    }
    fn save_ram(&self) -> Option<Vec<u8>> {
        None
    }
    fn load_save_ram(&mut self, _: &[u8]) -> Result<(), CoreError> {
        Ok(())
    }
    fn av_info(&self) -> AvInfo {
        AvInfo {
            fps: 60.0,
            sample_rate: 32768.0,
        }
    }
    fn read_memory(&mut self, address: u32, output: &mut [u8]) -> usize {
        self.reads.fetch_add(1, Ordering::Relaxed);
        if !self.memory_available {
            return 0;
        }
        for (i, byte) in output.iter_mut().enumerate() {
            *byte = match address + i as u32 {
                0x02000000 => self.frame as u8,
                0x02000001 => self.input,
                _ => 0,
            };
        }
        output.len()
    }
}

fn worker(memory_available: bool) -> (EmuHandle, Arc<AtomicUsize>) {
    let reads = Arc::new(AtomicUsize::new(0));
    let emu = EmuHandle::spawn(
        Box::new(MemoryCore {
            frame: 0,
            input: 0,
            reads: reads.clone(),
            video: vec![0; 240 * 160 * 4],
            memory_available,
        }),
        PathBuf::from("local-test.gba"),
        Arc::new(Ring::new(4096)),
        None,
        None,
    );
    wait(|| emu.state() == CoreState::Ready);
    (emu, reads)
}

fn wait(mut condition: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(2);
    while !condition() {
        assert!(
            Instant::now() < until,
            "worker did not reach expected state"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn install(emu: &EmuHandle, rule: &str) {
    emu.set_gba_achievements(vec![LocalAchievement {
        id: 42,
        title: "Test achievement".into(),
        rule: rule.into(),
    }])
    .recv_timeout(Duration::from_secs(2))
    .unwrap()
    .unwrap();
}

fn state(emu: &EmuHandle) -> Vec<u8> {
    emu.request_state()
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
}

fn pause(emu: &EmuHandle) {
    emu.set_speed(Speed::Paused);
    // Command completion is a worker barrier; allow any already-running frame to finish.
    state(emu);
}

#[test]
fn normal_and_fast_play_emit_once_even_on_unpresented_steps() {
    for speed in [Speed::Normal, Speed::Fast] {
        let (emu, _) = worker(true);
        install(&emu, "0xH008000=3");
        emu.set_speed(speed);
        let mut events = Vec::new();
        wait(|| {
            events.extend(emu.take_achievement_events());
            !events.is_empty()
        });
        assert_eq!(events, [AchievementEvent::Unlocked(42)]);
        pause(&emu);
        emu.request_load(0u64.to_le_bytes().to_vec());
        state(&emu);
        emu.set_speed(speed);
        wait(|| u64::from_le_bytes(state(&emu).try_into().unwrap()) > 6);
        pause(&emu);
        assert!(
            emu.take_achievement_events().is_empty(),
            "loading a state re-awarded an earned ID"
        );
    }
}

#[test]
fn paused_refresh_frames_do_not_read_achievement_memory_or_unlock() {
    let (emu, reads) = worker(true);
    install(&emu, "0xH008001=0");
    emu.set_input(ButtonMask(ButtonMask::B));
    emu.set_speed(Speed::Normal);
    wait(|| reads.load(Ordering::Relaxed) >= 2);
    pause(&emu);
    let before = reads.load(Ordering::Relaxed);
    emu.refresh();
    state(&emu);
    assert_eq!(reads.load(Ordering::Relaxed), before);
    assert!(emu.take_achievement_events().is_empty());
    emu.set_input(ButtonMask(0));
    emu.set_speed(Speed::Normal);
    let mut events = Vec::new();
    wait(|| {
        events.extend(emu.take_achievement_events());
        !events.is_empty()
    });
    assert_eq!(events, [AchievementEvent::Unlocked(42)]);
}

#[test]
fn failed_memory_stops_evaluation_once_and_empty_rules_disable_it() {
    let (emu, reads) = worker(false);
    install(&emu, "0xH008000=0");
    emu.set_speed(Speed::Normal);
    let mut events = Vec::new();
    wait(|| {
        events.extend(emu.take_achievement_events());
        !events.is_empty()
    });
    assert!(matches!(
        &events[..],
        [AchievementEvent::EvaluationStopped(_)]
    ));
    pause(&emu);
    let before = reads.load(Ordering::Relaxed);
    emu.set_speed(Speed::Fast);
    wait(|| u64::from_le_bytes(state(&emu).try_into().unwrap()) > 6);
    pause(&emu);
    assert_eq!(reads.load(Ordering::Relaxed), before);
    assert!(emu.take_achievement_events().is_empty());
    emu.set_gba_achievements(Vec::new())
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();
}

#[test]
fn invalid_replacement_keeps_the_previous_rules() {
    let (emu, _) = worker(true);
    install(&emu, "0xH008000=3");
    assert!(emu
        .set_gba_achievements(vec![LocalAchievement {
            id: 1,
            title: "Invalid test".into(),
            rule: "invalid".into()
        }])
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .is_err());
    emu.set_speed(Speed::Normal);
    let mut events = Vec::new();
    wait(|| {
        events.extend(emu.take_achievement_events());
        !events.is_empty()
    });
    assert_eq!(events, [AchievementEvent::Unlocked(42)]);
}

#[test]
fn rewind_frames_do_not_evaluate_achievement_rules() {
    let (emu, reads) = worker(true);
    install(&emu, "0xH008000=200");
    emu.set_speed(Speed::Normal);
    wait(|| reads.load(Ordering::Relaxed) >= 12);
    emu.set_rewinding(true);
    state(&emu);
    let before_reads = reads.load(Ordering::Relaxed);
    let before_frames = emu.published_count();
    wait(|| emu.published_count() >= before_frames + 2);
    pause(&emu);
    assert_eq!(reads.load(Ordering::Relaxed), before_reads);
    assert!(emu.take_achievement_events().is_empty());
}

fn persistent_rules(emu: &EmuHandle, directory: &std::path::Path) {
    emu.set_persistent_gba_achievements(
        vec![LocalAchievement {
            id: 42,
            title: "Persistent test".into(),
            rule: "0xH008000=3".into(),
        }],
        slot::achievements::ProgressConfig {
            directory: directory.into(),
            game_id: 4969,
            rom_hash: "f877b87f140945ad93cfce50411507c0".into(),
        },
    )
    .recv_timeout(Duration::from_secs(2))
    .unwrap()
    .unwrap();
}

#[test]
fn saved_unlock_is_not_awarded_again_by_a_new_game_worker() {
    let dir = tempfile::tempdir().unwrap();
    let (emu, _) = worker(true);
    persistent_rules(&emu, dir.path());
    emu.set_speed(Speed::Normal);
    let mut events = Vec::new();
    wait(|| {
        events.extend(emu.take_achievement_events());
        !events.is_empty()
    });
    assert_eq!(events, [AchievementEvent::Unlocked(42)]);
    drop(emu);
    let history = slot_achievements::progress::ProgressStore::open(
        dir.path(),
        4969,
        "f877b87f140945ad93cfce50411507c0",
    )
    .unwrap();
    assert_eq!(
        history.history().unlocks[&42].sync,
        slot_achievements::progress::SyncStatus::Local
    );
    let (emu, _) = worker(true);
    persistent_rules(&emu, dir.path());
    emu.set_speed(Speed::Fast);
    wait(|| u64::from_le_bytes(state(&emu).try_into().unwrap()) > 6);
    pause(&emu);
    assert!(emu.take_achievement_events().is_empty());
}

#[test]
fn a_failed_unlock_write_reports_failure_instead_of_showing_success() {
    let dir = tempfile::tempdir().unwrap();
    let (emu, _) = worker(true);
    persistent_rules(&emu, dir.path());
    std::fs::create_dir(dir.path().join("f877b87f140945ad93cfce50411507c0.json")).unwrap();
    emu.set_speed(Speed::Normal);
    let mut events = Vec::new();
    wait(|| {
        events.extend(emu.take_achievement_events());
        !events.is_empty()
    });
    assert!(
        matches!(&events[..], [AchievementEvent::EvaluationStopped(message)] if message.contains("could not be saved"))
    );
}

#[test]
fn corrupt_history_stops_tracking_without_overwriting_or_stopping_gameplay() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("f877b87f140945ad93cfce50411507c0.json");
    std::fs::write(&path, b"corrupt history").unwrap();
    let (emu, _) = worker(true);
    let result = emu
        .set_persistent_gba_achievements(
            vec![LocalAchievement {
                id: 42,
                title: "Test".into(),
                rule: "0xH008000=3".into(),
            }],
            slot::achievements::ProgressConfig {
                directory: dir.path().into(),
                game_id: 4969,
                rom_hash: "f877b87f140945ad93cfce50411507c0".into(),
            },
        )
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    assert!(result.is_err());
    assert!(matches!(
        &emu.take_achievement_events()[..],
        [AchievementEvent::EvaluationStopped(_)]
    ));
    emu.set_speed(Speed::Fast);
    wait(|| u64::from_le_bytes(state(&emu).try_into().unwrap()) > 6);
    pause(&emu);
    assert!(emu.take_achievement_events().is_empty());
    assert_eq!(std::fs::read(path).unwrap(), b"corrupt history");
}

#[test]
fn imported_unlocks_are_owned_by_the_emulation_worker_and_survive_future_awards() {
    let directory = tempfile::tempdir().unwrap();
    let (emu, _) = worker(true);
    emu.set_persistent_gba_achievements(
        vec![
            LocalAchievement {
                id: 42,
                title: "Imported".into(),
                rule: "0xH008000=3".into(),
            },
            LocalAchievement {
                id: 43,
                title: "Later".into(),
                rule: "0xH008000=5".into(),
            },
        ],
        slot::achievements::ProgressConfig {
            directory: directory.path().into(),
            game_id: 4969,
            rom_hash: "f877b87f140945ad93cfce50411507c0".into(),
        },
    )
    .recv_timeout(Duration::from_secs(2))
    .unwrap()
    .unwrap();
    let imported = slot_achievements::download::Imported {
        summary: Default::default(),
        game_id: 4969,
        rom_hash: "f877b87f140945ad93cfce50411507c0".into(),
        ids: vec![42],
        username: "Player".into(),
        confirmed_at: 123,
    };
    assert_eq!(
        emu.import_achievements(imported)
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .unwrap(),
        1
    );
    assert!(emu.take_achievement_events().is_empty());
    emu.set_speed(Speed::Normal);
    let mut events = Vec::new();
    wait(|| {
        events.extend(emu.take_achievement_events());
        !events.is_empty()
    });
    pause(&emu);
    assert_eq!(events, [AchievementEvent::Unlocked(43)]);
    let history = slot_achievements::progress::ProgressStore::open(
        directory.path(),
        4969,
        "f877b87f140945ad93cfce50411507c0",
    )
    .unwrap();
    assert!(history.history().unlocks.contains_key(&42));
    assert!(history.history().unlocks.contains_key(&43));
}

#[test]
fn measured_snapshot_works_while_paused_without_reading_core_memory() {
    let (emu, reads) = worker(true);
    install(&emu, "M:0xH008000=200");
    emu.set_speed(Speed::Normal);
    wait(|| reads.load(Ordering::Relaxed) > 3);
    pause(&emu);
    let before = reads.load(Ordering::Relaxed);
    let values = emu.achievement_progress().recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(values.len(),1);
    assert_eq!(values[0].0,42);
    assert!(values[0].1.ends_with("/200"));
    assert_eq!(reads.load(Ordering::Relaxed),before);
    assert!(emu.take_achievement_events().is_empty());
}
