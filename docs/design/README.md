# Bukno design

First version, approved by Rasmus on 1 October 2026. This folder is the design reference for building the app. The repository copy is canonical. The Claude Design artifacts listed at the end are where the design can be edited; after any change there, copy the files back here and re-export the screens.

## Read this first

For Codex, Claude or anyone implementing the interface:

1. [system/README.md](system/README.md): the rules. Colour, type, layout, surfaces, states, motion, provider identity and app copy.
2. [system/tokens.json](system/tokens.json): exact values for colour, type, spacing, radius, shadow, sizes and motion. Load them into one theme struct in the app.
3. [journey/screens/](journey/screens/): one image per screen, in journey order (table below).
4. `system/components/<Name>/README.md`: what each component shows, what it needs, and when to use it. Props are listed in [system/components/index.d.ts](system/components/index.d.ts).
5. [07-claude-design-pass.md](07-claude-design-pass.md): what changed from the original mockup and why.
6. [08-working-animation-proposal.md](08-working-animation-proposal.md): proposed replacements for the working orb, with measured cost. Not approved yet.

## Things to keep in mind

- Bukno is a native Rust app (egui first, see the [decisions](../decisions.md)). The React code in `system/components/bundle.js` only exists to draw previews and screens. Rebuild each component natively; match the tokens, states and behaviour, not the React structure.
- The provider marks (a hexagon for Codex, a diamond for Claude) are placeholders. Use each provider's official mark, as their brand guidelines allow, before release.
- Model names, effort levels, level descriptions, engine versions, usage figures and chat content are examples. Real values come from the engines.
- Row 3 of the journey shows the planned delegation flow. Its delivery milestone is not selected yet.
- The fonts are Geist and Geist Mono (SIL Open Font License). They are not stored here; the app should bundle the official font files.

## Screens

| # | Screen | Image | Source |
|---|---|---|---|
| 1.1 | Check the engines on first run | [png](journey/screens/1-1-check-the-engines.png) | [Main.dc.html](journey/Main.dc.html) |
| 1.2 | Start a chat without a project | [png](journey/screens/1-2-start-a-chat-without-a-project.png) | [StartChat.dc.html](journey/StartChat.dc.html) |
| 1.3 | Model picker, effort first | [png](journey/screens/1-3-open-the-picker-effort-first.png) | [PickModel.dc.html](journey/PickModel.dc.html) |
| 1.4 | Model list: ChatGPT and Claude models | [png](journey/screens/1-4-choose-from-chatgpt-or-claude-models.png) | [ModelList.dc.html](journey/ModelList.dc.html) |
| 2.1 | Codex at work, with the working orb and To-do | [png](journey/screens/2-1-codex-at-work.png) | [CodexWorking.dc.html](journey/CodexWorking.dc.html) |
| 2.2 | Approve a command | [png](journey/screens/2-2-approve-a-command.png) | [Approval.dc.html](journey/Approval.dc.html) |
| 2.3 | Add a direction mid-run | [png](journey/screens/2-3-add-a-direction-mid-run.png) | [Steer.dc.html](journey/Steer.dc.html) |
| 3.1 | A delegated task needs you | [png](journey/screens/3-1-a-delegated-task-needs-you.png) | [TaskNeedsYou.dc.html](journey/TaskNeedsYou.dc.html) |
| 3.2 | Open the task and follow up | [png](journey/screens/3-2-open-the-task-and-follow-up.png) | [ChildTask.dc.html](journey/ChildTask.dc.html) |
| 3.3 | Back at the parent, revised result arrives | [png](journey/screens/3-3-back-at-the-parent-revised-result-arrives.png) | [RevisedResult.dc.html](journey/RevisedResult.dc.html) |
| 4.1 | Done, review the changes | [png](journey/screens/4-1-done-review-the-changes.png) | [ReviewChanges.dc.html](journey/ReviewChanges.dc.html) |
| 4.2 | Profile, accounts and usage | [png](journey/screens/4-2-profile-accounts-and-usage.png) | [ProfileUsage.dc.html](journey/ProfileUsage.dc.html) |
| 4.3 | Connection lost (Windows chrome) | [png](journey/screens/4-3-connection-lost-windows.png) | [ConnectionLost.dc.html](journey/ConnectionLost.dc.html) |
| 5.1 | Narrow window | [png](journey/screens/5-1-narrow-window.png) | [NarrowWindow.dc.html](journey/NarrowWindow.dc.html) |
| Part | Sidebar used by every screen | [png](journey/screens/part-sidebar.png) | [Sidebar.dc.html](journey/Sidebar.dc.html) |

The short story for each row is in [journey/canvas.json](journey/canvas.json) under `notes`, and on the viewer's index page.

## Folder contents

| Path | What it is |
|---|---|
| `system/README.md` | The design system's rules (brand book). |
| `system/tokens.json` | All tokens. Source of truth for values. `system/tokens.css` is generated from it for the previews. |
| `system/components/` | `bundle.js` and `bundle.css` (reference components), `index.d.ts` (props), and a `README.md` plus `preview.html` per component. |
| `system/index.html` | Live gallery of every component. |
| `system/design-system.json` | Claude Design's index for the system. Only needed when syncing with Claude Design. |
| `journey/screens/` | Exported images of every screen at 1440 by 900 (narrow window 1024 by 720). |
| `journey/*.dc.html`, `journey/canvas.json` | Claude Design source for each screen and the canvas layout. |
| `journey/viewer.html` | Local viewer that renders the screens live from the source files. |
| `06-warm-composer.png`, `reference-chatgpt-composer.png`, `06-revision-and-handoff.json` | The original mockup and reference this design started from. |

## Viewing locally

The pages fetch their files, so serve the folder over http:

```bash
python3 -m http.server 8000 --directory docs/design
```

Then open `http://localhost:8000/journey/viewer.html` for the journey (click a screen to open it live, including the clickable model picker) and `http://localhost:8000/system/index.html` for the components. Both need internet access for React (cdnjs) and the Geist fonts (Google Fonts).

To re-export one screen image with Chrome while the server runs:

```bash
"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" --headless=new --hide-scrollbars --force-prefers-reduced-motion --window-size=1440,900 --virtual-time-budget=5000 --screenshot=docs/design/journey/screens/2-1-codex-at-work.png "http://localhost:8000/journey/viewer.html?screen=CodexWorking"
```

Reduced motion makes the orb draw one still frame. If Chrome does not exit on its own once the image is written, stop it.

## Editing in Claude Design

- Design system: https://claude.ai/artifact/93MCTZhdKL3o2re4nKutNa
- User journey canvas: https://claude.ai/artifact/TM1CrTTg1QFeZNNEPYjqQM

Both are private to Rasmus's account until shared. The canvas keeps its own copy of the system under `ds/bukno/`, which this repository does not duplicate; the viewer loads `../system/` instead.
