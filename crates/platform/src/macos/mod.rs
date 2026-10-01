use std::ffi::c_void;
use std::ptr::NonNull;

use objc2_app_kit::{NSView, NSWindowButton, NSWorkspace};
use objc2_foundation::NSPoint;

pub fn reduce_motion() -> bool {
    NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
}

/// Move the traffic lights so their centres sit `center_from_top` points
/// below the window's top edge, starting `left` points from its left edge.
///
/// AppKit lays the buttons out again on resize, so call this whenever the
/// window size changes.
///
/// # Safety
/// `ns_view` must be the window's live `NSView`, and this must run on the
/// main thread.
pub unsafe fn place_window_buttons(ns_view: NonNull<c_void>, left: f64, center_from_top: f64, spacing: f64) {
    // SAFETY: the caller guarantees a live NSView on the main thread.
    let view: &NSView = unsafe { ns_view.cast().as_ref() };
    let Some(window) = view.window() else { return };
    let kinds = [NSWindowButton::CloseButton, NSWindowButton::MiniaturizeButton, NSWindowButton::ZoomButton];
    let Some(close) = window.standardWindowButton(NSWindowButton::CloseButton) else { return };
    let button_height = close.frame().size.height;
    // The buttons live in the titlebar container. Make it as tall as Bukno's
    // titlebar so the buttons can be centred in it.
    // SAFETY: superview only reads the view hierarchy, on the main thread.
    let Some(container) = (unsafe { close.superview().and_then(|v| v.superview()) }) else { return };
    let bar_height = center_from_top * 2.0;
    let mut bar = container.frame();
    bar.size.height = bar_height;
    bar.origin.y = window.frame().size.height - bar_height;
    container.setFrame(bar);
    for (i, kind) in kinds.into_iter().enumerate() {
        if let Some(button) = window.standardWindowButton(kind) {
            let origin = NSPoint::new(left + i as f64 * spacing, (bar_height - button_height) / 2.0);
            button.setFrameOrigin(origin);
        }
    }
}
