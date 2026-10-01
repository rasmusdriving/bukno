# First milestone: Mac daily driver

Goal: real Codex and Claude text workflows in the Mac app, finished to daily-driver quality, with Windows completed before public release. Dictation and live voice follow through Codex app-server and do not block day one.

This is a checklist. The [first version specification](first-version-specification.md) defines what each item means, the pass it lands in, and its acceptance evidence; where this list and the specification differ, the specification wins ([decision 31](decisions.md)). Claude builds the foundation and Sol reviews each pass ([decision 34](decisions.md)).

## Done before implementation

- [x] Design system and 15-screen user journey, approved 1 October 2026 ([docs/design](design/README.md)).
- [x] First version specification approved, with review changes ([decision 31](decisions.md)).

## Pass 0: foundation and native UI gate

- [ ] Install the Rust toolchain on the internal drive and point Cargo build output there, per [AGENTS.md](../AGENTS.md).
- [ ] Scaffold the workspace, crates, xtask, and token loader ([spec sections 3 and 4](first-version-specification.md#4-repository-scaffold)).
- [ ] Build the coordinator state machine skeleton in `crates/core` ([section 2](first-version-specification.md#coordinator-as-one-state-machine)).
- [ ] Build the transcript widget first and pass its criteria with a large synthetic chat ([section 13](first-version-specification.md#the-transcript-widget)). If egui fails a must-have, compare the next toolkit now ([decision 13](decisions.md)).
- [ ] Check IME, Swedish text, keyboard-only use, VoiceOver, narrow windows, and idle repaint; measure orb CPU at the capped frame rate.
- [ ] Set up egui_kittest so UI checks produce repeatable screenshots.
- [ ] Add Mac and Windows compilation checks; run the early Windows UI smoke check when a Windows machine is available.
- [ ] Sol reviews Pass 0.

## Pass 1: complete Codex flow on Mac

- [ ] Enumerate the Codex failure rows in [section 18](first-version-specification.md#18-failure-paths-to-enumerate-before-adapter-implementation) before writing the adapter.
- [ ] Onboarding with the work folder choice; app state on the internal drive ([section 5](first-version-specification.md#5-file-placement-and-application-data)).
- [ ] The nine Pass 1 tables, delivery outbox, transcript items, and writer lease ([section 6](first-version-specification.md#6-domain-model-and-durable-records)).
- [ ] Tolerant Codex adapter, last working version record, and revert ([sections 9 and 9a](first-version-specification.md#9a-engine-versions-and-reverting)).
- [ ] Codex permission presets and decision cards ([section 11](first-version-specification.md#11-permissions-and-user-questions)).
- [ ] Complete a task in a disposable Git repository and in a projectless chat through the real app: streamed output, one declined and one allowed action, Stop, saved draft, resume after restart, and preserved unrelated edits.
- [ ] Launch Bukno.app from Finder and retain the Codex evidence package.
- [ ] Sol reviews Pass 1.

## Pass 2: Claude on Mac

- [ ] Enumerate the Claude failure rows before writing the bridge.
- [ ] One packaged bridge for all Claude chats, with Rust-defined protocol types ([section 10](first-version-specification.md#10-claude-adapter-and-bridge)).
- [ ] Same disposable-repository and projectless flows through the installed Claude Code CLI, including configuration loading, approvals, questions, Stop, and resume.
- [ ] Measure bridge and engine memory; decide the bridge packaging runtime.
- [ ] Sol reviews Pass 2.

## Pass 3: Mac daily driver

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
