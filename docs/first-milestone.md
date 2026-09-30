# First milestone: compatibility and memory

Goal: prove the two real agent backends and Codex speech flows before expanding the interface.

## Work sequence

- [ ] Enumerate adapter failure paths: unavailable executable, incompatible version, expired login, lost stream, rejected approval, cancellation, crash, and duplicate execution after retry.
- [ ] Establish installed-engine versions and generate matching app-server schemas.
- [ ] Prototype the native interface: selectable transcript, composer, code block, keyboard navigation, scaling, and accessible controls.
- [ ] Connect Rust directly to Codex app-server and complete a small disposable-repository task.
- [ ] Show streamed tools, approvals/questions, cancellation, and session resumption after restarting the client.
- [ ] Connect the Claude SDK bridge to the installed Claude CLI and complete the same flow.
- [ ] Complete a Codex realtime conversation with audible response and visible Codex thread work, then interrupt and stop it.
- [ ] Capture Codex dictation into an editable draft; send that draft to Claude through the normal composer.
- [ ] Verify dictation alone executes no coding task and stopping speech releases the microphone and owned audio resources.
- [ ] Measure idle, streaming, large tool output, speech active/stopped, and repeated session cycles with all descendants included.
- [ ] Produce a repeatable end-to-end evidence artifact and update the feature map with observed results.

## Acceptance

Both engines can edit a disposable repository, show a diff, handle permission decisions, and resume a saved session after restart. Cancellation does not leave an undisclosed running task. Voice works through the selected Codex auth route. Dictation can feed either composer without sending automatically. Memory remains bounded across repeated cycles.

Treat GUI idle below 100 MiB and a basic GUI-plus-Codex workload below 500 MiB as initial targets, not promises. Measure Claude before choosing its total budget. Compare the same workload with the existing desktop app.

Save human-reviewable results under `/Volumes/TOSHIBA Workspace/dev/artifacts/bukno/`. No implementation or tests were created as part of project setup.
