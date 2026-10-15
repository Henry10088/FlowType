use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

static OBJECTS: AtomicU32 = AtomicU32::new(0);
static LOCKS: AtomicU32 = AtomicU32::new(0);
static KEEP_LOADED: AtomicBool = AtomicBool::new(false);

// A worker that could not stop within the UI deadline may still execute DLL
// code. Keep this COM server loaded until the host exits, even if it later
// finishes; releasing the last reference from Rust thread cleanup is unsafe.
pub fn keep_loaded_until_process_exit() {
    KEEP_LOADED.store(true, Ordering::Release);
}

#[link(name = "ntdll")]
unsafe extern "system" {
    fn RtlDllShutdownInProgress() -> u8;
}

pub fn process_is_terminating() -> bool {
    unsafe { RtlDllShutdownInProgress() != 0 }
}

pub struct ObjectGuard;

impl ObjectGuard {
    pub fn new() -> Self {
        OBJECTS.fetch_add(1, Ordering::Relaxed);
        Self
    }
}

impl Drop for ObjectGuard {
    fn drop(&mut self) {
        OBJECTS.fetch_sub(1, Ordering::Release);
    }
}

pub fn set_server_lock(locked: bool) {
    if locked {
        LOCKS.fetch_add(1, Ordering::Relaxed);
    } else {
        let _ = LOCKS.fetch_update(Ordering::Release, Ordering::Relaxed, |value| {
            Some(value.saturating_sub(1))
        });
    }
}

pub fn can_unload() -> bool {
    !KEEP_LOADED.load(Ordering::Acquire)
        && OBJECTS.load(Ordering::Acquire) == 0
        && LOCKS.load(Ordering::Acquire) == 0
}
