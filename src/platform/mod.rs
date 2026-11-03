//! Everything that differs between operating systems lives in this folder, so
//! the rest of the program never asks which one it is running on.
//!
//! Each system has one file, and every file offers the same things:
//!
//! - `config_dir()`: where small settings are kept
//! - `clipboard_copy()`, `clipboard_paste()`, `clipboard_missing_hint()`
//! - `CAN_INSTALL`, `install()`, `uninstall()`, `is_current()`: putting the
//!   program in the launcher from inside the app, where that makes sense
//! - `hook_about_menu()`: on a Mac, makes the app menu's "About Copperview"
//!   open the program's own About window
//!
//! To support a new system, add a file with those names and one line below.
//! Window and trackpad differences are in `pinch.rs`, and the dependencies each
//! system needs are listed under its own heading in `Cargo.toml`.
//!
//! The other systems' files are still compiled by the tests, so a change made
//! on one machine can't quietly break another.

#[cfg(any(all(unix, not(target_os = "macos")), all(test, unix)))]
#[allow(dead_code)]
mod linux;
#[cfg(any(target_os = "macos", test))]
#[allow(dead_code)]
mod macos;
#[cfg(any(windows, test))]
#[allow(dead_code)]
mod windows;

#[cfg(all(unix, not(target_os = "macos")))]
use linux as native;
#[cfg(target_os = "macos")]
use macos as native;
#[cfg(windows)]
use windows as native;

pub use native::{
    CAN_INSTALL, clipboard_copy, clipboard_missing_hint, clipboard_paste, config_dir, hook_about_menu, install, is_current, uninstall,
};

static ABOUT_ASKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Something outside the window, such as a menu item, asked for the About window.
#[allow(dead_code)]
pub fn request_about() {
    ABOUT_ASKED.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// True once for each request for the About window.
pub fn about_requested() -> bool {
    ABOUT_ASKED.swap(false, std::sync::atomic::Ordering::Relaxed)
}
