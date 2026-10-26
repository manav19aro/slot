use slot_achievements::pack::{hash_gba_rom, load_for_rom, Achievement, GbaPack};
use std::io::ErrorKind;

fn pack(hash: &str) -> GbaPack {
    GbaPack {
        version: 1,
        console_id: 5,
        game_id: 1,
        title: "Synthetic test".into(),
        rom_hash: hash.into(),
        achievements: vec![Achievement {
            id: 1,
            title: "Test".into(),
            description: "Synthetic test rule".into(),
            points: 0,
            rule: "0xH008000=3".into(),
            badge: None,
        }],
    }
}

#[test]
fn hashes_contents_not_filenames_using_rcheevos() {
    let dir = tempfile::tempdir().unwrap();
    let rom = dir.path().join("first.gba");
    std::fs::write(&rom, b"abc").unwrap();
    assert_eq!(
        hash_gba_rom(&rom).unwrap(),
        "900150983cd24fb0d6963f7d28e17f72"
    );
    let renamed = dir.path().join("renamed.gba");
    std::fs::rename(&rom, &renamed).unwrap();
    assert_eq!(
        hash_gba_rom(&renamed).unwrap(),
        "900150983cd24fb0d6963f7d28e17f72"
    );
    std::fs::write(&renamed, b"abcd").unwrap();
    assert_ne!(
        hash_gba_rom(&renamed).unwrap(),
        "900150983cd24fb0d6963f7d28e17f72"
    );
}

#[test]
fn loads_only_a_valid_pack_for_the_exact_rom() {
    let dir = tempfile::tempdir().unwrap();
    let rom = dir.path().join("test.gba");
    std::fs::write(&rom, b"abc").unwrap();
    let cache = dir.path().join("packs");
    assert!(load_for_rom(&cache, &rom).unwrap().is_none());
    std::fs::create_dir(&cache).unwrap();
    let hash = hash_gba_rom(&rom).unwrap();
    let path = cache.join(format!("{hash}.json"));
    assert!(load_for_rom(&cache, &rom).unwrap().is_none());
    std::fs::write(&path, serde_json::to_vec(&pack(&hash)).unwrap()).unwrap();
    assert_eq!(
        load_for_rom(&cache, &rom)
            .unwrap()
            .unwrap()
            .achievements
            .len(),
        1
    );
    let mut wrong = pack(&hash);
    wrong.rom_hash = "00000000000000000000000000000000".into();
    std::fs::write(&path, serde_json::to_vec(&wrong).unwrap()).unwrap();
    assert_eq!(
        load_for_rom(&cache, &rom).unwrap_err().kind(),
        ErrorKind::InvalidData
    );
    std::fs::write(&path, b"broken JSON").unwrap();
    assert!(load_for_rom(&cache, &rom).is_err());
}

#[test]
fn rejects_wrong_console_version_duplicate_ids_and_invalid_rules() {
    let hash = "900150983cd24fb0d6963f7d28e17f72";
    let mut value = pack(hash);
    value.console_id = 4;
    assert!(value.validate(hash).is_err());
    value = pack(hash);
    value.version = 2;
    assert!(value.validate(hash).is_err());
    value = pack(hash);
    value.achievements.push(value.achievements[0].clone());
    assert!(value.validate(hash).is_err());
    value = pack(hash);
    value.achievements[0].rule = "invalid".into();
    assert!(value.validate(hash).is_err());
    assert!(pack(hash).validate("../not-a-hash").is_err());
}

#[test]
fn old_cached_service_warnings_are_removed_without_losing_real_achievements() {
    let dir = tempfile::tempdir().unwrap();
    let hash = "900150983cd24fb0d6963f7d28e17f72";
    let mut value = pack(hash);
    value.achievements[0].points = 0;
    let mut warning = value.achievements[0].clone();
    warning.id = 101_000_001;
    warning.title = "Warning: Unknown Emulator".into();
    warning.rule = "1=1.300.".into();
    value.achievements.push(warning);
    let path = dir.path().join("pack.json");
    std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    let loaded = GbaPack::read(&path, hash).unwrap();
    assert_eq!(loaded.achievements.len(), 1);
    assert_eq!(loaded.achievements[0].id, 1);
}
