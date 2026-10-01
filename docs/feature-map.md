# Bukno: feature map

Research draft, 30 September 2026. This is a proposed product scope and architecture, not an implemented application or a claim of complete desktop-app parity.

Build a lightweight, open-source native client for the installed Codex and Claude Code engines. Start with the everyday text coding workflow on Mac, then complete Windows before public release; add realtime voice and shared dictation through Codex app-server after the first usable release. Neither speech feature is a day-one requirement. Claude Code remains the second coding engine, controlled through its Agent SDK. There is no separate Claude voice service in scope. Keep expensive previews, audio models, and background agents inactive until needed. The [first version specification](first-version-specification.md) defines the implementation sequence and code boundaries.

Our existing audit measured roughly 5.2 GiB for the Codex desktop process family during its current workload. That is a useful motivation, but it is not a fair benchmark for a smaller replacement with fewer integrations. Performance comparisons must use the same workload and include all child processes.

## Architecture to start with

```mermaid
flowchart TD
    UI[Native Rust interface] --> Coordinator[Session and process coordinator]
    Coordinator --> Tasks[Bukno tasks and parent-child relationships]
    Coordinator --> Codex[Codex app-server]
    Coordinator --> Bridge[Small Claude SDK bridge]
    Bridge --> Claude[Installed Claude Code executable]
    Codex --> Dictation[Dictation transcript]
    Dictation --> UI
    UI --> Draft[Editable draft for either provider]
    Audio[Microphone and speaker transport] --> Codex
    Coordinator --> Store[Local metadata and search index]
    Coordinator --> Credentials[OS credential storage]
    Coordinator --> Git[Git and project tools]
    UI --> Optional[Optional previews and remote connections]
```

