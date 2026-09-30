# Decisions

Recorded 30 September 2026 from the project conversation.

Project name: Bukno, pronounced “Buck-no”. License: Apache 2.0, superseding the earlier noncommercial proposal. Commercial use is allowed.

## Agreed

1. Build a lightweight native interface, with Rust as the implementation direction.
2. Use Codex app-server as the Codex agent backend and the initial voice/dictation backend.
3. Use Claude Agent SDK with the installed Claude Code executable as the second agent backend.
4. Share one composer, project list, conversation interface, and review workflow; retain provider-specific sessions and capabilities.
5. Dictation supplies an editable draft for either provider. Do not build a separate Claude voice service.
6. Plan for an open-source release and supported Sign in with ChatGPT. Public-release authentication terms remain a release check.
7. Measure the actual process family and the same workload before making memory-saving claims.

## Proposed, not yet selected

- Native UI toolkit: evaluate egui before committing.
- A small TypeScript bridge for the Claude Agent SDK; direct Rust JSON-RPC for Codex.
- Local metadata/search storage, lazy transcript loading, and optional viewers.
- Mac-first delivery followed by Windows/Linux, pending a platform decision.

## Open proof requirements

- Which authentication route works for text, realtime voice, and dictation?
- Which app-server mode provides draft-only dictation without executing work?
- What are the public-distribution requirements for Claude subscription login?
- Does the chosen native toolkit meet accessibility and text-input requirements?
- What memory budget is realistic with each engine and its tool children included?

The [feature map](feature-map.md) contains sources and the wider backlog. Source support is not end-to-end verification.
