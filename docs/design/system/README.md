Bukno is a native desktop client for two strong agent harnesses: Codex (through `codex app-server`) and Claude Code (through the Claude Agent SDK). The engines do the work. Bukno's job is to make that work easy to follow, easy to steer and calm to look at. This system covers the dark interface for version one on macOS and Windows. It is written for a native Rust renderer; the React components here are reference implementations for previews and screen stories, not the app's code.

## Principles

- **Quiet chrome, clear state.** Surfaces are warm charcoal and almost flat. The only things that stand out are what is running, what needs you, and what changed.
- **Provider colour is identity, never decoration.** Blue means Codex and orange means Claude, wherever they appear. They never tint a surface, a border or a neutral control.
- **One live thing per region.** While you watch a run, the working indicator at the end of the chat is the one moving element there. Everything else states its status in words and still glyphs.
- **Chat is for the conversation.** Messages and replies go in the chat. The agent's to-do list and delegated tasks live in the right panel.
- **Show what the engine reports.** Plans, tool activity and reasoning summaries come from provider events. Never invent progress, percentages or narration to keep the screen busy.
- **Group with space, not lines.** Hierarchy comes from spacing, weight, text colour and surface tone. Borders are not a default.

## Content fundamentals

Write like a calm colleague reporting on work. Short, plain sentences in active voice. Sentence case everywhere, including buttons and section labels. No em dashes in app copy, no exclamation marks, no emoji.

- Name things the way people think about them: **chat**, **project**, **task**, **Delegated work**, **changes**. Not "thread", "session", "run ID" or "workspace config".
- Say who acts. "Codex wants to run a command", "Claude is checking the keyboard flow", "Your follow-up was sent to Claude".
- Controls say exactly what happens: **Stop**, **Allow once**, **Allow for this chat**, **Deny**, **Check again**, **Back to Polish the composer**.
- Placeholders name the recipient, so it is always clear who reads the message: "Ask Codex to do something", "Follow up with Claude on this task", "Add a direction while work continues…".
- Errors say what happened and what to do next, without apology: "Lost connection to Codex. Checking what finished before sending anything again."
- Never claim what is not known. "Outcome unknown" beats "Failed" when the stream dropped. "Usage unavailable" beats "0% left".
- Times and counts are short and tabular: `2m 14s`, `+100 −5`, `68% left`, `14 min ago`.

## Colour

Use the surface ladder for structure and nothing else:

| Token | Where |
|---|---|
| `surface-sidebar` | Left navigation. |
| `surface-panel` | Delegated work panel. |
| `surface-canvas` | The conversation and titlebar. Lightest ground, so the centre leads. |
| `surface-hover` | Hover on flat rows; the quiet card behind a notice. |
| `surface-selected` | The open chat or task. A fill only. |
| `surface-composer` | Composer, user bubble, approval card, setup cards. |
| `surface-raised` | Controls that sit on the composer, meter tracks, the avatar. |
| `surface-popover` | Menus, model picker, change list. Always with `shadow-popover`. |
| `surface-code` | Code and commands inside the transcript. |

- Set body copy and labels in `text-primary`, meta and activity in `text-secondary`, placeholders and section labels in `text-tertiary`. `text-tertiary` is not legible enough on `surface-raised` or `surface-popover`; use `text-secondary` there.
- `codex` and `claude` colour the provider name, the working ring, the usage fill and the selected-model check. The owning-provider mark in the sidebar stays `text-tertiary` until its row is selected.
- The lightning in the model control uses the effort ramp (`codex-effort-1` to `codex-effort-5`, `claude-effort-1` to `claude-effort-5`): dim at low effort, bright at high. Map the engine's levels onto the five steps evenly; never add levels the engine does not expose.
- `positive` and `negative` are for line counts and errors only, and always carry a sign, an icon or a word.
- **Needs you** is neutral: the `attention` glyph (a filled `text-primary` disc with an `on-attention` mark) plus the words "Needs approval" or "Has a question". It must never look like a provider.
- One primary action per view uses `action` with `on-action`: Send, Allow once, Continue.
- `divider` is only for separating sections inside a popover. Columns, panels, the composer and buttons have no outlines.

## Typography

One family, Geist, with Geist Mono for code. Bundle both (SIL Open Font License) so macOS and Windows render identically. Use three weights only: 400, 500, 600.

- `t-title` for the open chat's title. `t-display` only for the empty new-chat greeting and first-run setup.
- `t-body` for the transcript, `t-ui` for rows and controls, `t-ui-strong` for task titles, provider labels and buttons.
- `t-small` for meta lines and the change strip, `t-caption` for section labels and usage labels.
- `t-code` for code and commands, `t-mono-small` for file paths and engine versions.
- Turn on tabular figures for anything that updates live: line counts, elapsed time, percentages.
- Swedish and other non-ASCII text must render in every style; test å, ä and ö in titles and paths.

## Layout

The window is three columns plus a titlebar.

- `size-titlebar` (44) holds the macOS traffic lights over the sidebar, or the Windows caption buttons at the far right. The canvas part of the titlebar shows project and branch, and is a drag region.
- `size-sidebar` (264): New chat, Search, Projects, Chats, then usage meters and the profile row pinned to the bottom.
- The conversation column is centred in the canvas at up to `size-column` (760). The transcript and the composer share this width.
- `size-panel` (336): the right panel, with **Delegated work** on top and **To-do** below it. To-do shows the plan of the agent in the main pane: the parent, or a delegated task once it is opened. Show the panel when the chat has a plan or delegated tasks, or when the person opens it; a section with nothing in it is left out. When the window narrows, the panel collapses first into a header button that opens a sheet over `scrim`; the sidebar collapses second.
- The composer and the profile row sit `size-inset-bottom` (12) above the window's lower edge. The change strip sits directly above the composer.
- Rows are `size-row` (32). Toolbar controls are `size-control` (32).
- Use `space-2` between an icon and its label, 6px between the parts of the model control, `space-4` inside the composer, `space-6` between turns.

