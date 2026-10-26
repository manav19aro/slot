//! Portable preferences: copying Achievements/ carries the mode with progress.
use serde::{Deserialize, Serialize};
use std::{io, path::Path};
#[derive(Copy, Clone, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Local,
    SyncEnabled,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    version: u32,
    mode: Mode,
    #[serde(default = "default_enabled")]
    enabled: bool,
}
fn default_enabled() -> bool {
    true
}
fn settings(root: &Path) -> io::Result<Settings> {
    use std::io::Read;
    let file = match std::fs::File::open(root.join("Achievements/settings.json")) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Ok(Settings {
                version: 1,
                mode: Mode::Local,
                enabled: true,
            })
        }
        Err(e) => return Err(e),
    };
    let mut bytes = Vec::new();
    file.take(4097).read_to_end(&mut bytes)?;
    if bytes.len() > 4096 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "achievement settings too large",
        ));
    }
    let settings: Settings = serde_json::from_slice(&bytes)?;
    if settings.version != 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unsupported achievement settings",
        ));
    }
    Ok(settings)
}
pub fn read(root: &Path) -> io::Result<Mode> {
    Ok(settings(root)?.mode)
}
pub fn enabled(root: &Path) -> io::Result<bool> {
    Ok(settings(root)?.enabled)
}
fn save(root: &Path, settings: &Settings) -> io::Result<()> {
    std::fs::create_dir_all(root.join("Achievements"))?;
    let bytes = serde_json::to_vec_pretty(settings)?;
    slot_store::atomic_write(&root.join("Achievements/settings.json"), &bytes)
}
pub fn write(root: &Path, mode: Mode) -> io::Result<()> {
    let mut current = settings(root)?;
    current.mode = mode;
    save(root, &current)
}
pub fn set_enabled(root: &Path, enabled: bool) -> io::Result<()> {
    let mut current = settings(root)?;
    current.enabled = enabled;
    save(root, &current)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn enabled_defaults_for_legacy_settings_and_preserves_mode() {
        let root = tempfile::tempdir().unwrap();
        assert!(enabled(root.path()).unwrap());
        std::fs::create_dir_all(root.path().join("Achievements")).unwrap();
        std::fs::write(
            root.path().join("Achievements/settings.json"),
            br#"{"version":1,"mode":"sync_enabled"}"#,
        )
        .unwrap();
        assert!(enabled(root.path()).unwrap());
        set_enabled(root.path(), false).unwrap();
        assert!(!enabled(root.path()).unwrap());
        assert_eq!(read(root.path()).unwrap(), Mode::SyncEnabled);
        write(root.path(), Mode::Local).unwrap();
        assert!(!enabled(root.path()).unwrap());
        set_enabled(root.path(), true).unwrap();
        assert_eq!(read(root.path()).unwrap(), Mode::Local);
    }
}
