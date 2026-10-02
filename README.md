# Bukno

A lightweight, open-source native interface for Codex and Claude Code. Pronounced “Buck-no”: build + knowledge. Pass 0 foundation done and reviewed: the Mac shell, theme and transcript widget run with synthetic data only. No engine is connected yet; Pass 1 connects Codex.

## Agreed architecture

- Starts as the author's daily driver, published as open source for others to use.
- Native Rust interface and process/session coordinator. No webview shell, to keep memory low; egui is tried first.
- Mac first for the daily driver, with shared portable code and early Windows build/UI checks. Complete native Windows integration and real-machine testing before public release.
- Codex app-server for Codex agents and the preferred starting integration for later speech. Bukno runs its own app-server child with the existing Codex login; the exact voice/dictation transport and authentication still need proof.
- Claude Agent SDK controlling the installed Claude Code CLI, the same approach as T3 Code. Users sign in through Claude Code itself.
- Dictation becomes an editable draft that can be sent to either provider.
- The first usable release can ship with text workflows; dictation and live voice follow through Codex app-server.
- Design Bukno-owned tasks and parent/child relationships from the start so either provider can delegate to Codex/OpenAI or Claude workers. The delegation mechanism and delivery milestone remain to be proven and selected.
- No separate Claude voice service or additional Codex SDK runtime in the starting scope.
- Projectless chats are required alongside project chats, with a persistent workspace per chat in a work folder chosen during onboarding. App state stays on the internal drive.
- Bukno works with any installed engine version and offers a one-click switch back to the last working version if an update breaks it.
- Dark interface with blue Codex and orange Claude accents, simple text navigation with provider logos, a model/effort/fast-mode control, interactive delegated chats, animated working states, compact change totals, and cached usage beside the profile. Ordinary chat formatting stays inline; previews open in the user's browser. No embedded browser or diff viewer is required for version one.
- Interface feel is a first-release requirement: warm grey surfaces, clear typography, restrained shadows, and responsive controls. Keep the compact lightning/model/effort control free of a provider logo, move the composer and profile close to the bottom edge, and avoid ornamental borders or an embedded preview panel.

## Building

```bash
sh scripts/bootstrap.sh          # puts compiler output on the internal drive
cargo xtask doctor               # reports tools and locations
cargo xtask dev --scenario long-chat
cargo xtask check                # format, lint, Mac build, Windows compile check
cargo xtask e2e --provider synthetic --scenario pass0-ui
cargo xtask package --platform macos
```

Every launch is the labeled synthetic scenario mode until Pass 1. See the
[Pass 0 toolkit decision](docs/toolkit-decision.md) for what was verified.

## Project documents

- [Feature map](docs/feature-map.md): architecture, full backlog, DevDay options, authentication boundaries, and performance goals.
- [Decisions](docs/decisions.md): agreed choices and unresolved questions.
- [First milestone](docs/first-milestone.md): the pass-by-pass checklist to the Mac daily driver and Windows release.
- [Implementation plan](docs/implementation-plan.md): recommended release boundary, decisions to resolve, cross-platform requirements, and staged delivery.
- [First version implementation specification](docs/first-version-specification.md): the approved build plan and the single source for code scaffold, architecture, data and provider contracts, implementation passes, and end-to-end acceptance criteria.
- [Research evidence](docs/research/provider-capability-evidence.json): installed CLI version, pinned source revisions, and experimental realtime schemas.
- [Research provenance](docs/research/README.md): snapshot location and verification limits.
- [Interface specification](docs/implementation-plan.md#interface-direction-from-the-mockup-review): selected mockup direction, motion, inline rendering, and usage refresh.
- [Design](docs/design/README.md): the approved first-version design system (rules, tokens, components) and the user journey (15 screens with images and a local viewer). Start here before building any interface.
- [Original mockup](docs/design/06-warm-composer.png) and [composer reference](docs/design/reference-chatgpt-composer.png): the inputs the design system was built from.

## Current status

Planning and source/schema research are complete enough to start the compatibility milestone. Authentication, inference, voice, dictation, and a replacement UI have not been tested end to end. Memory figures in the plan are targets or observations from the existing desktop app, not replacement-app results.

The first-version design system and user journey are done and saved in [docs/design](docs/design/README.md). The first version specification is approved. Pass 0 is merged and reviewed. Next is Pass 1 of the [first milestone](docs/first-milestone.md): the complete Codex flow on Mac. Keep source and manifests here; install tools and dependency directories on the internal drive as directed in [AGENTS.md](AGENTS.md).

## License

Copyright 2026 Rasmus Driving. Licensed under the [Apache License 2.0](LICENSE). Commercial use, modification, and redistribution are permitted under its terms. No noncommercial restriction applies. Contributions are accepted under the same license.

Third-party material retains its original licensing; see [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
