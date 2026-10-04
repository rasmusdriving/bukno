# Ubuntu

The native app now has Linux window-system support, XDG application-data paths,
and Linux process identity/group cleanup. It still uses the existing direct
Codex adapter. The T3 client from the [T3 backend plan](t3-backend-audit.md)
runs alongside it; T3 chats appear under T3 in the sidebar once a server is
paired, and can be worked in from Bukno.
On Linux the sign-in is kept in the Secret Service (GNOME Keyring).

## Build and install

Use the Ubuntu machine's normal internal Rust install, pinned by
`rust-toolchain.toml`. Keep compiler output and dependencies internal:

```sh
devbox-local /mnt/toshiba/dev/repositories/codex/bukno -- \
  sh scripts/install-linux.sh
```

`--debug` installs a development build instead. The script installs the binary
in `~/.local/share/bukno/bin/`, a `~/.local/bin/bukno` launcher, and a Bukno
application-menu entry. It does not install or configure a T3 backend.

Normal app state is in `$XDG_DATA_HOME/bukno`, or `~/.local/share/bukno` when
that variable is absent or relative. The first launch asks for the work folder.
`BUKNO_STATE_DIR` and `BUKNO_WORK_DIR` can isolate a verification run. Existing
Mac state is not copied. The installed Codex CLI supplies its own login.

Both Wayland and X11 are compiled in. The checked Ubuntu computer already had a
C linker, pkg-config, Wayland/X11 libraries and Vulkan drivers. No root package
installation was needed. If those are absent, distribution development packages
may be needed. Native screen-reader speech, desktop IME, system reduced-motion
integration and broader Linux distributions are not established by this smoke
check. Reduced motion has the existing `BUKNO_REDUCED_MOTION=1` override.

## Verification

See [the failure paths](../e2e/scenarios/ubuntu-portability.md). Retained evidence
for 3 October 2026 is at `/mnt/toshiba/dev/artifacts/bukno/2026-10-03/ubuntu/`.
The existing `codex_live` tests exercise the real app UI, coordinator, SQLite
store and installed engine. Synthetic UI checks separately exercise selection,
keyboard navigation, draft handling, accessibility structure and layout.
Linux visual references are kept in `apps/desktop/tests/snapshots/linux/` so
Ctrl labels and Vulkan rendering are checked without replacing the Mac images.
The checks also keep SQLite fixtures on the internal build drive. An early run
with SQLite under the shared evidence folder produced `DatabaseBusy`; that
fixture placement was corrected before the passing run.

Run checks from the internal working copy with an explicit evidence location:

```sh
export BUKNO_EVIDENCE_DIR=/mnt/toshiba/dev/artifacts/bukno/ubuntu-check
cargo test --locked -p bukno-desktop --test ui_checks -- --test-threads=1
BUKNO_E2E_LIVE=1 BUKNO_CODEX_EFFORT=low \
  cargo test --locked -p bukno-desktop --test codex_live -- --nocapture --test-threads=1
```

Live tests use a little real Codex usage and disposable workspaces on the
internal disk. Never put user transcripts or credentials in Git. A native-window
smoke capture can be repeated independently of the offscreen UI harness:

```sh
BUKNO_AUTOPILOT=screenshot \
BUKNO_EVIDENCE_DIR=/mnt/toshiba/dev/artifacts/bukno/ubuntu-window \
  bukno --scenario long-chat
```

Create the evidence directory first. This captures the app's own rendered frame,
not the compositor's window decorations, and closes the synthetic window.

## Source-on-share exception used for this check

The normal `devbox-local` import stalled enumerating `.git/objects` over SMB.
Direct known-object and source-file reads worked, but a fresh directory listing
also timed out. The stopped import's process was verified gone, its working copy
was empty and there was no `pending.json`; only then was its reservation released.
No active reservation or existing source was overwritten.

The Ubuntu portability changes used the machine instructions' exception for
small development tasks verified to work directly on Toshiba. Cargo's platform
check first proved source reads and compilation with all output internal. Source
edits were made directly on the canonical share, builds used an explicit
`CARGO_TARGET_DIR=~/.cache/bukno/cargo-target`, and no Git commands ran on SMB.
The existing ignored Mac Cargo configuration was preserved. The task's source
manifest records retained files and hashes. A final check also caught six
unexpected trailing null bytes after a formatter write on the share; the test
source was replaced atomically and read back before format/lint verification.
Those initial direct edits did not involve a local export.

PR publication subsequently used a one-time project-specific recovery with the
same reservation, pending-state and source/mutable-Git-metadata fingerprint
guards. The internal checkout's immutable Git objects were seeded from origin,
and referenced local checkpoint objects were restored by known object ID. New
complete Git packs were exported additively and checked byte for byte; existing
canonical objects were preserved. Source and mutable Git metadata were imported
and exported with verification. The recovery script and pack hashes are retained
with the PR publication evidence outside Git.

Keep `devbox-local` as the default for later development. Its Git-object import
needs to work before the larger T3 client implementation begins. Do not run
`cargo xtask e2e` directly on SMB: its build identification invokes Git. The
direct Cargo test commands above can be used for the narrow verified exception,
with internal compiler output and internal disposable fixtures.
