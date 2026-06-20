//! Poison-tolerant lock helpers.

use std::sync::{Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// Lock a mutex, recovering the inner value if the mutex was poisoned.
pub(crate) fn mutex<T>(lock: &Mutex<T>) -> MutexGuard<'_, T> {
    lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Acquire a read lock, recovering the inner value if the lock was poisoned.
pub(crate) fn rw_read<T>(lock: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Acquire a write lock, recovering the inner value if the lock was poisoned.
pub(crate) fn rw_write<T>(lock: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    lock.write().unwrap_or_else(|poisoned| poisoned.into_inner())
}
