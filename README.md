# Native Agent Client

A lightweight, open-source native interface for Codex and Claude Code. Working name; planning stage, no application implementation yet.

## Agreed architecture

- Rust interface and process/session coordinator.
- Codex app-server for Codex agents, realtime voice, and shared dictation.
- Claude Agent SDK controlling the installed Claude Code CLI.
- Dictation becomes an editable draft that can be sent to either provider.
- No separate Claude voice service or additional Codex SDK runtime in the starting scope.

## Project documents

- [Feature map](docs/feature-map.md): architecture, full backlog, DevDay options, authentication boundaries, and performance goals.
- [Decisions](docs/decisions.md): agreed choices and unresolved questions.
- [First milestone](docs/first-milestone.md): end-to-end compatibility and memory checks before the full interface.
- [Research evidence](docs/research/provider-capability-evidence.json): installed CLI version, pinned source revisions, and experimental realtime schemas.
- [Research provenance](docs/research/README.md): snapshot location and verification limits.

## Current status

Planning and source/schema research are complete enough to start the compatibility milestone. Authentication, inference, voice, dictation, and a replacement UI have not been tested end to end. Memory figures in the plan are targets or observations from the existing desktop app, not replacement-app results.

Development starts with the [first milestone](docs/first-milestone.md). Keep source and manifests here; install tools and dependency directories on the internal drive as directed in [AGENTS.md](AGENTS.md).
