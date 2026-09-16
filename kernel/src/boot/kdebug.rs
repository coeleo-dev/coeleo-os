//! Kernel Debugging Utility for AI Agents and Developers.
//!
//! Provides zero-overhead (when disabled), structured diagnostic logging to COM1 serial.
//! In production / test runs, logging is disabled by default to avoid failing headless tests
//! that expect exact serial substrings.
//!
//! # Enabling Debug Logging
//! To enable debug output:
//! - Programmatically: `crate::kdebug::enable()`
//! - Or flip the default constant: `const DEFAULT_ENABLED: bool = true;`
//!
//! # Macro Usage
//! ```rust
//! crate::kdebug!("PID {} spawned with {} bytes", pid, size);
//! crate::kdebug_tag!("SPAWN", "path '{}' loaded", path);
//! ```

#![allow(dead_code)]

use core::sync::atomic::{AtomicBool, Ordering};

const DEFAULT_ENABLED: bool = false;
static ENABLED: AtomicBool = AtomicBool::new(DEFAULT_ENABLED);

/// Enables debug serial logging.
pub fn enable() {
    ENABLED.store(true, Ordering::Release);
}

/// Disables debug serial logging.
pub fn disable() {
    ENABLED.store(false, Ordering::Release);
}

/// Returns true if debug serial logging is currently active.
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Acquire)
}

#[macro_export]
macro_rules! kdebug {
    ($($arg:tt)*) => {
        if $crate::kdebug::is_enabled() {
            $crate::serial_println!("[DEBUG] {}", format_args!($($arg)*));
        }
    };
}

#[macro_export]
macro_rules! kdebug_tag {
    ($tag:expr, $($arg:tt)*) => {
        if $crate::kdebug::is_enabled() {
            $crate::serial_println!("[{}] {}", $tag, format_args!($($arg)*));
        }
    };
}
