# Read-only T3 client failure paths

Written 3 October 2026, before the `t3-client` crate, as Stage 1 of the
[T3 backend plan](../../docs/t3-backend-audit.md) requires. Each row says what T3
does, what Bukno must do, and how it is proven. The "T3 source" column comes
from reading the pinned source. The "Observed" column is filled in only from
real runs against the Ubuntu server on 3 October 2026 (`t3-read-only/run-2` and later in the artifacts folder), or says why it was not run.

Pinned against T3 `0.0.46-nightly.20261003.2632`, source revision
`f391794a35c604d57e166a3ab48d56fc6e4e469a`, orchestration protocol 2, Effect
`4.0.0-rc.115` with T3's patch. See
[`crates/t3-client/PINNED.md`](../../crates/t3-client/PINNED.md).

How each row is proven:

- **Live**: the real Bukno app or the `t3-probe` tool against the T3 server
  already running on this Ubuntu machine.
- **Replay**: recorded server frames fed through the real client state code.

## What the protocol looks like

| Fact from the pinned source | Consequence for Bukno |
|---|---|
| `GET /.well-known/t3/environment` is public and returns the environment ID, label, server version, `orchestrationProtocolVersion` and capabilities | Read it first, every time a connection starts. Compare the ID with the one saved at pairing |
| A pairing link carries a one-time token in `#token=` (or `?token=`). It expires after 5 minutes by default and can be used once | Exchange it straight away. Never store or log the link |
| `POST /oauth/token` (form encoded, token-exchange grant) returns a bearer token. Its session lasts 30 days. There is no refresh for direct pairing; after expiry the user pairs again | Store the token in the system keychain with its expiry. When it expires, ask for a new pairing link |
| `POST /api/auth/websocket-ticket` with the bearer token returns a ticket that lasts 5 minutes | Get a fresh ticket for every socket connection. Never put the bearer token in a URL |
| T3 names a paired session after the pairing link's label when the link has one, not the label the client sends | Session lists show the link's label |
| `GET /ws?wsTicket=…&orchestrationProtocol=2` upgrades to a WebSocket. A missing or different protocol number returns HTTP 426 with code `orchestration_protocol_incompatible` | Always send the parameter; show 426 as "this T3 needs a newer Bukno" |
| Frames are JSON text. One frame holds one message or an array of messages. Client sends `Request`, `Ack`, `Interrupt`, `Ping`, `Eof`; server sends `Chunk`, `Exit`, `Defect`, `Pong`, `ClientProtocolError` | Implement exactly these. Any other `_tag` is kept as an unknown frame and logged, not a crash |
| A stream sends `Chunk` with a batch of values and waits for the client's `Ack` before sending the next batch | Ack only after the values are applied, so a slow app slows the server instead of losing items |
| The T3 client pings every 5 seconds and treats 3 missed pongs as a dead socket | Same rule, so a silent network drop is noticed in about 15 seconds |
| Shell and thread events carry `sequence` numbers from one global event log | Numbers are not continuous within one stream. A jump is normal, not a lost event. Drop anything at or below the last applied number |
| `subscribeShell` and `subscribeThread` accept `afterSequence`. The server replays stored events after it, or sends a fresh snapshot when the gap is too large (shell: over 1,000 events or 8 MiB; thread: over 128 events or the byte budget) | Resume with the last applied number. Treat a snapshot that arrives during a resume as a full replacement |
| On a resume, the first `snapshot` frame carries `resolvedRepositoryIdentityRoots` and holds project metadata only, with no chats and the newest sequence | Merge its projects; never treat it as the chat list or move the cursor with it, or the replayed events after it are dropped |
| `requestCompletionMarker: true` adds a `synchronized` item between catch-up and live delivery | Report a chat as current only after that marker |
| `acceptBoundedSnapshot: true` makes a long thread arrive as a recent window plus `historyCursor` | Load older items with `GET /api/orchestration/threads/:id/history?cursor=…`, newest page first |
| Thread events of a type the client does not know should be skipped while still advancing the cursor | Keep them as visible "unknown event" counts; never fail the subscription |

## Failure rows

