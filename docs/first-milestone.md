# First milestone: Mac daily driver

Goal: real Codex and Claude text workflows in the Mac app, finished to daily-driver quality, with Windows completed before public release. Dictation and live voice follow through Codex app-server and do not block day one.

On 3 October 2026 the T3 backend was agreed ([decision 37](decisions.md)). The
passes below describe the existing direct-provider implementation. Pass 2 (the
Claude bridge), Bukno's own delegation engine and the remaining direct-Codex
failure rows are paused; T3 replaces them. The T3 stages follow.

## T3 Stage 1: read-only client

- [x] Record the decision and pause the replaced work ([decision 37](decisions.md)).
- [x] Write the failure cases first: [t3-client-failure-paths.md](../e2e/scenarios/t3-client-failure-paths.md).
- [x] `crates/t3-client`: identity and protocol check, pairing (read-only scope in Stage 1), keychain storage, socket tickets, Effect RPC framing with acknowledgements and pings, shell and thread subscriptions with `afterSequence` resume, history pages, unknown types kept visible, pinned revision and recorded fixtures ([PINNED.md](../crates/t3-client/PINNED.md)).
- [x] One connection layer in the app (`apps/desktop/src/sources.rs`): T3 chats carry their server ID; the direct Codex path is unchanged.
- [x] Add environment screen with status and models.
- [x] Live end-to-end check against the Ubuntu server: [t3-read-only.md](../e2e/scenarios/t3-read-only.md).
- [ ] Sol reviews Stage 1.

## T3 Stage 2: full text flow

- [x] Write the failure cases first: [t3-operate-failure-paths.md](../e2e/scenarios/t3-operate-failure-paths.md).
- [x] Pair with `orchestration:read orchestration:operate`; a Stage 1 sign-in asks to pair again.
- [x] Typed commands with one command ID per action (`crates/t3-client/src/command.rs`); an outbox that marks lost replies "Not confirmed", settles them from the chat by the message or chat ID Bukno chose, and offers Send again with the same ID. Never resends by itself.
- [x] New chats in a T3 project with a Codex or Claude model; composer, approval and question cards, Stop, queue (Enter while working), Steer (button or Alt+Enter), Steer now, Remove, Resume after Stop, T3's runtime modes in the permission menu.
- [x] Drafts, the open chat and unsettled sends kept in `t3-client-state.json`; after a restart an unsettled send is checked with its original IDs, never resent as a new message.
- [x] Live end-to-end check against the Ubuntu server with a demo video: [t3-operate.md](../e2e/scenarios/t3-operate.md).
- [ ] Review Stage 2. Then confirm the T3 route and rewrite the backend specification.

This is a checklist. The [first version specification](first-version-specification.md) defines what each item means, the pass it lands in, and its acceptance evidence; where this list and the specification differ, the specification wins ([decision 31](decisions.md)). Claude builds the foundation and Sol reviews each pass ([decision 34](decisions.md)).

## Done before implementation

- [x] Design system and 15-screen user journey, approved 1 October 2026 ([docs/design](design/README.md)).
- [x] First version specification approved, with review changes ([decision 31](decisions.md)).

## Pass 0: foundation and native UI gate (closed 2 October 2026)

Merged as PR 1 after Sol's review. Two by-hand checks move to Pass 3, listed under it.

