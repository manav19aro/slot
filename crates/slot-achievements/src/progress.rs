//! Local, softcore unlock history. It is not bound to any online account yet.
use std::collections::BTreeMap;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum SyncStatus {
    Local,
    // Legacy state retained so older portable histories remain readable.
    PendingSync,
    ServerConfirmed { username: String, confirmed_at: u64 },
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Unlock {
    pub earned_at: u64,
    pub sync: SyncStatus,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct History {
    pub version: u32,
    pub console_id: u32,
    pub game_id: u32,
    pub rom_hash: String,
    pub mode: String,
    pub profile: String,
    pub unlocks: BTreeMap<u32, Unlock>,
}

pub struct ProgressStore {
    path: PathBuf,
    history: History,
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

impl ProgressStore {
    pub fn open(directory: &Path, game_id: u32, rom_hash: &str) -> io::Result<Self> {
        if game_id == 0
            || rom_hash.len() != 32
            || !rom_hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(invalid("invalid progress game identity"));
        }
        let path = directory.join(format!("{rom_hash}.json"));
        let history = match std::fs::File::open(&path) {
            Ok(file) => {
                let mut bytes = Vec::new();
                file.take(8 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
                if bytes.len() > 8 * 1024 * 1024 {
                    return Err(invalid("progress history exceeds 8 MiB"));
                }
                let history: History = serde_json::from_slice(&bytes)
                    .map_err(|_| invalid("could not parse achievement history; file preserved"))?;
                if history.version != 1
                    || history.console_id != 5
                    || history.game_id != game_id
                    || history.rom_hash != rom_hash
                    || history.mode != "softcore"
                    || history.profile != "local"
                {
                    return Err(invalid(
                        "achievement history identity/version does not match; file preserved",
                    ));
                }
                if history.unlocks.iter().any(|(id, record)| *id == 0 || crate::is_service_warning(*id)
                    || matches!(&record.sync, SyncStatus::ServerConfirmed {username, ..} if username.trim().is_empty())) {
                    return Err(invalid("invalid achievement history record; file preserved"));
                }
                history
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => History {
                version: 1,
                console_id: 5,
                game_id,
                rom_hash: rom_hash.into(),
                mode: "softcore".into(),
                profile: "local".into(),
                unlocks: BTreeMap::new(),
            },
            Err(error) => return Err(error),
        };
        Ok(Self { path, history })
    }

    pub fn history(&self) -> &History {
        &self.history
    }

    /// Import account unlocks into the portable local union without deleting local IDs.
    /// The legacy IDs-only service response has no earned timestamps; zero means unknown.
    pub fn merge_confirmed(
        &mut self,
        ids: &[u32],
        username: &str,
        confirmed_at: u64,
    ) -> io::Result<usize> {
        if username.trim().is_empty()
            || ids
                .iter()
                .any(|id| *id == 0 || crate::is_service_warning(*id))
        {
            return Err(invalid("invalid imported achievement record"));
        }
        let mut next = self.history.clone();
        let mut added = 0;
        for id in ids {
            let record = next.unlocks.entry(*id).or_insert_with(|| {
                added += 1;
                Unlock {
                    earned_at: 0,
                    sync: SyncStatus::Local,
                }
            });
            record.sync = SyncStatus::ServerConfirmed {
                username: username.into(),
                confirmed_at,
            };
        }
        if next.unlocks != self.history.unlocks {
            let bytes = serde_json::to_vec_pretty(&next)
                .map_err(|_| invalid("could not encode achievement history"))?;
            std::fs::create_dir_all(self.path.parent().expect("progress has parent"))?;
            slot_store::atomic_write(&self.path, &bytes)?;
            self.history = next;
        }
        Ok(added)
    }

    /// Writes and syncs a complete replacement before publishing it in memory.
    /// Existing timestamps and sync states survive repeated awards unchanged.
    pub fn record(&mut self, ids: &[u32], earned_at: u64) -> io::Result<()> {
        let mut next = self.history.clone();
        for id in ids {
            if *id == 0 || crate::is_service_warning(*id) {
                return Err(invalid("invalid earned achievement ID"));
            }
            next.unlocks.entry(*id).or_insert(Unlock {
                earned_at,
                sync: SyncStatus::Local,
            });
        }
        if next.unlocks == self.history.unlocks {
            return Ok(());
        }
        let bytes = serde_json::to_vec_pretty(&next)
            .map_err(|_| invalid("could not encode achievement history"))?;
        std::fs::create_dir_all(self.path.parent().expect("progress has parent"))?;
        slot_store::atomic_write(&self.path, &bytes)?;
        self.history = next;
        Ok(())
    }
}
