//! Offline GBA rule evaluation and local unlock history. Explicit preparation lives
//! in `download`; the evaluator and local cache never perform network requests.
//! Call once after each committed emulated frame, never for speculative or refresh
//! frames. Each instance belongs to one game/session; create a new one on game changes.
use std::collections::BTreeSet;
use std::ffi::{c_char, c_int, c_void, CString};
use std::fmt;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr::NonNull;

/// Upstream rc_client_game.h reserves IDs starting here for service warnings.
/// These are status messages, never earned achievements (including in softcore).
pub const SERVICE_WARNING_ID: u32 = 101_000_001;

pub fn is_service_warning(id: u32) -> bool {
    id >= SERVICE_WARNING_ID
}

#[repr(C)]
struct Runtime {
    _opaque: [u8; 0],
}

type ReadMemory = unsafe extern "C" fn(u32, *mut u8, u32, *mut c_void) -> u32;

extern "C" {
    fn rc_runtime_alloc() -> *mut Runtime;
    fn rc_runtime_destroy(runtime: *mut Runtime);
    fn rc_runtime_activate_achievement(
        runtime: *mut Runtime,
        id: u32,
        rule: *const c_char,
        unused: *mut c_void,
        index: c_int,
    ) -> c_int;
    fn rc_runtime_deactivate_achievement(runtime: *mut Runtime, id: u32);
    fn rc_runtime_reset(runtime: *mut Runtime);
    fn rc_runtime_format_achievement_measured(
        runtime: *const Runtime,
        id: u32,
        buffer: *mut c_char,
        size: usize,
    ) -> c_int;
    fn slot_rc_do_frame(runtime: *mut Runtime, read: ReadMemory, userdata: *mut c_void);
    fn slot_rc_is_triggered(runtime: *mut Runtime, id: u32) -> c_int;
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    Allocation,
    RuleContainsNul,
    InvalidRule(i32),
    MemoryUnavailable,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Allocation => write!(f, "could not allocate achievement runtime"),
            Self::RuleContainsNul => write!(f, "achievement rule contains a nul byte"),
            Self::InvalidRule(code) => {
                write!(f, "invalid achievement rule (rcheevos error {code})")
            }
            Self::MemoryUnavailable => write!(
                f,
                "achievement memory read failed; evaluation progress reset"
            ),
        }
    }
}
impl std::error::Error for Error {}

/// Converts RetroAchievements GBA addresses to native addresses and bytes left in
/// the region, using the pinned upstream rc_consoleinfo.c GBA memory layout.
fn gba_address(address: u32) -> Option<(u32, usize)> {
    let (start, end, native) = match address {
        0..=0x007fff => (0, 0x008000, 0x03000000),
        0x008000..=0x047fff => (0x008000, 0x048000, 0x02000000),
        0x048000..=0x057fff => (0x048000, 0x058000, 0x0e000000),
        _ => return None,
    };
    Some((native + address - start, (end - address) as usize))
}

struct Reader<F> {
    read: F,
    failed: bool,
}

unsafe extern "C" fn read_memory<F: FnMut(u32, &mut [u8]) -> usize>(
    address: u32,
    buffer: *mut u8,
    len: u32,
    userdata: *mut c_void,
) -> u32 {
    // SAFETY: evaluate_frame supplies this context synchronously, and rcheevos
    // supplies a writable buffer of len bytes. Neither pointer is retained.
    let reader = &mut *userdata.cast::<Reader<F>>();
    let output = std::slice::from_raw_parts_mut(buffer, len as usize);
    output.fill(0);
    // A user-supplied reader must never unwind through C.
    let result = catch_unwind(AssertUnwindSafe(|| {
        let mut count = 0;
        while count < output.len() {
            let logical = address.checked_add(u32::try_from(count).ok()?)?;
            let (native, remaining) = gba_address(logical)?;
            let size = remaining.min(output.len() - count);
            if (reader.read)(native, &mut output[count..count + size]) != size {
                return None;
            }
            count += size;
        }
        Some(())
    }));
    if !matches!(result, Ok(Some(()))) {
        reader.failed = true;
        output.fill(0);
        return 0;
    }
    len
}

/// Owns a rcheevos runtime and emits each achievement ID once per instance.
/// Keep this on the emulation thread. Unlock persistence is the caller's concern.
pub struct GbaAchievements {
    runtime: NonNull<Runtime>,
    active: BTreeSet<u32>,
    unlocked: BTreeSet<u32>,
}

impl GbaAchievements {
    pub fn new() -> Result<Self, Error> {
        // SAFETY: allocation returns an owned runtime or null.
        let runtime = NonNull::new(unsafe { rc_runtime_alloc() }).ok_or(Error::Allocation)?;
        Ok(Self {
            runtime,
            active: BTreeSet::new(),
            unlocked: BTreeSet::new(),
        })
    }

