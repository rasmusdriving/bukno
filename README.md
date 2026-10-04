# Bukno

A lightweight, open-source native interface for Codex and Claude Code. Pronounced “Buck-no”: build + knowledge. Pass 1 built: the Mac app runs real Codex chats, with or without a project, through your existing Codex login. Claude arrives in Pass 2.

Bukno is moving to T3's server as its backend while keeping the Rust interface
([decision 37](docs/decisions.md), [T3 backend plan](docs/t3-backend-audit.md)).
Stages 1 and 2 are built: Bukno pairs with a T3 server, shows its projects,
chats and models live, and works in them through T3. It starts Codex and
Claude chats, sends, answers approvals and questions, stops, queues and steers,
and picks up again after a restart. See [the read-only check](e2e/scenarios/t3-read-only.md)
and [the Stage 2 check](e2e/scenarios/t3-operate.md). The direct Codex path
still works unchanged. [Ubuntu setup](docs/ubuntu.md) covers the native app on Ubuntu.

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
cargo xtask e2e --provider codex --scenario pass1   # live: uses your Codex login and a little usage
cargo xtask package --platform macos
```

On the Ubuntu devbox, use the internal working-copy workflow and
`sh scripts/install-linux.sh` as described in [Ubuntu setup](docs/ubuntu.md).
The installed launcher opens a normal app; it does not start a synthetic demo.

A normal launch uses your app state in `~/Library/Application Support/Bukno/`
and the installed Codex. `--scenario <name>` starts the labeled synthetic mode
instead. See the [Pass 0 toolkit decision](docs/toolkit-decision.md) for what
was verified.

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

Codex text work runs end to end in the Mac app with the existing Codex login
(Codex 0.158.0, 2 October 2026). Ubuntu native launch, all 18 existing UI checks
and both live Codex scenarios passed on 3 October with Codex 0.160.0, including
approvals, Stop, restart/resume, draft persistence and engine cleanup. See
[Ubuntu evidence and limitations](docs/ubuntu.md). Claude, voice and dictation
have not been tested end to end, and Windows has only been compile-checked.
On 3 October the read-only T3 client passed its live end-to-end check against
the Ubuntu T3 server (pairing, lists, a 362-row chat, paged history, live
updates, network loss, restart, unknown message types, revocation). On 4 October
the Stage 2 check passed against the same server's newer build: Codex and
Claude chats started from Bukno, approvals allowed and denied, a question
answered, queue, steer, Stop and Resume, a Bukno restart during a run, lost
replies and an offline send, each message in T3 exactly once. Delegated child
chats are Stage 3 and are not opened separately yet. The plugin manager is proposed. Memory figures in the plan
remain targets or observations from the existing desktop app, not new Bukno
versus T3 benchmark results.

The first-version design system and user journey are done and saved in [docs/design](docs/design/README.md). The first version specification is approved. Pass 0 is merged and reviewed. Pass 1 (the complete Codex flow on Mac) is built and passed its live end-to-end run; it waits for Sol's review. See the [first milestone](docs/first-milestone.md). Keep source and manifests here; install tools and dependency directories on the internal drive as directed in [AGENTS.md](AGENTS.md).

## License

Copyright 2026 Rasmus Driving. Licensed under the [Apache License 2.0](LICENSE). Commercial use, modification, and redistribution are permitted under its terms. No noncommercial restriction applies. Contributions are accepted under the same license.

Third-party material retains its original licensing; see [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
