# Bukno instructions

Follow the parent Toshiba workspace instructions and the user's current task instructions. This repository is planning-stage; do not treat proposed features as implemented capabilities.

## Scope

Read README.md, docs/decisions.md, and docs/first-milestone.md before implementation. Codex app-server provides Codex agents and initial voice/dictation. Claude Agent SDK controls the installed Claude Code CLI. Do not add a separate Claude voice backend without a new scope decision.

## Placement

Keep source, scripts, manifests, lockfiles, and project configuration in this repository on Toshiba. Install CLI tools, SDKs, runtimes, caches, and dependency directories in normal user-level locations on the internal drive; configure external dependency/build directories or documented symlinks where supported. The user's machine instructions take precedence over parent cache-placement defaults.

Use `/Volumes/TOSHIBA Workspace/dev/artifacts/bukno/` for benchmark outputs and end-to-end evidence. Keep credentials and user transcripts out of Git.

## Quality and verification

Put substantial app changes in a pull request for review.

Use concise, plain language and no em dashes in documents or app copy. Distinguish source/schema observations from successful authenticated flows. Measure the actual bottleneck and compare the same workload before/after. Include owned engine and tool children in memory figures.

Do not add unit tests as a default implementation follow-up. Enumerate adapter failure paths before writing isolated tests or adapter implementation. Prefer real end-to-end provider flows and retain repeatable evidence. Preserve unrelated user edits; do not implement blanket Git resets as undo.
