use slot_achievements::pack::{hash_gba_rom, GbaPack};
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if !(2..=3).contains(&args.len()) {
        return Err("usage: inspect_pack ROM.gba [PACK.json]".into());
    }
    let hash = hash_gba_rom(Path::new(&args[1]))?;
    println!("GBA RetroAchievements hash: {hash}");
    if let Some(path) = args.get(2) {
        let pack = GbaPack::read(Path::new(path), &hash)?;
        println!(
            "Validated {} rules for {} (game {})",
            pack.achievements.len(),
            pack.title,
            pack.game_id
        );
    }
    Ok(())
}
