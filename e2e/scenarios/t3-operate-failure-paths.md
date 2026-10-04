# Stage 2 failure paths: sending through T3

Written 4 October 2026, before the Stage 2 client code, as the
[T3 backend plan](../../docs/t3-backend-audit.md) requires. Stage 2 lets Bukno
start chats, send, answer approvals and questions, stop, queue and steer, and
pick up again after restarts, all through T3. Each row says what T3 does, what
Bukno must do, and how it is proven. The "Observed" column is filled in only
from real runs against the Ubuntu T3 server (`t3-operate/run-8`, 4 October
2026, all 23 checks pass, after both reviews), or says why it was not run.

Written against T3 `0.0.46-nightly.20261003.2632`; run against
`0.0.46-nightly.20261004.2644` (revision `737993303d36e10674c54b95e5bd3826682c99c7`)
after the server updated itself. See
[`crates/t3-client/PINNED.md`](../../crates/t3-client/PINNED.md).

## What the protocol looks like

| Fact from the pinned source | Consequence for Bukno |
|---|---|
| `orchestration.dispatchCommand` and `orchestration.launchThread` need the `orchestration:operate` scope; a session without it is refused at call time | Ask for `orchestration:read orchestration:operate` when pairing. A Stage 1 sign-in only has read: show "Pair again to send", never try |
| Every command carries a `commandId`. The server stores a receipt per ID. The same ID again returns the first result (or the first rejection) instead of running twice; the receipt is tied to its thread | One command ID per user action, made before the first try and reused for every retry of that action. Retrying an unconfirmed command is safe |
| `dispatchCommand` returns `{sequence}` once the command is accepted. Acceptance is not completion: the provider work follows as events | Show a send as sent on acceptance, and the run's progress from the thread stream |
| `message.dispatch` takes a client-chosen `messageId`. With `serverResolvedCommandContext` (this server has it) the client sends `start_immediately` plus `deliveryIntent` `auto` or `steer`, or `queue_after_active` to queue | Bukno chooses the message ID, so it can look for that ID in the chat after a lost reply |
| The user message's `inputIntent` says what happened: `turn_start`, `steer`, `queued_turn` or `promoted_queued_to_steer` | Label mid-run messages from `inputIntent`, not from what was asked |
| `run.interrupt` stops one run by ID. T3's client sends `holdQueue: true`, so queued messages wait. `queue.resume` releases them | Stop holds the queue. Show the waiting messages with Resume and Remove |
| Queued messages are runs with status `queued` and a `queuePosition`. `queued-message.promote-to-steer` and `queued-run.cancel` act on one | Show each queued message with Steer now and Remove |
| Approvals and questions are `runtimeRequests` with `status` `pending`, `resolved`, `expired` or `cancelled` and a `responseCapability` of `live`, `message` or `not_resumable`. Their text is on the matching `approval_request` or `user_input_request` turn item | Show a card only while the request is pending. Answer with `runtime-request.respond` and the request ID. A `not_resumable` request cannot be answered |
| Question answers are keyed by question ID: an option's `value` (or its label when it has none), a list for multi-select, or free text | Send the option value, not the label shown |
| `orchestration.launchThread` creates the chat and sends its first message in one call, with a client-chosen `threadId` and `messageId` | A lost reply is checked by the thread ID appearing in the chat list |
| After a server restart T3 may hold a chat's queue (`queueHeld`) until the user resumes it | Show it as a paused queue with Resume |

## Failure rows

