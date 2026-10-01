//! Operating-system services: application paths and system preferences.
//!
//! Shared code calls these functions; each platform module implements them.
//! An operation a platform does not implement yet returns an explicit
//! unavailable result instead of pretending to succeed.

pub mod paths;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

/// Move the macOS window buttons into Bukno's taller titlebar.
///
/// # Safety
/// `ns_view` must be the window's live `NSView`, on the main thread.
#[cfg(target_os = "macos")]
pub unsafe fn place_window_buttons(
    ns_view: std::ptr::NonNull<std::ffi::c_void>,
    left: f64,
    center_from_top: f64,
    spacing: f64,
) {
    // SAFETY: forwarded from the caller.
    unsafe { macos::place_window_buttons(ns_view, left, center_from_top, spacing) }
}

/// Whether the user asked the system to reduce motion.
///
/// `None` means Bukno cannot read the setting on this platform yet.
pub fn reduce_motion() -> Option<bool> {
    #[cfg(target_os = "macos")]
    {
        Some(macos::reduce_motion())
    }
    #[cfg(target_os = "windows")]
    {
        windows::reduce_motion()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        None
    }
}
