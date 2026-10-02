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

/// Frames of the three window buttons as (x, y from the window top, width,
/// height) in points, for evidence that the placement took effect.
///
/// # Safety
/// Same as [`place_window_buttons`].
pub unsafe fn window_button_frames(ns_view: NonNull<c_void>) -> Vec<[f64; 4]> {
    // SAFETY: the caller guarantees a live NSView on the main thread.
    let view: &NSView = unsafe { ns_view.cast().as_ref() };
    let Some(window) = view.window() else { return Vec::new() };
    let window_height = window.frame().size.height;
    [NSWindowButton::CloseButton, NSWindowButton::MiniaturizeButton, NSWindowButton::ZoomButton]
        .into_iter()
        .filter_map(|kind| {
            let button = window.standardWindowButton(kind)?;
            // Convert to window coordinates (origin bottom-left), then flip.
            let frame = button.convertRect_toView(button.bounds(), None);
            Some([
                frame.origin.x,
                window_height - frame.origin.y - frame.size.height,
                frame.size.width,
                frame.size.height,
            ])
        })
        .collect()
}

fn bsd_info(pid: u32) -> Option<libc::proc_bsdinfo> {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    // SAFETY: the buffer is a correctly sized proc_bsdinfo.
    let n = unsafe { libc::proc_pidinfo(pid as libc::c_int, libc::PROC_PIDTBSDINFO, 0, (&raw mut info).cast(), size) };
    (n == size).then_some(info)
}

pub fn process_start(pid: u32) -> Option<u64> {
    bsd_info(pid).map(|info| info.pbi_start_tvsec)
}

/// PIDs whose process group is `leader`.
pub fn group_members(leader: u32) -> Vec<u32> {
    // proc_listpids type for "processes in this process group" (sys/proc_info.h).
    const PROC_PGRP_ONLY: u32 = 2;
    let mut pids = vec![0 as libc::pid_t; 512];
    let bytes = (pids.len() * std::mem::size_of::<libc::pid_t>()) as libc::c_int;
    // SAFETY: the buffer holds `bytes` bytes of pid_t.
    let n = unsafe { libc::proc_listpids(PROC_PGRP_ONLY, leader, pids.as_mut_ptr().cast(), bytes) };
    if n <= 0 {
        return Vec::new();
    }
    let count = n as usize / std::mem::size_of::<libc::pid_t>();
    pids.truncate(count);
    pids.into_iter().filter(|p| *p > 0).map(|p| p as u32).collect()
}
