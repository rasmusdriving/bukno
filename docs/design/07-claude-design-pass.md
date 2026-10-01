# Claude design pass

Approved by Rasmus as the first version on 1 October 2026 ([decision 29](../decisions.md)). Local copies of everything live in this folder; start at [README.md](README.md). This file records what changed from the original mockup and why.

- Design system: [Bukno](https://claude.ai/artifact/93MCTZhdKL3o2re4nKutNa) (Claude Design). Tokens, brand book, motion rules and 35 reference components.
- User journey: [Bukno User Journey](https://claude.ai/artifact/TM1CrTTg1QFeZNNEPYjqQM) (Claude Design canvas). 14 screens built from the design system, plus the shared sidebar part.

Both links are private until shared from each page's Share menu.

## Kept from 06-warm-composer

- Three columns, warm charcoal surfaces, blue only for Codex and orange only for Claude.
- Text sidebar rows with the owning provider's mark on the right and a soft selected fill.
- Delegated work: assignment first, then provider and state, then one activity line.
- Composer shape and toolbar grouping: add and permission on the left; lightning, model, effort and chevron on the right, then Stop.
- Change counts directly above the composer, usage and profile at the bottom left, no embedded preview.

## Changed

1. One moving indicator per run. The mockup showed four spinners at once. Now the ring spins in the chat header and the sidebar row only; the active plan step is a still arc; running child tasks breathe gently.
2. The sidebar logo slot also shows state: ring while working, a neutral attention disc when a chat needs you, a dot for a new result. Marks stay muted until selected. Rows are 32 points high so more chats fit.
3. "Needs you" is neutral (a light disc plus words), because amber or yellow would read as Claude.
4. Line counts use green and red with their signs. Changes belong to the workspace, not to a provider.
5. The separate top bar is gone. The titlebar holds the macOS traffic lights or Windows caption buttons, with project and branch at the left of the canvas. Search moved into the sidebar.
6. Chat title is 20 points instead of about 28, and the user bubble has no "You" label.
7. Completed steps use a quiet grey disc so the active step leads.
8. Delegated work only shows when the chat has tasks, and folds into a header button and sheet in narrow windows.
9. Approvals dock above the composer so they never scroll away.
10. The primary action is Stop while running with an empty draft, and Send plus a small Stop once a follow-up is typed.
11. The lightning is filled when fast mode is on and outlined when off. Its brightness follows the effort level.
12. Type is Geist and Geist Mono (SIL Open Font License), bundled so both platforms match.

## Revision 2 (1 October 2026)

From Rasmus's review of the first draft:

- **Chat is only the conversation.** Plans moved out of the transcript. While you watch a run, the chat ends with a working indicator instead of a spinner: a flowing pulse line in the provider colour (gentle while thinking, faster with a spike while a tool runs, flat while waiting for you), shimmering activity text, the latest reasoning summary fading in word by word, and recent events as chips. The header chip stays still so this is the one moving thing in the chat.
- **To-do moved to the right panel**, below Delegated work. It shows the plan of whichever agent is open in the main pane, with a done-of-total count and a thin progress bar. The panel now shows whenever there is a plan or delegated work.
- **New model picker.** The model control opens on reasoning effort: the current model on top, a thick segmented effort slider that steps from dim to bright, a description of the level, then fast mode. Clicking the model name opens the model list with two sections, ChatGPT models (runs in Codex) and Claude models (runs in Claude Code). Screens 1.3 and 1.4 are clickable in the canvas's Play mode.

Item 1 and item 8 above are superseded by this revision.

## Revision 3 (1 October 2026)

The pulse line, shimmer, word-by-word fade and event chips were too much. The working indicator is now a small dotted orb (`ThinkingOrb`, 32 points in the chat) beside one line of activity and an optional reasoning summary. Only the orb moves:

- Warm neutral dots on a slowly turning sphere (about 20 seconds a turn); near dots are larger and brighter.
- The provider colour shows only where the work is: a drifting band of light while thinking, a sweep while reading, two small orbiting points while a tool runs. Waiting stops the sphere and removes the colour.
- Drawn on a plain 2D canvas, pauses when off screen or hidden, still frame with reduced motion.

Inspired by [Thinking Orbs](https://github.com/Jakubantalik/thinking-orbs) (MIT) for the dotted, depth-shaded sphere idea. No code was copied; the drawing, states and colour use are Bukno's own.

## Added to the design system for the journey

Setup cards for each engine, the delegated task brief, inline event lines (result arrived, revision 2), notices for reconnecting and unknown outcomes, and usage reset times in the profile menu.

## Open questions

- Is Geist the right typeface, or should Bukno use the platform fonts?
- Provider marks are placeholders. Confirm using each provider's official mark within their brand guidelines.
- Is removing the wordmark from the window chrome acceptable? It remains on first-run setup and on Windows.
- Row density: 32 points, or closer to the mockup's roomier rows?

## Not verified

- Nothing has been built in the native renderer. Spacing, shadows, motion cost and focus behaviour still need checking in egui on macOS and Windows.
- Contrast was checked by calculation against the token values, not with assistive technology.
- Model names, effort levels, versions, usage figures and task content are illustrative.
- Motion is shown as CSS approximations in the previews.
