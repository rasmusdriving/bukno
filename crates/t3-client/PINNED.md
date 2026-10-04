# What this client was built against

| Item | Value |
|---|---|
| T3 Code build | `0.0.46-nightly.20261004.2644` (installed T3 Code Nightly on the Ubuntu machine) |
| T3 source revision | [`737993303d36e10674c54b95e5bd3826682c99c7`](https://github.com/pingdotgg/t3code/tree/737993303d36e10674c54b95e5bd3826682c99c7), from the build's `t3codeCommitHash` |
| Stage 1 recorded against | `0.0.46-nightly.20261003.2632`, revision `f391794a35c604d57e166a3ab48d56fc6e4e469a` |
| Orchestration protocol | 2 (`/.well-known/t3/environment`, `orchestrationProtocol` socket parameter) |
| Effect | `4.0.0-rc.115` with T3's `patches/effect@4.0.0-rc.115.patch` |
| Recorded on | Fixtures: 3 October 2026, from the live server, with `t3-probe`. Commands: 4 October 2026 |

The server updated itself to the newer build on 4 October. Between the two
revisions the contracts Bukno uses changed only by additions: a
`prepared-run.retry` command, an optional `workspacePreparation` on runs and
`defer_start`, and a scope check on every RPC that fails with the
`EnvironmentAuthorizationError` Bukno already reads as "pair again". The
replay checks pass against the Stage 1 fixtures, and the Stage 2 live check ran
against the newer build.

## Source files the wire format comes from

At the revision above:

- `packages/contracts/src/environmentHttp.ts`, `auth.ts`, `environment.ts`: HTTP paths, the token exchange form, tickets and error bodies.
- `packages/contracts/src/orchestrationV2.ts`, `orchestrationProject.ts`, `server.ts`: shell and thread stream items, turn items, domain events, server config.
- `packages/contracts/src/rpc.ts` and `apps/server/src/auth/RpcAuthorization.ts`: method names and the scope each needs.
- `apps/server/src/ws.ts`, `apps/server/src/orchestration-v2/ShellStream.ts`, `LiveStreamBudget.ts`, `threadHistoryPaging.ts`: socket upgrade, resume and snapshot fallback rules, buffer limits, history pages.
- `packages/contracts/src/orchestrationV2.ts` (`OrchestrationV2Command`, `OrchestrationV2ThreadLaunchInput`, `OrchestrationV2RuntimeRequest`, `OrchestrationV2Run`), `providerPolicy.ts`, `auth.ts`: the commands Bukno sends, approval decisions, question answers, queue fields, and the `orchestration:operate` scope.
- `packages/client-runtime/src/operations/commands.ts` and `state/threadRequests.ts`: how T3's own client builds each command and picks the pending approvals and questions.
- `apps/server/src/orchestration-v2/Orchestrator.ts` (command receipts): a repeated command ID returns the first result instead of running again.
- `packages/client-runtime/src/rpc/session.ts`, `state/orchestrationV2Projection.ts`, `state/threadSort.ts`: how T3's own client frames requests, applies events and orders chats.
- Effect `src/unstable/rpc/RpcMessage.ts`, `RpcSerialization.ts` (`layerJson`), `RpcClient.ts` (`makeProtocolSocket`, pinger): message shapes, framing, acknowledgements and pings.

## Recorded sample messages

`fixtures/` holds frames recorded from that server and passed through
`fixtures/sanitize.py`, which replaces every chat text, title, path and
command with placeholder text. Structure, IDs, enums, sequence numbers and
timestamps are kept. The thread fixtures also empty `nodes` and `turnItems`,
which the client does not read. `tests/replay.rs` runs them through the real
decoders and state code.

| File | What it holds |
|---|---|
| `shell.jsonl` | `server.getConfig`, then `subscribeShell`: snapshot, catch-up marker, three metadata-only snapshot frames |
| `long-thread.jsonl` | `subscribeThread` for a 362-row chat: bounded snapshot and marker |
| `live-thread.jsonl` | `subscribeThread` for a chat while it ran: snapshot, marker, then live `turn-item.updated`, `node.updated`, `provider-turn.updated` and `message.updated` events |

## When T3 changes

Protocol 2 alone does not promise every wire detail. After a T3 update:
record new fixtures with `t3-probe record`, sanitize them, run the replay
checks and the live check in `e2e/scenarios/t3-read-only.md`, and update the
revision here. Unknown types already decode to visible unknown values rather
than failing, so most additions degrade to "Unsupported item" rows.