| # | Failure | T3 source | Bukno behavior | Proof | Observed |
|---|---|---|---|---|---|
| O1 | The saved sign-in is from Stage 1 and only has `orchestration:read` | Commands refused with an authorization error | The composer says "Pair this server again to send from Bukno." Nothing is sent | Live: a read-only pairing || Partly. Pairing gave `orchestration:read orchestration:operate` on both sides. A read-only sign-in is refused in the client before sending; not run against the server. Pairing again keeps commands still in doubt, with their IDs |
| O2 | T3 rejects a command (validation, run no longer active, request already answered) | `Exit` failure with a tagged error; a rejected receipt | Show T3's reason under the composer or card. The draft is kept | Live: answer a request twice; stop a finished run || Not run: no command was refused during the runs. A refusal of a message or new chat, or a server defect, is kept "Not confirmed" with T3's reason, because T3 can store a message before a later step fails; only refusals of other commands and of the sign-in are final |
| O3 | The connection drops after a command was written, before T3 answered | The command may or may not have run | Mark it "Not confirmed". After reconnecting and catching up, look for its message or thread ID. Found: it was sent. Not found: offer Send again with the same command ID. Never resend by itself | Live: relay drops the socket right after the request || Pass. Reply dropped after T3 ran it: shown as sent from the chat, once in T3. Request held, then cut: "Not confirmed", Send again, once in T3. Reply dropped and Bukno quit: the send was saved, settled from the chat after the relaunch, its draft cleared, once in T3 |
| O4 | Enter pressed twice, or Send clicked during a send | n/a | One command. Send is unavailable while a send is unconfirmed | Live: two quick sends, one user message || Partly. Every send was one command and one message in T3; a double press was not tried separately |
| O5 | Send while not connected | n/a | Refused before anything is written: "Not connected to T3. Nothing was sent." Draft kept | Live: relay refusing, then send || Pass. Send unavailable ("Not connected to T3"), draft kept, nothing in T3 |
| O6 | A steer races the end of the run | The server resolves the intent against its own serialized state; the message may start a new run instead | The label follows the message's `inputIntent` | Live where it happens; source otherwise || Pass for the labels: T3 recorded `promoted_queued_to_steer` and `steer`, and the captions followed. The race itself was not provoked |
| O7 | An approval is answered in T3 (or another client) while Bukno shows it | The request becomes `resolved`; a second answer is rejected | The card disappears when the request is no longer pending. A late click shows "already answered" | Live: answer through T3's own API while the card shows || Not run |
| O8 | The provider session that asked has gone (`not_resumable`) | The request cannot be answered | The card explains this and offers no Allow | Source; live if it occurs || Not run (did not occur) |
| O9 | An approval arrives a moment after Enter sent a message | n/a | The card's keys act only while it has focus (Stage 0 rule, reused) | Live: Enter does not answer || Pass. Enter in the empty composer left the approval pending |
| O10 | Stop with no active run, or Stop racing the end of the run | `run.interrupt` for a finished run is rejected or does nothing | Show the run's final state; no error loop | Live || Pass for Stop: the run ended `interrupted`. The race with completion was not provoked |
| O11 | Stop with messages queued | `holdQueue: true` keeps them | "N messages wait. Resume, or remove them." Nothing starts by itself | Live || Pass. The queued message was held after Stop and sent after Resume |
| O12 | A queued message is promoted, edited or removed elsewhere | Run status changes | The list follows the run statuses | Live through T3's API || Not run |
| O13 | New chat: the launch reply is lost | The thread may exist | Look for the chosen thread ID in the chat list after catching up; open it if found, otherwise offer Try again with the same command and thread IDs | Live: relay drop during launch || Not cut. Run 3 and 4 found a Bukno bug on this path (a send forgotten before the client reported it); fixed. A new chat opens once T3 shows the chat with its first message (the chat alone does not prove the message landed); an unconfirmed launch offers Send again on the new-chat screen |
| O14 | A provider is disabled or not installed in T3 | `server.getConfig` reports it | It is not offered for a new chat | Live: the provider list matches T3 || Partly. Models match the server (Stage 1 check). Claude Haiku 4.5 is listed but its requests fail on this server; Bukno showed T3's error in the chat |
| O15 | Bukno quits with a draft typed in a T3 chat | n/a | The draft is saved on the internal drive and comes back; nothing is resent | Live: quit and relaunch || Pass. The unsent draft came back after the relaunch; nothing was resent. A send still unconfirmed at quit is saved with its IDs and checked again, not left as a draft to send twice |
| O16 | Bukno quits while a T3 run works | The run continues in T3 | After relaunch the chat shows it working and Stop works | Live || Pass. The running Codex chat reopened as working and Stop worked |
| O17 | T3 restarts | Socket closes; T3 may hold the queue | Reconnect and catch up (Stage 1). A held queue shows Resume | Relay refuse-and-drop stands in; a real restart is not run because this agent runs inside that server || Relay stands in (refuse and drop); a real T3 restart and its held queue were not run |
| O18 | A file in the workspace has uncommitted changes | T3 runs agents in the project folder | Bukno never writes workspace files itself. A dirty file the chat was not asked to touch stays as it was | Live: hash a dirty file before and after || Pass. `notes.md` byte-identical before and after |
| O19 | A command runs twice | Receipts prevent replays of one ID | Each user action has one command ID; the run counts it | Live: T3's snapshot has one user message per send and one run per started message || Pass. Every one of the 11 sent texts is in T3 exactly once |
| O20 | Token leaks | n/a | As F19: keychain and headers only. The command log names the method and command ID, never the message text | Live: search logs and state for tokens || Pass. No token, link or chat text in the log, state or evidence |
| O21 | A question has options with separate values, several choices or free text | Answers keyed by question ID | Option value sent; a list for multi-select; free text when allowed | Live where a provider asks; source otherwise || Pass for one single-choice question: T3 recorded the answer `Tea`. Multi-select is supported (several options toggled, every value sent) but not run live; separate option values not run. Answers belong to one request. A live question offers Stop instead of Skip, which T3 allows only for questions answered by message |
| O22 | The user switches chats while a command is in flight | n/a | Every command carries its environment and thread; its result lands on that chat | Live: switch during send || Partly. Switching between the Codex and Claude chats during work kept every message in its own chat (per-chat counts above) |