- [x] Install the Rust toolchain on the internal drive and point Cargo build output there, per [AGENTS.md](../AGENTS.md).
- [x] Scaffold the workspace, crates, xtask, and token loader ([spec sections 3 and 4](first-version-specification.md#4-repository-scaffold)).
- [x] Build the coordinator state machine skeleton in `crates/core` ([section 2](first-version-specification.md#coordinator-as-one-state-machine)).
- [x] Build the transcript widget first and pass its criteria with a large synthetic chat ([section 13](first-version-specification.md#the-transcript-widget)). If egui fails a must-have, compare the next toolkit now ([decision 13](decisions.md)).
- [x] Check IME, Swedish text, keyboard-only use, narrow windows, and idle repaint; measure the working animation's CPU. The orb was replaced by the StreakLabel at 10 fps, about 2.5 % of one core ([decision 35](decisions.md)).
- [x] Set up egui_kittest so UI checks produce repeatable screenshots.
- [x] Add Mac and Windows compilation checks. Mac comes first; Windows compiles in CI so the shared code stays portable ([decision 30](decisions.md)).
- [x] Sol reviews Pass 0. Ready to merge as the synthetic foundation, no open findings; merged as PR 1 on 2 October 2026. Review evidence is in the artifact folder (`review-pr1-*`).

## Pass 1: complete Codex flow on Mac

Built and run end to end on 2 October 2026 (commit b0a0c28, Codex 0.158.0). Evidence is in the artifact folder under `2026-10-02/b0a0c28f76/macos/codex/`.

- [x] Enumerate the Codex failure rows in [section 18](first-version-specification.md#18-failure-paths-to-enumerate-before-adapter-implementation) before writing the adapter: [pass1-codex-failure-paths.md](../e2e/scenarios/pass1-codex-failure-paths.md), with what the installed engine actually does.
- [x] Onboarding with the work folder choice; app state on the internal drive ([section 5](first-version-specification.md#5-file-placement-and-application-data)).
- [x] The nine Pass 1 tables, delivery outbox, transcript items, and writer lease ([section 6](first-version-specification.md#6-domain-model-and-durable-records)).
- [x] Tolerant Codex adapter, last working version record, and revert ([sections 9 and 9a](first-version-specification.md#9a-engine-versions-and-reverting)). Revert proven with the old file still on disk; the npm reinstall route is built but not yet run.
- [x] Codex permission presets and decision cards ([section 11](first-version-specification.md#11-permissions-and-user-questions)): "Ask before commands" (default) and "Ask before changes", both proven live.
- [x] Complete a task in a disposable Git repository and in a projectless chat through the real app: streamed output, one declined and one allowed action, Stop, saved draft, resume after restart, and preserved unrelated edits. Also an engine killed mid-turn, reconciled from Codex history without a resend.
- [x] Launch Bukno.app from Finder and retain the Codex evidence package.
- [ ] Failure rows still to prove, mostly with a protocol-peer process: see the coverage table in the failure-path list.
- [ ] Sol reviews Pass 1.

## Pass 2: Claude on Mac

- [ ] Enumerate the Claude failure rows before writing the bridge.
- [ ] One packaged bridge for all Claude chats, with Rust-defined protocol types ([section 10](first-version-specification.md#10-claude-adapter-and-bridge)).
- [ ] Same disposable-repository and projectless flows through the installed Claude Code CLI, including configuration loading, approvals, questions, Stop, and resume.
- [ ] Measure bridge and engine memory; decide the bridge packaging runtime.
- [ ] Sol reviews Pass 2.

## Pass 3: Mac daily driver

- [ ] Carried from Pass 0: by-hand checks of VoiceOver, a real input method, and the real Reduce motion setting in [e2e/scenarios/pass0-native-checks.md](../e2e/scenarios/pass0-native-checks.md), before the component set grows.
- [ ] Carried from Pass 0: early Windows UI smoke check (typing, selection, scaling, Narrator) when a Windows machine is available.
- [ ] Full navigation and component set, model picker, To-do, change strip, attachments, settings, usage, menus, and notifications.
- [ ] Drive-loss, engine-loss, quit, sleep/wake, and crash recovery behavior.
- [ ] Run every applicable scenario in [section 20](first-version-specification.md#20-end-to-end-verification-and-evidence), including E17 engine update and revert.
- [ ] Performance measurements from [section 21](first-version-specification.md#21-performance-acceptance), compared with the same workload in the existing desktop app.
- [ ] Review native screenshots against the design references; update the feature map with observed results.
- [ ] Sol reviews Pass 3. Daily use can begin.

## Acceptance

On Mac, both engines edit a disposable repository, show change totals and an expandable file summary, handle permission decisions, and resume after restart. Both complete and resume projectless chats with persistent drafts and output. Stop leaves no undisclosed running work. Memory stays bounded across repeated cycles. Store a separate result for every OS and engine; Mac success does not establish Windows compatibility.

Treat GUI idle below 100 MiB and a basic GUI-plus-Codex workload below 500 MiB as initial targets, not promises. Save evidence in the artifact folder named in [AGENTS.md](../AGENTS.md).

## Pass 4 and 5: Windows and public release

- [ ] Record the supported OS/architecture matrix and verify native Windows engine and shell requirements.
- [ ] Build and launch the packaged app on a Windows machine without a developer terminal's environment, and repeat both providers' workflows.
- [ ] Verify owned process cleanup, permissions and sandbox differences, drive paths, IME, Narrator, 100%/150%/200% scaling, notifications, sleep/wake, external opening, and the Windows engine revert routes.
- [ ] Complete clean-machine installation and the public-release gates in [Pass 5](first-version-specification.md#pass-5-public-distribution).

## Later milestone: Codex speech integration

- [ ] Establish exact protocol modes, authentication, and handoff controls for dictation and live voice on both platforms.
- [ ] Account for the [desktop speech findings](research/README.md#speech-follow-up): default WebSocket voice requires API-key auth in the inspected source, the desktop has additional call setup, and its dictation client is separate from app-server RPC. Do not equate bundling with service access.
- [ ] Complete a realtime conversation with audible response and visible Codex thread work, then interrupt and stop it.
- [ ] Capture dictation into an editable draft and send it to either provider only through the normal composer.
- [ ] Verify dictation alone executes no coding task, including when the session starts, stops, or reconnects.
- [ ] Verify microphone/playback release, device changes, late transcript handling, and memory with all owned children included.
- [ ] Retain repeatable speech evidence separately from the first text milestone.

Neither speech feature blocks day one. Keep the agreed Codex app-server backend; do not silently substitute another speech service.

## Planned delegation proof: milestone to select

The architecture must support this now; a working mixed-provider task flow is a later acceptance gate whose delivery timing is not yet agreed.

- [ ] Have Codex delegate a bounded read-only task to Claude, receive a referenced result, and continue its original task.
- [ ] Repeat Claude to Codex, then same-provider delegation, on both operating systems.
- [ ] Show child identity, chosen model, workspace, progress, approvals, and final result in Bukno.
- [ ] Open a child from Delegated work in the main chat pane, send follow-ups to running and completed children, and return to the parent. Verify separate drafts, correct message recipient while navigating, and revised-result delivery without duplicate execution or automatically restarting a completed parent.
- [ ] Verify duplicate delegate calls, worker failure, parent cancellation, parent restart, and result delivery after reconnection.
- [ ] Before child editing, prove writer ownership transfer or isolated worktree changes without damaging unrelated edits.
- [ ] Retain a task-tree record, provider events, and the final parent response as repeatable evidence.
