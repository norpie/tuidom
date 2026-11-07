//! Poison-tolerant lock helpers, mirroring the engine's `tuidom/src/lock.rs`.
//!
//! A panic while a lock is held poisons it. The runtime would rather recover the inner
//! value and carry on — a poisoned reactive arena is no less usable than a live one — than
//! propagate the poison as a second failure, so every acquisition here swallows it.

use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

/// Acquire a read lock, recovering the inner value if the lock was poisoned.
pub(crate) fn rw_read<T>(lock: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Acquire a write lock, recovering the inner value if the lock was poisoned.
pub(crate) fn rw_write<T>(lock: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    lock.write()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
