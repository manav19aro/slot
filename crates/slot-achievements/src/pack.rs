//! Versioned, account-free achievement packs selected by ROM content hash.
use std::ffi::{c_char, c_int};
use std::io::{self, Read};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::GbaAchievements;

const MAX_PACK_BYTES: u64 = 8 * 1024 * 1024;
const MAX_ROM_BYTES: u64 = 32 * 1024 * 1024;

extern "C" {
    fn slot_rc_gba_hash(hash: *mut c_char, data: *const u8, size: usize) -> c_int;
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Achievement {
    pub id: u32,
    pub title: String,
    pub description: String,
    pub points: u32,
    pub rule: String,
    #[serde(default)]
    pub badge: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GbaPack {
    pub version: u32,
    pub console_id: u32,
    pub game_id: u32,
    pub title: String,
    pub rom_hash: String,
    pub achievements: Vec<Achievement>,
}

fn invalid(message: impl ToString) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}

/// Uses the pinned rcheevos GBA hashing implementation. No ROM data leaves disk.
pub fn hash_gba_rom(path: &Path) -> io::Result<String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(MAX_ROM_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.is_empty() || bytes.len() as u64 > MAX_ROM_BYTES {
        return Err(invalid("GBA ROM must contain 1 byte to 32 MiB"));
    }
    let mut hash = [0u8; 33];
    // SAFETY: C receives a writable 33-byte hash buffer and a valid ROM buffer
    // for a synchronous call; neither pointer is retained.
    let result = unsafe { slot_rc_gba_hash(hash.as_mut_ptr().cast(), bytes.as_ptr(), bytes.len()) };
    if result == 0 {
        return Err(invalid("rcheevos could not hash GBA ROM"));
    }
    let hash = std::str::from_utf8(&hash[..32])
        .map_err(invalid)?
        .to_owned();
    if !valid_hash(&hash) {
        return Err(invalid("rcheevos returned an invalid ROM hash"));
    }
    Ok(hash)
}

fn valid_hash(hash: &str) -> bool {
    hash.len() == 32
        && hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl GbaPack {
    pub fn validate(&self, expected_hash: &str) -> io::Result<()> {
        if self.version != 1 || self.console_id != 5 || self.game_id == 0 {
            return Err(invalid("unsupported pack version, console, or game ID"));
        }
        if !valid_hash(expected_hash) || self.rom_hash != expected_hash {
            return Err(invalid(
                "achievement pack does not match the exact ROM hash",
            ));
        }
        if self.title.trim().is_empty()
            || self.achievements.is_empty()
            || self.achievements.len() > 10000
        {
            return Err(invalid("pack requires a title and 1 to 10000 achievements"));
        }
        let mut runtime = GbaAchievements::new().map_err(invalid)?;
        let mut ids = std::collections::BTreeSet::new();
        for achievement in &self.achievements {
            if achievement.id == 0
                || !ids.insert(achievement.id)
                || achievement.title.trim().is_empty()
            {
                return Err(invalid(
                    "invalid or duplicate achievement ID, or missing title",
                ));
            }
            runtime
                .activate(achievement.id, &achievement.rule)
                .map_err(invalid)?;
        }
        Ok(())
    }

    /// Validates an explicit pack file; never derives a path from its contents.
    pub fn read(path: &Path, expected_hash: &str) -> io::Result<Self> {
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take(MAX_PACK_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_PACK_BYTES {
            return Err(invalid("achievement pack exceeds 8 MiB"));
        }
        let mut pack: Self = serde_json::from_slice(&bytes).map_err(invalid)?;
        // Older caches can contain synthetic warning entries. Keep these out of
        // achievement counts and evaluation while retaining their status in logs.
        pack.achievements.retain(|achievement| {
            if crate::is_service_warning(achievement.id) {
                eprintln!(
                    "slot: achievements: service notice {:?}: {:?}",
                    achievement.title, achievement.description
                );
                false
            } else {
                true
            }
        });
        pack.validate(expected_hash)?;
        Ok(pack)
    }
}

/// Missing cache entries are normal; corrupt/mismatched packs are errors. A
/// missing directory avoids hashing every game when the feature is not in use.
pub fn load_for_rom(directory: &Path, rom: &Path) -> io::Result<Option<GbaPack>> {
    if !directory.try_exists()? {
        return Ok(None);
    }
    let hash = hash_gba_rom(rom)?;
    match GbaPack::read(&directory.join(format!("{hash}.json")), &hash) {
        Ok(pack) => Ok(Some(pack)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}