## Surfaces, elevation and shape

- Lists and panels are flat. Only things that float or take input get a shadow: `shadow-composer` for the composer and approval card, `shadow-popover` for menus and pickers, `shadow-raised` for Stop and secondary buttons on the composer.
- Do not substitute a thin shadow ring for every removed border, and do not put every item in a card.
- Radii by role: `radius-xl` for the composer and approval card, `radius-lg` for popovers, code, notices and task rows, `radius-md` for rows and buttons, `radius-bubble` for the user message, `radius-full` for the avatar, send button and meters.

## States

| State | Treatment |
|---|---|
| Hover | `surface-hover` on flat rows; one step lighter on raised controls. |
| Selected | `surface-selected` fill. No side bar, no bold, no colour. |
| Pressed | `surface-selected` on rows; `dur-press` feedback. |
| Keyboard focus | `focus-ring` on every focusable element, following its radius. Focus and selection must always look different. |
| Disabled | `text-disabled` plus a reason in words nearby. |
| Needs you | `attention` glyph, "Needs approval", and a `surface-hover` card in Delegated work. |
| Unread result | Title at 500 weight and a small `text-primary` dot before the mark. |
| Unavailable | `text-tertiary` title and an alert icon; the chat keeps its identity. |

## Motion

Motion makes real work legible. It is short, interruptible and tied to events.

| Moment | Treatment |
|---|---|
| Run is active, chat in view | The `WorkingIndicator` closes the transcript with a `ThinkingOrb`: a small dotted sphere in warm neutral that turns slowly. The provider colour shows only where the work is: a drifting band of light while thinking, a sweep while reading, two small orbiting points while a tool runs. Text beside it fades in once when it changes and never shimmers. The header chip stays still. |
| Run is active elsewhere | The ring spins once per `loop-working` on the chat's sidebar row. The plan step in progress shows a still arc. |
| Child task running | Its dot breathes once per `loop-breathe` in Delegated work. |
| New provider event | The activity line crossfades over `dur-crossfade`; layout does not move. |
| Step completes | Settles into the done disc over `dur-settle`, once, then stays still. |
| Waiting for you | Steady attention glyph, and the orb stops and loses its colour. No pulsing. |
| Error, stopped or disconnected | The working loop stops. Reconnecting uses a slow dashed ring. |
| Disclosure | Panels, popovers and the change list open over `dur-standard` with `ease-standard` and close with `ease-exit`. |
| Highest effort | A highlight sweeps the thick slider and a soft glow breathes around it once per `loop-shimmer`, only while the picker is open. |

With reduced motion, every loop stops and the same glyphs and words remain. Stop repainting when the window is hidden, minimised or idle.

## Provider identity

- The chat's owning provider is shown by its mark on the right of the sidebar row and by the coloured provider name on each reply and task.
- The composer never shows a provider logo. The lightning colour, the model name and the placeholder carry the recipient.
- A new chat can switch provider freely until the first message. After that, switching provider is an explicit handoff, never a silent reassignment.
- The model control opens the picker on effort first: the current model on top, a thick segmented effort slider, then fast mode. Clicking the model row opens the model list, in two sections: **ChatGPT models** (runs in Codex) and **Claude models** (runs in Claude Code).
- Effort and fast mode reflect what the adapter actually applies. When a setting is pending or unavailable, say so.
- The two engines protect differently. Say "Runs in the Codex sandbox" or "Approvals only, no OS sandbox" rather than implying equal protection.

## Iconography

Use the `Icon` component: 16px grid, 1.5px round stroke, drawn in `currentColor`, sized 14 or 16. Icons sit in `text-secondary` or `text-tertiary` unless they carry state. No emoji anywhere.

The provider marks in `ProviderMark` are **placeholders** (a hexagon for Codex, a diamond for Claude). Replace them with each provider's official mark, used as their brand guidelines allow, before release. Keep the same 14 to 20px sizes and the accessible names "Codex" and "Claude".

## Components

- **Identity and state:** `ProviderMark`, `StatusGlyph`, `StateChip`, `Icon`.
- **Actions:** `Button`, `IconButton`, `Kbd`, `Switch`, `Menu`.
- **Navigation:** `SectionLabel`, `ProjectRow`, `ChatRow`, `UsageMeter`, `ProfileRow`, `Breadcrumb`.
- **Conversation:** `UserMessage`, `AgentTurn`, `PlanStep`, `ActivityLine`, `CodeBlock`, `Notice`, `TaskBrief`, `EventLine`.
- **Composer:** `Composer`, `PermissionControl`, `ModelControl`, `ModelPicker`, `EffortSlider`, `ChangeStrip`, `ApprovalCard`.
- **Working and planning:** `ThinkingOrb`, `WorkingIndicator`, `TodoList`.
- **Delegation and setup:** `TaskRow`, `EngineCard`.

Each component's guidelines say what the consumer provides. Screens and layout are the consumer's; the components only draw themselves.

## Native implementation notes

- Treat every px value as a logical point. Check each size at 100%, 150% and 200% scaling on Windows and on Retina displays.
- Load the tokens into one theme struct at startup; do not scatter literals through widgets.
- Shadows are drawn on the few elevated frames only. Measure their cost; nothing animates when idle.
- Keep the focus ring drawable on every interactive widget, and expose names and states through the platform accessibility API.
