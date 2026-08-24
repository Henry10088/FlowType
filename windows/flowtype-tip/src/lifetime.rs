use std::sync::atomic::{AtomicU32, Ordering};

static OBJECTS: AtomicU32 = AtomicU32::new(0);
static LOCKS: AtomicU32 = AtomicU32::new(0);

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
    OBJECTS.load(Ordering::Acquire) == 0 && LOCKS.load(Ordering::Acquire) == 0
}
