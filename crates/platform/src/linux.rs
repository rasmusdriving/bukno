//! Linux engine identity and process-group discovery. See the failure paths in
//! e2e/scenarios/ubuntu-portability.md before changing these operations.

use std::sync::OnceLock;

/// Fields after the parenthesized command name in /proc/<pid>/stat.
/// The name can itself contain spaces and parentheses.
fn stat_fields(text: &str) -> Option<Vec<&str>> {
    Some(text.rsplit_once(')')?.1.split_whitespace().collect())
}

/// Boot time plus kernel clock frequency, fixed for this process's lifetime.
fn boot_clock() -> Option<(u64, u64)> {
    static CLOCK: OnceLock<Option<(u64, u64)>> = OnceLock::new();
    *CLOCK.get_or_init(|| {
        let stat = std::fs::read_to_string("/proc/stat").ok()?;
        let boot: u64 = stat.lines().find_map(|line| line.strip_prefix("btime "))?.parse().ok()?;
        // SAFETY: sysconf reads the kernel's clock frequency, with no pointers.
        let ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
        (ticks > 0).then_some((boot, ticks as u64))
    })
}

/// Epoch nanoseconds derived from the boot time and exact kernel start tick.
/// Retaining tick precision distinguishes PIDs reused within the same second;
/// including boot time prevents old records from matching after a reboot.
pub fn process_start(pid: u32) -> Option<u64> {
    let text = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let fields = stat_fields(&text)?;
    // Field 22; the first field after the command name is field 3 (state).
    let start_ticks: u64 = fields.get(19)?.parse().ok()?;
    let (boot, hz) = boot_clock()?;
    boot.checked_mul(1_000_000_000)?.checked_add(start_ticks.checked_mul(1_000_000_000)? / hz)
}

/// Find tools still in an owned group, even if its leader has already exited.
pub fn group_members(leader: u32) -> Vec<u32> {
    let Ok(entries) = std::fs::read_dir("/proc") else { return Vec::new() };
    let mut members = Vec::new();
    for entry in entries.flatten() {
        let Some(pid) = entry.file_name().to_str().and_then(|name| name.parse::<u32>().ok()) else {
            continue;
        };
        let Ok(text) = std::fs::read_to_string(entry.path().join("stat")) else { continue };
        if stat_fields(&text)
            // Field 5 is pgrp. Exited zombies no longer own running work.
            .is_some_and(|fields| {
                fields.first() != Some(&"Z") && fields.get(2).and_then(|p| p.parse::<u32>().ok()) == Some(leader)
            })
        {
            members.push(pid);
        }
    }
    members.sort_unstable();
    members
}