    /// Registers a local rule. A previously unlocked ID stays unlocked. On parse
    /// failure an existing rule for this ID is removed, preventing stale evaluation.
    pub fn activate(&mut self, id: u32, rule: &str) -> Result<(), Error> {
        if is_service_warning(id) {
            return Ok(());
        }
        let rule = CString::new(rule).map_err(|_| Error::RuleContainsNul)?;
        if self.unlocked.contains(&id) {
            return Ok(());
        }
        self.active.remove(&id);
        // SAFETY: runtime is owned, string lives for the call; parser copies it.
        let result = unsafe {
            rc_runtime_deactivate_achievement(self.runtime.as_ptr(), id);
            rc_runtime_activate_achievement(
                self.runtime.as_ptr(),
                id,
                rule.as_ptr(),
                std::ptr::null_mut(),
                0,
            )
        };
        if result < 0 {
            return Err(Error::InvalidRule(result));
        }
        self.active.insert(id);
        Ok(())
    }

    /// Evaluates the current committed frame. Pass a native memory reader such as
    /// `|address, buffer| core.read_memory(address, buffer)`.
    /// Any failed read discards this frame's awards and resets transient progress.
    pub fn evaluate_frame<F: FnMut(u32, &mut [u8]) -> usize>(
        &mut self,
        read: F,
    ) -> Result<Vec<u32>, Error> {
        let mut reader = Reader {
            read,
            failed: false,
        };
        // SAFETY: runtime and stack context live through this synchronous call.
        unsafe {
            slot_rc_do_frame(
                self.runtime.as_ptr(),
                read_memory::<F>,
                (&mut reader as *mut Reader<F>).cast(),
            );
        }
        if reader.failed {
            self.reset_progress();
            return Err(Error::MemoryUnavailable);
        }
        let newly_unlocked: Vec<_> = self
            .active
            .iter()
            .copied()
            .filter(|id| {
                // SAFETY: runtime belongs to self; this only inspects trigger state.
                unsafe { slot_rc_is_triggered(self.runtime.as_ptr(), *id) != 0 }
            })
            .collect();
        for id in &newly_unlocked {
            self.unlocked.insert(*id);
            self.active.remove(id);
            unsafe { rc_runtime_deactivate_achievement(self.runtime.as_ptr(), *id) };
        }
        Ok(newly_unlocked)
    }

    /// Read measured values without advancing evaluation or touching core memory.
    /// Upstream handles percentage rules, unknown values, and inactive triggers.
    pub fn measured_progress(&self) -> Vec<(u32, String)> {
        self.active
            .iter()
            .filter_map(|id| {
                let mut buffer = [0u8; 64];
                // SAFETY: runtime is owned and the formatter writes at most buffer.len().
                let written = unsafe {
                    rc_runtime_format_achievement_measured(
                        self.runtime.as_ptr(),
                        *id,
                        buffer.as_mut_ptr().cast(),
                        buffer.len(),
                    )
                };
                if written <= 0 || written as usize >= buffer.len() {
                    return None;
                }
                let value = std::str::from_utf8(&buffer[..written as usize]).ok()?;
                Some((*id, value.into()))
            })
            .collect()
    }

    /// Restores a previously earned ID without producing another unlock event.
    pub fn restore_unlock(&mut self, id: u32) {
        if is_service_warning(id) {
            return;
        }
        self.unlocked.insert(id);
        self.active.remove(&id);
        unsafe { rc_runtime_deactivate_achievement(self.runtime.as_ptr(), id) };
    }

    /// Use when loading a state/rewinding until runtime progress is serialized
    /// alongside emulator states. Earned IDs remain earned within this instance.
    pub fn reset_progress(&mut self) {
        unsafe { rc_runtime_reset(self.runtime.as_ptr()) };
    }
}

impl Drop for GbaAchievements {
    fn drop(&mut self) {
        // SAFETY: this is the sole owner, destroyed exactly once.
        unsafe { rc_runtime_destroy(self.runtime.as_ptr()) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gba_translation_covers_region_edges_and_rejects_other_addresses() {
        for (logical, native, remaining) in [
            (0, 0x03000000, 0x8000),
            (0x7fff, 0x03007fff, 1),
            (0x8000, 0x02000000, 0x40000),
            (0x47fff, 0x0203ffff, 1),
            (0x48000, 0x0e000000, 0x10000),
            (0x57fff, 0x0e00ffff, 1),
        ] {
            assert_eq!(gba_address(logical), Some((native, remaining)));
        }
        assert_eq!(gba_address(0x58000), None);
        assert_eq!(gba_address(u32::MAX), None);
    }
}

pub mod pack;

pub mod progress;

/// Explicit user-requested pack preparation; never called during evaluation.
pub mod download;

pub mod mode;