- Use a native Rust UI, with egui as the first candidate and no webview shell ([decision 13](decisions.md)). Prototype text selection, long transcripts, accessibility, code rendering, and IME support before committing. Rust alone does not guarantee a small memory footprint.
- Use a provider adapter boundary. Preserve provider-specific capabilities, session IDs, permissions, and errors instead of forcing them into misleading equivalents.
- Model Bukno tasks independently of provider sessions, with parent/child relationships, to support delegation between Codex/OpenAI and Claude. A shared agent-facing tool surface is proposed; see the [delegation design](implementation-plan.md#5-subagent-delegation-and-handoff).
- Let each engine own its conversation state. Store our project associations, labels, bookmarks, search index, drafts, and process ownership locally. Avoid keeping a second full transcript in memory.
- Project association is optional. Projectless chats are required and get persistent, separate working directories in a work folder the user chooses during onboarding; app state stays on the internal drive. See [projectless storage](implementation-plan.md#projectless-chats).
- Use the selected dark interface with blue Codex and orange Claude accents, simple text navigation with owning-provider logos, a model/effort/fast-mode control, directly accessible delegated chats, motion tied to real activity, compact change totals, and profile/usage controls. Ordinary chat formatting stays inline; previews open in the user's browser. See the [interface specification](implementation-plan.md#interface-direction-from-the-mockup-review).
- Start with local transport. Add remote hosts after local recovery and process lifecycle are dependable.
- Build the Mac daily driver first, with portable boundaries and early Windows build/UI checks. Native Windows integration and real-machine verification are required before public release; WSL and Linux are separate future scope decisions.
- Version-check engines at startup and negotiate available capabilities. Experimental voice needs a visible compatibility check.

Codex exposes structured application integration through [app-server](https://learn.chatgpt.com/docs/app-server). We should use that protocol rather than parse terminal rendering.

## Authentication and subscription boundaries

| Route | Intended use | What must be established |
|---|---|---|
| Sign in with ChatGPT for our client | Our own OAuth client for an eligible open-source/local application | Correct registration, browser login, credential storage, refresh, and successful inference |
| Existing Codex CLI authentication | Daily driver, through Bukno's own app-server child | Compatibility with our client and each requested capability |
| Existing Claude Code installation | All Claude use, following the T3 Code approach | Engine login, session resumption, and limits |
| Provider API keys | Optional alternative backend and API-only services | Separate billing and feature eligibility |

OpenAI documents [Sign in with ChatGPT for open-source applications](https://developers.openai.com/siwc/token-sharing-open-source) and an [app-server integration recipe](https://developers.openai.com/siwc/token-sharing-open-source/codex-app-server). Use our own honest client identity and token handling. It does not grant access to existing ChatGPT conversations. A model catalog entry is not proof of entitlement; complete an inference request.

The current [preview limitations](https://developers.openai.com/siwc/token-sharing-open-source/preview-limitations) distinguish local agent tools from hosted API tools. Hosted image generation, file search, Code Interpreter, native computer use, and hosted connectors are outside that route. Audio/video input and transcription are also excluded there. This does not establish the eligibility of Codex's separate experimental realtime transport; test that transport independently.

Bundling an engine and choosing a login route are separate decisions. The installed CLI and a bundled CLI expose the features of their respective versions; distribution alone does not grant speech access. The daily driver continues to use the installed CLI. The [speech follow-up](research/README.md#speech-follow-up) distinguishes the API-key WebSocket path, app-server WebRTC, and additional desktop-app speech clients.

Claude follows the T3 Code approach ([decision 12](decisions.md)): Bukno drives the user's own installed Claude Code CLI through the [Agent SDK](https://code.claude.com/docs/en/agent-sdk/overview), and users sign in through Claude Code itself. Bukno does not offer its own Claude login and never stores Claude credentials. Usage counts against the user's own plan as described in the [subscription-usage support article](https://support.claude.com/en/articles/15036540-use-the-claude-agent-sdk-with-your-claude-plan).

## App-server and SDK choices

Use Codex app-server directly from Rust over its structured local protocol. The [Codex SDK documentation](https://learn.chatgpt.com/docs/codex-sdk) explicitly recommends app-server for custom clients needing authentication, history, approvals, and streaming. Its Python SDK controls app-server; the inspected TypeScript SDK launches `codex exec` with JSON output. They are alternative integration layers, not two additional agent engines we need running beside app-server. We do not need Node or Python on the Codex path just to wrap a protocol Rust can speak.

The Claude path uses the TypeScript Claude Agent SDK plus the installed Claude CLI. The SDK manages an engine process; it is not a separate Claude agent or direct model replacement. Keep the bridge small and start it only when Claude is needed.

## What T3 tells us about Claude integration

The inspected [T3 Claude adapter](https://github.com/pingdotgg/t3code/blob/38969148a23e7a422045be023627c7fbbddf7503/apps/server/src/provider/Layers/ClaudeAdapter.ts) uses the TypeScript Claude Agent SDK to control the installed Claude executable. It supplies executable location, working directory, settings, permissions, streaming callbacks, and resume information. It handles tool approval, questions, planning, interruption, and session lifecycle.

Recommended first implementation: a small TypeScript SDK bridge controlled by the Rust coordinator. Measure its overhead with the real Claude process included. A completely Rust implementation using Claude's [structured CLI output](https://code.claude.com/docs/en/headless) is a possible later option, but needs equivalent approval, cancellation, recovery, and streaming behavior before replacing the SDK bridge.

The examined T3 revision is public main at `38969148a23e7a422045be023627c7fbbddf7503`; it is not a verification of the exact installed Nightly build. T3's [MIT license](https://github.com/pingdotgg/t3code/blob/38969148a23e7a422045be023627c7fbbddf7503/LICENSE) permits code reuse subject to its license obligations. This does not grant Anthropic authentication or service entitlements.

## Feature inventory

Everything in this inventory is a proposed product feature. Availability must be mapped per provider during implementation. “Core” identifies candidates for a usable first release, “Next” means valuable after that release, and “Later” means optional expansion. The [implementation plan](implementation-plan.md) recommends a narrower release boundary for agreement before implementation. This inventory is broader than a verified list of first-party Codex desktop capabilities.

| Area | Core | Next | Later |
|---|---|---|---|
| Projects | Open folder, recent projects, working directory, repository detection; projectless chats with persistent per-chat workspaces | Multiple roots, project profiles, project search, move chat to project | Shared project templates |
| Conversations | Start/resume, persistent text sidebar with owning-provider logos, stream responses, rename, archive | Search, pin, fork, unread state, filters, export | Read-only history import with explicit compatibility checks |
| Composer | Multiline input, drafts, keyboard shortcuts, file references, supported image input | Queued prompts, steering active work, reusable prompts | Rich input forms |
| Run control | Start, cancel, clear running state, errors, retry | Reconnect, detached-job visibility, per-run environment | Durable orchestration across providers |
| Provider settings | Compact provider-colored lightning/model/effort control without a provider logo, stepped supported-effort slider, explicit fast-mode state where available, permissions | Project defaults and further provider feature indicators | Additional providers |
| Authentication | Existing Codex/Claude engine login, engine status, explicit account handling | Own ChatGPT login for public release, account switching, expiry/relogin recovery | Enterprise configuration where supported |
| Usage | Cached profile-area limits where exposed, window/reset/age labels, event updates and conservative shared refresh | Token/cost details with source and billing route, budgets | Team reporting |
| Tools | Structured tool cards, collapsible output, command status | Search tool output, copy exact commands, tool timing | Custom visual tool interfaces |
| Approvals | Command/file approvals, permission scope, decline, user questions | Remember scoped choices where engine supports it, plan approval | Organization policies |
| Skills and instructions | Discover project instructions and configured skills, show loaded configuration | Skill browser, editing and validation, hooks visibility | Skill/plugin marketplace |
| MCP and plugins | Existing local MCP configuration, connection status, errors | Add/remove servers, auth flows, provider compatibility | Our own panel-extension API and event plugins |
| Git | Branch/status, compact line totals above composer, expandable file summary, open changed file | Embedded diff, worktree isolation, staging, commit, PR flow, review comments | Multiple concurrent branch workflows |
| Editing | Open in external editor, clickable paths and lines | Lightweight native text editor, patch review | Full IDE features only if needed |
| Artifacts | Inline Markdown, tables, code, compact file actions; full previews in the user's browser | Attachment organization and external viewer integrations | Embedded document/artifact viewers only after an explicit scope and memory review |
| Browser | Open ready previews and URLs in the default or selected external browser | Preview lifecycle refinements and external-browser integrations | Embedded browser or authenticated automation only after an explicit scope decision |
| Dictation | Not required for day one | Codex app-server flow, push-to-talk, editable draft for either provider, device selection, English/Swedish and code-identifier checks | Global shortcut and additional modes if needed |
| Codex voice | Not required for day one | Protocol integration proof, experimental voice mode, captions, interruption, voice selection, thread work visibility | Multi-thread voice navigation if exposed and verified |
| Claude speech input | Text composer | Use shared Codex-powered dictation, then send text to Claude | No separate Claude voice backend planned |
| Agents and delegation | Bukno task identity and parent/child model; show provider children when exposed | Codex/Claude delegation tools, task tree, open child chats and send follow-ups, revised-result return, cancellation and concurrency limits; delivery milestone to select | Deeper delegation and advanced team workflows |
| Automations | Completion notifications | Local schedules, recurring tasks, quiet meaningful-change notifications | MCP event triggers and optional always-on remote runner |
| Remote hosts | Design host identity into state from the beginning | Connect to a host's engine, host-local auth/tools, ownership transfer | Mac/Windows/Linux continuity and metadata sync |
| Recovery | Session resume after restart, owned-process cleanup, diagnostic export | Crash recovery, interrupted approval recovery, version rollback | Cross-host failover |
| Accessibility | Keyboard navigation, text selection, scaling, screen-reader prototype, reduced motion and labeled working states | Themes, command palette, accessibility validation | Cosmetic features and pets |
| Memory and CPU | Lazy transcript loading, virtual lists, bounded output, limited background work | On-demand viewers/audio, configurable agent cap, memory diagnostics | Optional low-resource operating profile |

Provider changes within one project should create or resume the selected provider's own session. “Continue with Claude” should send an explicit summary plus file references, not pretend the providers share hidden context. Delegation is a related but distinct flow: the parent retains ownership, creates a bounded child task on either provider, receives its result, and continues. A full handoff instead transfers execution ownership. Neither relies on shared hidden context.

Git recovery should preserve the user's unrelated edits. Checkpoints and undo must be scoped to the relevant change; a blanket reset is not an acceptable implementation.

Schedules require a running machine or an explicitly configured remote runner. Local scheduling does not automatically inherit Codex desktop automations or ChatGPT cloud scheduling.

## Voice: use the existing Codex route first

The user's correction is supported by the inspected source. Codex contains realtime conversation support with a `gpt-live-1-codex` model path and handoff machinery connecting voice to backend Codex work. It also has other version-dependent model paths, so do not hard-code a model based on a single source constant. See [realtime conversation implementation](https://github.com/openai/codex/blob/6996cde697b228fbf5695ce855b2498c5c668795/codex-rs/core/src/realtime_conversation.rs).

The installed `codex 0.158.0` generated experimental protocol schemas for `thread/realtime/start`, `appendAudio`, `appendText`, `appendSpeech`, `stop`, and `listVoices`. The [realtime protocol](https://github.com/openai/codex/blob/6996cde697b228fbf5695ce855b2498c5c668795/codex-rs/app-server-protocol/src/protocol/v2/realtime.rs) includes audio, transcripts, transport selection, version overrides, and handoff-related configuration.

We should implement native microphone capture and playback around that protocol and preserve the app-server's Codex work handoff. This is the preferred Codex voice route. Schema generation and source inspection prove an integration surface, not a successful authenticated voice session.

Follow-up inspection found both the standalone CLI (0.158.0) and desktop-bundled CLI (0.159.2) have public source that requires API-key authentication for default WebSocket realtime. App-server also has a WebRTC path using the configured provider authentication, which is a concrete candidate for a ChatGPT-login proof. The desktop app contains an additional client-owned call path and attaches it to app-server. `gpt-live-1-codex` is present in source, but a model name does not establish which accounts can start that call. See [source and local-build evidence](research/README.md#speech-follow-up).

Acceptance flow: sign in through the chosen route; start realtime; speak a request; hear a response; have Codex perform a small task in a disposable project; see its thread/tool activity; interrupt; end the call; resume the thread; verify microphone release and process/memory cleanup. Record protocol version and account route in the result.

Dictation should first be investigated through Codex app-server as requested: capture a transcript into an editable composer draft and submit that text to the selected engine only when the user sends it. Test actual English/Swedish speech and code identifiers. Verify no unintended coding turn, automatic submission, or end-of-call task. The schema exposes realtime transcripts but no separately named dictation RPC; public source also contains a transcription session mode. The inspected desktop app's dictation UI instead has its own streaming-transcription client and audio-upload fallback outside the app-server RPC. Do not assume bundling that engine reproduces desktop dictation. Select the exact supported mode and authentication through a dedicated proof.

Claude receives dictated text through our normal composer and runs through Claude Code. A separate Live API service, Claude conversational voice broker, system-dictation integration, and bundled local transcription model are outside the starting scope.

## DevDay: what can enter our interface

The [official 29 September roundup](https://learn.chatgpt.com/docs/whats-new/devday-2026) is the reference for yesterday's announcements. Access to an announced first-party feature does not automatically grant its backend to a standalone client.

| Announcement or capability | Product decision |
|---|---|
| Sign in with ChatGPT for open-source/local applications | Build our own supported login flow; validate the selected model and limits |
| GPT-6.1 Sol and faster Codex options | Populate the engine's available model/capability list; show eligibility rather than promise universal access |
| Reusable cloud environments | Consider an optional remote execution backend once its integration and access are established |
| Hosted browser computer use | Possible additional API-backed workflow; distinct from local Mac control |
| Plugin Extensions | Inspiration for native panels/forms/viewers; first-party extension hosting is not automatically reusable |
| MCP Events | Candidate for an event-driven automation service; draft/version compatibility and our scheduler are our responsibility |
| Website annotations | Useful future browser feedback flow; needs our own browser/session implementation |
| Space, Work sync, dots, Sites and team workflows | Separate hosted products; build local equivalents selectively or integrate only through documented access |
| Codex Security Cloud preview | Track as an optional service, not assume a general local feature or public API |

The [API changelog](https://developers.openai.com/api/docs/changelog) dates GPT-Live-1 availability to 10 September and transcription releases earlier. Voice is relevant to our design, but should not all be described as a new release yesterday. The new [hosted browser computer-use tool](https://developers.openai.com/api/docs/guides/agents-api/tools/computer-use) belongs to an API service route with its own requirements, not the limited open-source ChatGPT token route.

## Build sequence and release gates

1. **Mac compatibility and memory spike.** Tiny native window, Codex inference/resume/approval/cancel, then the Claude SDK bridge with the same flow. Use a disposable repository and the existing engine logins. Add early Windows compilation and native UI checks.
2. **Mac daily-driver text release.** Projects and projectless chats, conversations, inline results, motion/reduced motion, profile and cached usage, model/settings controls, structured tools, approval/questions, compact change summaries, restart recovery, MCP/skills visibility, notifications, and bounded memory behavior. Embedded diffs are not required.
3. **Windows and public release.** Complete native Windows integration and authenticated tests on a Windows machine. Verify the same core workflows, then finish public login, packaging, signing, and clean-machine setup on both platforms.
4. **Speech integration.** Establish the exact Codex app-server dictation and live-voice flows on both platforms, then add them to the interface. Neither blocks the first usable release. Keep live voice experimental while the protocol remains experimental.
5. **Power-user release.** Worktrees, full Git/PR workflow, transcript search, agent tree, previews, local schedules, remote hosts, accessible command palette, and voice refinement. Delegation can have a separate milestone after the core text flow and need not wait for speech.
6. **Optional services.** Hosted browser tools, remote always-on automation, our own extensions, collaborative metadata, and specialized artifact editing.

Before writing an adapter, enumerate its failure paths: expired login, incompatible version, missing executable, stream loss, duplicate execution after retry, declined approval, user cancellation, process crash, and restart with unfinished work. Prefer end-to-end checks through the actual engines, with a repeatable recording/log/result artifact.

For performance, benchmark the same repository and prompt sequence with one active task and with several resumed inactive chats. Measure idle, streaming, large tool output, voice active, voice stopped, and repeated open/close cycles. Include the GUI, sidecars, engine binaries, and tool children. Track process footprint and system pressure separately; summed RSS can misrepresent shared memory.

Initial engineering targets, not measured promises: native GUI idle below 100 MiB, and GUI plus a basic single Codex workload below 500 MiB. Measure Claude separately before setting a total target. Retained memory after repeated cycles should settle near a bounded baseline. Supporting twenty stored chats should not require twenty active engine sessions.

## Decisions still to make

- Which exact Codex authentication route completes text, realtime voice, and dictation without unintended task execution?
- Does the native UI prototype meet accessibility, selection, Markdown/code, and input-method requirements without an expensive browser layer?
- Which first-party integrations matter enough to reproduce? Documents, rich browser tooling, remote host discovery, and cloud collaboration can dominate effort and memory.
- How much existing local history can safely be displayed or indexed without relying on private schemas?
- Which milestone delivers cross-provider delegation, and what first limits apply to child depth, concurrency, permissions, and write ownership?
- Which macOS/Windows versions and CPU architectures will the first release support? Both operating systems are required; Linux is not yet committed.

## Evidence and limits

Saved [provider-capability-evidence.json](research/provider-capability-evidence.json) contains the local CLI version, inspected source revisions, and extracted realtime schema information. This research made no voice call, inference request, microphone recording, or authentication change. No replacement UI has been built or benchmarked. The feature inventory is our proposed backlog; protocol support and service access must be confirmed through the release gates above.
