# Ubuntu portability

Recorded before the Linux platform implementation, 3 October 2026.

The current app still uses its direct Codex adapter. This check does not establish
a working T3 frontend or a working Claude flow.

## Failure paths

- Linux cannot select a window-system backend because eframe defaults are disabled.
- A normal launch cannot resolve an internal state folder, or treats a relative XDG path as absolute.
- A process exits while `/proc` is being read, or its command name contains spaces or parentheses.
- PID reuse or a reboot makes a stale engine record identify a different process.
- Group cleanup overlooks tool children after the engine leader exits.
- Bootstrap invokes Xcode tools or writes build output to a Mac path on Linux.
- A Wayland window fails to render, or native launch and the offscreen UI harness disagree.
- A Codex run fails to stream, handle approval, stop, preserve a draft, resume, or clean up its children.

## Repeatable checks

Use the source and artifact paths in `docs/ubuntu.md`. Run the existing
`ui_checks` and opt-in `codex_live` scenarios, with evidence outside Git. Also
launch the actual window with `BUKNO_AUTOPILOT=screenshot`; it saves its frame
and exits. Keep Linux results separate from Mac results.

Real desktop IME, screen-reader speech, reduced-motion preference integration,
Windows execution, Claude, and T3 client compatibility remain separate gates.
