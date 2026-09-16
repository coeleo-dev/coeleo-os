//! IRQ-safe spinlock.
//!
//! A plain `spin::Mutex` deadlocks when an IRQ handler on the *same* CPU tries
//! to take a lock already held by interrupted kernel code (e.g. a syscall that
//! holds `SCHED`, interrupted by the keyboard IRQ). This lock saves the current
//! interrupt flag, disables interrupts, spins, and restores the flag on release.

use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, Ordering};

use x86_64::registers::rflags::{self, RFlags};

pub struct IrqLock<T> {
    locked: AtomicBool,
    data: UnsafeCell<T>,
}

pub struct IrqGuard<'a, T> {
    lock: &'a IrqLock<T>,
    /// Whether interrupts were enabled before we disabled them.
    was_enabled: bool,
}

// SAFETY: `IrqLock` provides exclusive access to `data`, exactly like a mutex.
unsafe impl<T: Send> Sync for IrqLock<T> {}
unsafe impl<T: Send> Send for IrqLock<T> {}

impl<T> IrqLock<T> {
    pub const fn new(data: T) -> Self {
        IrqLock {
            locked: AtomicBool::new(false),
            data: UnsafeCell::new(data),
        }
    }

    pub fn lock(&self) -> IrqGuard<'_, T> {
        // Read IF before disabling so the guard can restore the prior state.
        let was_enabled = rflags::read().contains(RFlags::INTERRUPT_FLAG);
        x86_64::instructions::interrupts::disable();
        while self
            .locked
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
        IrqGuard {
            lock: self,
            was_enabled,
        }
    }
}

impl<T> Deref for IrqGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        // SAFETY: the guard holds the lock, giving exclusive access.
        unsafe { &*self.lock.data.get() }
    }
}

impl<T> DerefMut for IrqGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: the guard holds the lock, giving exclusive access.
        unsafe { &mut *self.lock.data.get() }
    }
}

impl<T> Drop for IrqGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.locked.store(false, Ordering::Release);
        if self.was_enabled {
            x86_64::instructions::interrupts::enable();
        }
    }
}