| # | Failure | T3 source | Bukno behavior | Proof | Observed |
|---|---|---|---|---|---|
| F1 | Pairing link expired (over 5 minutes) | `/oauth/token` returns 401 `EnvironmentAuthInvalidError` with reason `invalid_credential` | "This pairing link has expired or was already used. Create a new one in T3." Nothing is stored | Live: wait out a short `--ttl` link | Not run directly (would need a 5-minute wait). Same 401 path as F2 |
| F2 | Pairing link used twice | Same 401; one-time tokens are consumed on exchange | Same message as F1 | Live: paste the same link again | Not run as its own step. The exchange maps any 400/401/403 to this message |
| F3 | Saved token revoked in T3 while Bukno is closed | Ticket request returns 401 | Stop retrying. Show "T3 no longer accepts this computer. Pair again." Keep the environment entry | Live: `t3 auth session revoke`, then start Bukno | Pass. After `t3 auth session revoke`, the ticket request returned 401 and the app showed the message and stopped retrying; pairing again with a second link worked |
| F4 | Token revoked while connected | No code path closes live sockets on revocation; every RPC checks its scope at call time. Existing streams may keep running until the socket closes | Next reconnect hits F3. Record what the open stream actually does | Live: revoke during a connection | Not observed separately: the revocation step reconnected right after revoking |
| F5 | Token past its 30-day expiry | Ticket request returns 401 | As F3, with "expired" wording, based on the stored expiry | Source only (cannot wait 30 days); stored expiry is checked before connecting | Not run (30 days). Expiry stored at pairing: 2,591,999 s after pairing |
| F6 | Wrong address (nothing listening, wrong port, not T3) | Connection refused, timeout, or a non-T3 response | "Nothing answered at this address" or "This address is not a T3 server". Never sends the pairing token anywhere but `/oauth/token` of a verified T3 descriptor | Live: wrong port; a plain web server | Pass. `http://127.0.0.1:9` showed "Nothing answered at this address (connection refused or no route)." and the link was not sent |
| F7 | Wrong server identity: the saved address now answers with a different environment ID | Descriptor returns another `environmentId` | Refuse to connect or send the token. "This address now belongs to a different T3 server" | Live: point a saved entry at another T3 data folder, or edit the saved ID | Not run through the app: needs a second T3 server. The check runs before any token is sent, at every connection |
| F8 | Protocol mismatch | Descriptor reports another protocol; `/ws` returns 426 | "This T3 uses protocol N; Bukno supports 2." No retry loop | Live: request `/ws` with protocol 1 | Pass with the probe: `/ws?orchestrationProtocol=1` returned 426 and the client reported a protocol mismatch |
| F9 | Unknown frame `_tag`, unknown shell item kind, unknown thread event type, unknown turn item type | New server builds add these | Frame: counted and logged. Shell kind: ignored and counted. Event: cursor advances, counted. Turn item: shown as "Unsupported item: <type>" in the transcript | Replay: injected unknowns through the real decoder and reducers; live run counts | Pass. Injected unknown frame tag, stream kind, turn item type and event type: one "Unsupported item: `hologram`" row, the rest logged, connection stayed up |
| F10 | Malformed JSON or a known type with a broken payload | T3's own client fails that stream | Show "T3 sent something Bukno could not read" for that chat or list, keep the rest of the connection, and reconnect that subscription | Replay: broken frame | Replay check only (`tests/replay.rs`): a known event with a broken payload is a decode error. The hub reloads that chat, or the chat list, from a fresh snapshot on the same socket, up to 3 times in a row, then reconnects |
| F11 | Slow app falling behind | Server waits for each Ack. Its live buffer holds 1,000 items or 8 MiB, then the stream fails with `LiveStreamBufferError` | Apply and Ack on the network task, not the UI frame. If the stream fails, resubscribe with `afterSequence` | Live: pause acks in the probe and watch the stream end; resubscribe | Not run |
| F12 | Gaps in sequence numbers | Normal; numbers are global | Not treated as loss. Duplicates and older numbers are dropped. Replay vs snapshot is the server's choice | Live: reconnect run counts duplicates and missing items against a fresh snapshot | Pass. Resume after cuts showed no duplicate or missing rows against T3's snapshot (355 and 358 rows); sequence numbers jumped normally |
| F13 | Server restart | Socket closes. Same environment ID after restart | Reconnect with backoff (0.5 s growing to 10 s), new ticket, resubscribe with `afterSequence`, compare with a fresh snapshot | Live: restart T3 Code | Stood in by the relay refusing and dropping connections (the agent runs inside the real server). Reconnected and matched T3 |
| F14 | Network loss without a close | No frames; pongs stop | Declare the socket dead after 3 missed pongs, then F13's reconnect | Live: block the port with a firewall rule, or stop the socket in the probe | Pass. Pings stopped being answered; "no reply from T3 for 15 seconds" exactly 15 s after the cut, then reconnect and catch-up |
| F15 | Bukno restarts | Nothing is cached locally in this stage | Load fresh snapshots on start; the token comes from the keychain | Live: quit and start Bukno | Pass. Relaunch read the token from the keychain and the open chat matched T3 (347 rows) |
| F16 | Thread deleted or archived while open | Shell sends `thread.removed` or moves it to `archive`; thread stream may end | Keep the transcript, mark it "removed in T3", stop its subscription | Live, if a disposable thread is available | Not run (no disposable chat to delete) |
| F17 | History page cursor rejected | 400 `invalid_history_cursor` | Reload the thread snapshot and start paging again | Replay: bad cursor against the live HTTP endpoint | Not run against the server; the hub reloads the chat on a 400 from the history endpoint |
| F18 | Accidental mutation | Read-only stage | The client only sends methods from a fixed read-only list (`server.getConfig`, `orchestration.subscribeShell`, `orchestration.subscribeThread`) plus GET reads. Anything else fails before it reaches the socket | Live: the server's command log shows no command from Bukno's session during the whole run | Pass. Log shows only `server.getConfig`, `orchestration.subscribeShell`, `orchestration.subscribeThread`; T3 recorded zero command receipts from any client during the run; Bukno's session holds only `orchestration:read` |
| F19 | Token leaks | n/a | Token only in the keychain and in request headers. Logs print `Bearer <redacted>`. Pairing links are never logged, saved or shown after submit. Evidence screenshots show only the address | Live: search logs, evidence and the state folder for the token after the run | Pass. Neither token nor pairing link found in the log, state folder or evidence |
| F20 | Keychain unavailable or locked | n/a | Pairing refuses to finish: "Bukno could not save the sign-in in the system keychain". Nothing is written to a file instead | Source and a run with the secret service stopped, if safe | Not run (the keychain was available) |
