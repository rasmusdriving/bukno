# Automatic T3 setup

Bukno finds the local server from T3's `userdata/server-runtime.json` (or its
`dev` variant), verifies the live process and public environment identity,
then pairs through `t3 pair`. The short-lived link stays in memory. The access
sign-in stays in the system keychain with the existing read/operate scopes.
A saved connection is reused, including after the server changes its port.
Existing remote connections remain available.

If the desktop app is installed but closed, Bukno opens it normally and waits
for its server. It uses the CLI bundled with that installation for pairing.
If an installed CLI has no running server, or T3 was downloaded by Bukno,
Bukno starts `t3 serve` on a dynamic loopback port in its own separate data
folder. It never opens a second writer on an existing T3 database. Attached
servers and persistent managed servers keep running when Bukno closes.

If T3 is missing, **Download T3 Code** installs the self-contained upstream CLI
from the exact release in [PINNED.md](../crates/t3-client/PINNED.md).
SHA-256 values are pinned from that release's official assets. Downloads use
HTTPS, stream to disk, reject a checksum mismatch, and extract only safe
regular files/directories. Staging is cleaned on failure or cancellation.
An existing installation is kept intact. There is no Node, npm, shell script,
administrator prompt or global PATH change in this installation flow.

The default launch detects CLI paths, common per-user install locations and
T3 desktop installations. The current download matrix is Linux x64/ARM64,
Apple Silicon macOS, and Windows x64/ARM64. Intel Mac has no self-contained
asset in this pinned upstream release, so it needs an installed T3 desktop
app or an explicit connection. macOS and Windows runtime behavior is still
unverified; Windows compilation passes.

A fresh server registers the chat folder chosen in onboarding through
`t3 project add`. The recommended folder is `~/Documents/Bukno chats`.
An existing server names the project Continue will open before the user
continues. No message is sent during setup. New chat and the next launch
remember the chosen T3 project; existing direct Codex project chats remain
available. Advanced connection keeps the explicit remote address/link form.
Automatic discovery accepts loopback and addresses assigned to this computer; it does not scan a network or decrypt T3's remote catalog.

## Internal files

Under Bukno's application-data folder:

- `t3-runtime/<version>/`: the verified, self-contained release.
- `t3-server/`: the separately managed T3 data and runtime record.
- `t3-environments.json`: server identity/address, scopes and sign-in expiry.
- `t3-client-state.json`: drafts and the selected T3 project/chat.

T3 owns its database and agent children. Bukno's own token stays in the
keychain. These files are not source or Git deliverables. Keep them on an
internal local filesystem.

## Repeatable verification

Failure paths: [t3-setup-failure-paths.md](../e2e/scenarios/t3-setup-failure-paths.md).
From the internal checkout on Ubuntu, with a running local T3:

```sh
BUKNO_EVIDENCE_DIR=/mnt/toshiba/dev/artifacts/bukno/<date>/t3-setup \
  sh scripts/t3-setup-e2e.sh
```

The script drives the real native onboarding through AccessKit and egui's
renderer. It checks an attached server, a clean official download, pairing,
creation of a non-Git chat folder, reuse after a Bukno restart, automatic
startup after a stopped managed server, a live invalid runtime record, and
an offline download with staging cleanup. Test state and runtime files stay
internal; screenshots and `result.json` go to the evidence folder. The test
stops its disposable managed server and cleans its successful scratch run.

Closed desktop startup can be checked on Ubuntu without interrupting the
user's active T3: run the same test with `BUKNO_T3_SETUP_MODE=desktop`, an
isolated `BUKNO_T3_SETUP_ROOT`, `BUKNO_T3_HOME` and `XDG_CONFIG_HOME`, and
`BUKNO_T3_CLI` / `BUKNO_T3_CLI_ENTRY` pointing to the installed desktop and its
bundled `apps/server/dist/bin.mjs`. Stop only that isolated server afterward.

Ubuntu evidence for 4 October 2026 is in
`/mnt/toshiba/dev/artifacts/bukno/2026-10-04/t3-setup/`.
The actual native executable also kept the managed server alive after a full
process exit and reused it after relaunch (`native-window/process-result.json`).
All 19 existing UI checks pass. The direct Codex checks explicitly use their
recovery constructor so they exercise the existing direct route.
The existing real Codex and Claude Stage 2 scenario also passed, including
approvals, questions, queue/steer, Stop/Resume, restart and lost replies.
Manual pairing scenarios explicitly set `BUKNO_T3_AUTO_SETUP=0` to avoid
mixing automatic setup with their controlled pairing sessions.

Development-only overrides: `BUKNO_T3_HOME`, `BUKNO_T3_CLI`,
`BUKNO_T3_CLI_ENTRY`, and `BUKNO_T3_AUTO_SETUP=0`.
`BUKNO_T3=0` keeps the direct Codex recovery path.
