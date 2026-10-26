use slot_achievements::GbaAchievements;
use slot_retro::{ButtonMask, LibretroCore, RetroCore};
use std::path::Path;

#[test]
#[ignore = "requires the pinned host mGBA library; see README.md"]
fn gba_test_rom_unlocks_one_local_achievement() {
    let extension = if cfg!(target_os = "macos") {
        "dylib"
    } else {
        "so"
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let library = root.join(format!("vendor/mgba_libretro.{extension}"));
    // Deliberately fail instead of reporting a skipped proof as success.
    assert!(
        library.exists(),
        "build the pinned host mGBA core first; see slot-achievements/README.md"
    );
    let mut core = LibretroCore::open(&library).unwrap();
    // The test program increments a halfword in EWRAM once per VBlank.
    let code: [u32; 15] = [
        0xe3a00404, 0xe3a01c04, 0xe3811003, 0xe5801000, 0xe3a02402, 0xe3a03000, 0xe1d040b6,
        0xe35400a0, 0x1afffffc, 0xe2833001, 0xe1c230b0, 0xe1d040b6, 0xe35400a0, 0x0afffffc,
        0xeafffff6,
    ];
    let mut rom = vec![0; 0x8000];
    rom[..4].copy_from_slice(&0xea00002eu32.to_le_bytes());
    rom[0xa0..0xac].copy_from_slice(b"SLOT ACHIEVE");
    rom[0xac..0xb0].copy_from_slice(b"SACH");
    rom[0xb0..0xb2].copy_from_slice(b"00");
    rom[0xb2] = 0x96;
    rom[0xbd] = 0u8
        .wrapping_sub(rom[0xa0..0xbd].iter().fold(0u8, |a, b| a.wrapping_add(*b)))
        .wrapping_sub(0x19);
    for (i, word) in code.iter().enumerate() {
        rom[0xc0 + i * 4..0xc4 + i * 4].copy_from_slice(&word.to_le_bytes());
    }
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join("slot-achievement.gba");
    std::fs::write(&path, rom).unwrap();
    core.load(&path).unwrap();
    let mut achievements = GbaAchievements::new().unwrap();
    achievements.activate(42, "0x008000=3").unwrap();
    let mut events = Vec::new();
    for _ in 0..30 {
        core.run_frame(ButtonMask::default());
        events.extend(
            achievements
                .evaluate_frame(|address, buffer| core.read_memory(address, buffer))
                .unwrap(),
        );
    }
    assert_eq!(
        events,
        [42],
        "one unlock, with no repeated events on later frames"
    );
}
