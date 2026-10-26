use slot_achievements::progress::{ProgressStore, SyncStatus};
const HASH: &str = "f877b87f140945ad93cfce50411507c0";

#[test]
fn history_survives_reopen_and_duplicate_awards_preserve_timestamps() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = ProgressStore::open(dir.path(), 4969, HASH).unwrap();
    assert!(store.history().unlocks.is_empty());
    store.record(&[37571], 123).unwrap();
    drop(store);
    let mut reopened = ProgressStore::open(dir.path(), 4969, HASH).unwrap();
    assert_eq!(reopened.history().unlocks[&37571].earned_at, 123);
    assert_eq!(reopened.history().unlocks[&37571].sync, SyncStatus::Local);
    reopened.record(&[37571, 37570], 456).unwrap();
    assert_eq!(reopened.history().unlocks[&37571].earned_at, 123);
    assert_eq!(reopened.history().unlocks[&37570].earned_at, 456);
}

#[test]
fn corrupt_or_mismatched_history_is_never_replaced_with_an_empty_one() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(format!("{HASH}.json"));
    std::fs::write(&path, b"corrupt").unwrap();
    assert!(ProgressStore::open(dir.path(), 4969, HASH).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"corrupt");
    std::fs::remove_file(&path).unwrap();
    let mut store = ProgressStore::open(dir.path(), 4969, HASH).unwrap();
    store.record(&[37571], 123).unwrap();
    let before = std::fs::read(&path).unwrap();
    assert!(ProgressStore::open(dir.path(), 1, HASH).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert!(ProgressStore::open(dir.path(), 4969, "../../invalid").is_err());
    assert!(store.record(&[101000001], 123).is_err());
}

#[test]
fn failed_write_does_not_claim_history_was_saved() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = ProgressStore::open(dir.path(), 4969, HASH).unwrap();
    let path = dir.path().join(format!("{HASH}.json"));
    std::fs::create_dir(&path).unwrap();
    assert!(store.record(&[37571], 123).is_err());
    assert!(store.history().unlocks.is_empty());
    assert!(path.is_dir());
}

#[test]
fn existing_server_confirmation_survives_a_duplicate_local_award() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = ProgressStore::open(dir.path(), 4969, HASH).unwrap();
    store.record(&[37571], 123).unwrap();
    let mut history = store.history().clone();
    history.unlocks.get_mut(&37571).unwrap().sync = SyncStatus::ServerConfirmed {
        username: "test-account".into(),
        confirmed_at: 456,
    };
    std::fs::write(
        dir.path().join(format!("{HASH}.json")),
        serde_json::to_vec(&history).unwrap(),
    )
    .unwrap();
    let mut reopened = ProgressStore::open(dir.path(), 4969, HASH).unwrap();
    reopened.record(&[37571, 37570], 789).unwrap();
    assert_eq!(reopened.history().unlocks[&37571], history.unlocks[&37571]);
    assert_eq!(reopened.history().unlocks[&37570].sync, SyncStatus::Local);
}

#[test]
fn imports_merge_without_losing_local_unlocks_or_original_timestamps() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = ProgressStore::open(dir.path(), 4969, HASH).unwrap();
    store.record(&[1, 2], 123).unwrap();
    assert_eq!(store.merge_confirmed(&[2, 3, 3], "Player", 456).unwrap(), 1);
    assert_eq!(store.history().unlocks[&1].sync, SyncStatus::Local);
    assert_eq!(store.history().unlocks[&2].earned_at, 123);
    assert_eq!(store.history().unlocks[&3].earned_at, 0);
    assert_eq!(store.history().unlocks.len(), 3);
    assert_eq!(store.merge_confirmed(&[2, 3], "Player", 456).unwrap(), 0);
    let reopened = ProgressStore::open(dir.path(), 4969, HASH).unwrap();
    assert_eq!(reopened.history().unlocks, store.history().unlocks);
}

#[test]
fn achievements_folder_transfers_progress_and_mode_to_a_fresh_install() {
    use slot_achievements::mode::{self, Mode};
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    assert_eq!(mode::read(first.path()).unwrap(), Mode::Local);
    mode::write(first.path(), Mode::SyncEnabled).unwrap();
    let directory = first.path().join("Achievements/Progress/GBA");
    let mut store = ProgressStore::open(&directory, 4969, HASH).unwrap();
    store.record(&[42], 123).unwrap();
    std::fs::create_dir_all(second.path().join("Achievements/Progress/GBA")).unwrap();
    std::fs::copy(
        first.path().join("Achievements/settings.json"),
        second.path().join("Achievements/settings.json"),
    )
    .unwrap();
    std::fs::copy(
        directory.join(format!("{HASH}.json")),
        second
            .path()
            .join(format!("Achievements/Progress/GBA/{HASH}.json")),
    )
    .unwrap();
    assert_eq!(mode::read(second.path()).unwrap(), Mode::SyncEnabled);
    let mut moved =
        ProgressStore::open(&second.path().join("Achievements/Progress/GBA"), 4969, HASH).unwrap();
    assert_eq!(moved.history().unlocks[&42].earned_at, 123);
    moved.record(&[43], 456).unwrap();
    assert!(!store.history().unlocks.contains_key(&43));
}

#[test]
fn legacy_pending_histories_remain_readable_and_earned() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = ProgressStore::open(dir.path(), 4969, HASH).unwrap();
    store.record(&[42], 123).unwrap();
    let mut legacy = store.history().clone();
    legacy.unlocks.get_mut(&42).unwrap().sync = SyncStatus::PendingSync;
    std::fs::write(
        dir.path().join(format!("{HASH}.json")),
        serde_json::to_vec(&legacy).unwrap(),
    )
    .unwrap();
    let reopened = ProgressStore::open(dir.path(), 4969, HASH).unwrap();
    assert_eq!(reopened.history().unlocks[&42].earned_at, 123);
}
