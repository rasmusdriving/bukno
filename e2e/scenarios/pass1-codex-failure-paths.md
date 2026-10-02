# Pass 1 Codex failure paths

Written 2 October 2026, before the Codex adapter, as required by
[section 18](../../docs/first-version-specification.md#18-failure-paths-to-enumerate-before-adapter-implementation).
Each row says what must happen and how it is proven. Add new failures here as
they are found, so every check has a specific behavior to prove.

How each row is proven:

- **Live**: the real app with the real Codex engine and your existing login.
- **Peer**: a small fake app-server process on the real stdio transport, for
  failures that cannot be caused reliably with the real engine. Labeled
  Simulated protocol coverage; it never replaces a live result.
- **Replay**: a recorded input sequence through the real coordinator and storage.

## What the installed engine actually does

Observed with Codex 0.158.0 on this Mac, using small probe scripts that speak
the app-server protocol directly. Raw logs:
`/Volumes/TOSHIBA Workspace/dev/artifacts/bukno/2026-10-02/codex-protocol-probe/`.

| Observation | Consequence for Bukno |
|---|---|
| `codex` on PATH is an npm JavaScript wrapper that starts a native binary in `node_modules/@openai/codex-darwin-arm64/vendor/aarch64-apple-darwin/bin/codex` | Launch the native binary directly, so no Node process sits between Bukno and the engine |
| The user's `~/.codex/config.toml` sets `model = "gpt-6.1-sol"`. Through Bukno's app-server it fails every turn with "not supported when using Codex with a ChatGPT account" and is missing from `model/list`, yet it works in the Codex app and the Codex CLI. Cause not known yet (open question below) | For now Bukno uses the config model only when `model/list` offers it, otherwise the list's default, and says so |
| The same config sets `approval_policy = "never"` and `sandbox_mode = "danger-full-access"` | Always send Bukno's preset (sandbox and approval policy) on `thread/start`, `thread/resume` and `turn/start`. Never inherit the config's policy silently |
| Configured MCP servers start with every thread and run outside the Codex sandbox | No preset may be labeled read only while MCP tools are loaded |
| `workspace-write` with `untrusted` asks before each shell command; `decline` keeps the turn going and only the allowed command runs | Proven presets and the decision card's Allow and Decline |
| `turn/interrupt` on an active turn returns `{}` and `turn/completed` with `interrupted` arrives within 20 ms | Normal Stop path |
| `turn/interrupt` on a turn that already finished gets **no response at all** | Every control request has a timeout; a missing reply is not a failure of the run |
| Closing the engine's stdin makes it exit within 40 ms | Crash safety: the engine dies with Bukno |
| `thread/resume` after an engine restart keeps context (a code word from the first process was recalled in the second) | Resume path |
| `turn/start` accepts `clientUserMessageId`, which comes back as the `clientId` of the user message item | Reconciliation key: Bukno sends its message ID, so after a crash it can tell whether a message reached the engine |
| `turn/steer` without an active turn fails with "no active turn to steer"; an unknown thread fails with "thread not found" | Typed errors, shown in words |
| `/usr/bin/git` exits 69 because the Xcode license is not accepted | Prefer a real Git (Homebrew, Command Line Tools). If none works, treat the folder as non-Git and say why |

## Open questions

- Why is `gpt-6.1-sol` refused through Bukno but accepted by the Codex app and CLI on the same account? Things to compare: the client name Bukno sends in `initialize` (`bukno`, which Codex records as the originator), the `experimentalApi` capability, the engine version (the desktop app bundles 0.159.2, the CLI is 0.158.0), and whether the CLI applies the config model through a profile or override that `model/list` does not show. To debug later.

## Failure rows

| # | Failure | Required behavior | Proof |
|---|---|---|---|
| C01 | Codex not installed or not found from a Finder launch (no shell PATH) | Setup names Codex as missing with install guidance and a Choose file action; the rest of Bukno opens | Live (override to a missing path) |
| C02 | `codex` is the npm wrapper | The native binary it would start is launched instead; diagnostics record both paths | Live |
| C03 | Engine exits during startup or the handshake fails | Not working state on the engine card naming the version, with Check again and, when known, the last working version | Peer |
| C04 | Newer engine version breaks the methods Bukno uses | "Codex X is not working with Bukno. The last working version is Y." with Use Y, Check again, Show details. Saved chats stay readable | Peer |
| C05 | Engine updated while Bukno runs | Version read at each process start; a running process is not restarted to pick it up | Live (version is recorded per process) |
| C06 | Revert target gone from disk | Show the manual install command; keep the pin unchanged | Peer |
| C07 | Not logged in or the login expired | Show the account state and how to log in with the Codex CLI; keep the draft; no API-key fallback | Peer (account/read returns no account) |
| C08 | Config default model rejected for the account | Bukno sends a model from `model/list`; a turn that still fails on its model shows the engine's reason; no silent switch to another model | Live (observed) |
| C09 | Config sets a permissive approval policy or sandbox | Bukno's preset always overrides it; the effective preset is shown before the first send | Live |
| C10 | Unknown notification | Ignored or shown as generic activity; no crash | Peer |
| C11 | Malformed JSON line on stdout | Counted and logged as a protocol error; the connection keeps reading | Peer |
| C12 | Frame larger than 8 MiB | Protocol error; the connection is treated as lost and active runs become Outcome unknown; never parsed partially | Peer |
| C13 | Unknown server request that needs a reply | Rejected with a JSON-RPC error and a visible note in the chat; never an invented approval | Peer |
| C14 | MCP elicitation, permission request, dynamic tool call or auth refresh request | Same as C13 for Pass 1 (not answerable yet), with the request kind named | Peer |
| C15 | Huge streamed output | Text is coalesced for display; Stop and the composer stay responsive | Live (long reply) |
| C16 | Repeated Send click or retry | Exactly one local message and one turn | Replay |
| C17 | User switches chat right after Send | The message stays with the chat it was sent from | Replay and kittest |
| C18 | Engine dies before `turn/start` is acknowledged | Delivery Unknown; run Outcome unknown; never resent automatically; explicit Resend explains possible duplicate work | Peer |
| C19 | Engine dies after acknowledgement, mid-turn | Run Outcome unknown; partial reply kept; decisions expired; lease kept until reconciled | Peer and Live (kill engine) |
| C20 | Reconciling after a crash finds the message (by `clientId`) | Delivery Acknowledged; run takes the turn's real status; final reply filled in | Live (crash then relaunch) |
| C21 | Reconciling finds no trace of the message | Stays Outcome unknown with Resend offered | Peer |
| C22 | Approval answered after the run ended, or from an older connection | Rejected as stale; no effect on any newer run | Replay |
| C23 | Same numeric request ID reused by a new process | Decisions are keyed by connection generation plus request ID; the old card cannot answer the new request | Replay |
| C24 | Enter that sent a message must not approve a card that arrives just after | Card shortcuts act only when the card has focus | kittest |
| C25 | Stop races with completion | One settled outcome; the later event is ignored | Replay |
| C26 | Stop gets no reply (turn already finished) | Control timeout; run settles from the engine's turn status, not from the missing reply | Live (observed) |
| C27 | Stop takes longer than five seconds | The chat says stopping is taking longer and offers Force stop, naming the other chats that share the engine | Peer |
| C28 | Resume of a thread the engine no longer has | Chat and files kept; explain that context is missing; offer an explicit new continuation only | Peer |
| C29 | Chat continued outside Bukno (CLI or desktop app) | Notice in the chat; missing turns loaded before Send is allowed | Live |
| C30 | Two chats write to the same folder, a symlink to it, or a nested folder | Detected as one workspace; second run waits (Queue) by default; Run anyway is explicit and labeled on both | Replay and Live |
| C31 | Unrelated dirty file in the repository | Never touched or reverted by Bukno; its hash is unchanged after the run | Live |
| C32 | Git missing or broken (Xcode license shim) | Folder treated as non-Git with the reason shown; work still possible | Live (observed) |
| C33 | Work folder or project folder missing at launch | App opens; affected chats show Unavailable and keep their identity; no replacement folder is created | Live (rename a folder) |
| C34 | SQLite write fails (disk full or read-only) | Nothing is sent; the draft stays with "Not saved" in words | Peer (read-only state folder) |
| C35 | Second Bukno launch on the same state folder | Refused with the owner's PID; the first instance is untouched | Live |
| C36 | Database from a newer Bukno | Bukno refuses to change it, says so, and preserves the files | Replay (bumped schema version) |
| C37 | Bukno crashes with a run active | Engine exits on end of input; next launch reconciles from the outbox and process records; no leftover engine | Live (kill -9 Bukno) |
| C38 | Quit with active work | Keep working or Stop and quit; an orderly quit stops runs, saves drafts, ends the engine and verifies it exited | Live |
| C39 | Mac sleeps with a run active | On wake, a dead transport becomes Outcome unknown and reconciles; nothing is resent | Pass 3 (recorded here, not built in Pass 1) |
| C40 | Interactive question from the engine (`item/tool/requestUserInput`) | Shown as a question card with its options and free text; a cancelled or timed-out question is never turned into permission | Peer (the stable API rarely sends it) |
| C41 | Two runs already active (the default limit) | A third submission waits visibly as queued; it is not rejected or lost | Replay |
| C42 | Approval card while the user is in another chat | The waiting chat shows Needs approval in the sidebar; answering it later still reaches the right run | kittest |

## Coverage after the first live run

Run on commit b0a0c28, 2 October 2026, Codex 0.158.0, with
`cargo xtask e2e --provider codex --scenario pass1` (live, through the real
app UI) and `cargo test -p bukno-core --test replay` (replay). Evidence:
`/Volumes/TOSHIBA Workspace/dev/artifacts/bukno/2026-10-02/b0a0c28f76/macos/codex/`.

| Status | Rows |
|---|---|
| Proven live | C02, C03, C04, C08 (observed, model chosen from the list), C09, C15 (long replies), C19, C20, C26, C31, C35, C38, plus the revert route "file still on disk" and Try latest version |
| Proven by replay | C16, C17, C21, C22, C23, C25, C30, C41, restart without resend, outside turns loaded before a send |
| Built, not yet proven | C01 (missing engine), C05, C06 (revert target gone), C07 (signed out), C10 to C14 (malformed, oversized and unknown protocol messages), C18, C24, C27 (Force stop), C28, C29 live, C30 live, C32, C33 (missing folder), C34 (store failure), C36, C37 (Bukno itself killed mid-run), C40, C42 |
| Later pass | C39 (sleep and wake, Pass 3) |

The protocol-peer process for the simulated rows has not been written yet.
