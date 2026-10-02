# Pass 0 toolkit decision: keep egui

Written 1 October 2026 at the end of Pass 0, for Rasmus and for Sol's review.
Covers the gate in [specification section 23](first-version-specification.md#23-decisions-and-gates-before-declaring-completion):
"egui first; retain only if text, input and accessibility pass".

## Decision

**Provisional:** keep egui 0.36 with eframe, wgpu and AccessKit for the Mac
daily driver. No must-have failed in the automatic checks, so the GPUI
comparison from decision 13 was not started. The decision stays provisional
until these are recorded, as the PR 1 review asked:

- VoiceOver in the packaged app (message boundaries and selected text read aloud).
- A real input method (dead keys, and Japanese or Chinese composition).
- The real Reduce motion system setting.
- The early Windows smoke check: typing, selection, scaling and Narrator.

The AccessKit and injected-event checks below do not close those gates. They
are pending, not failed. The steps are in
`e2e/scenarios/pass0-native-checks.md`.

The investigation used well under the two-day time box. The toolkit was not
patched; the one workaround (accessibility parent registration) uses egui's
public API.

## Versions

Rust 1.99.0 (pinned in `rust-toolchain.toml`), egui, eframe, egui-wgpu and
egui_kittest 0.36.2, AccessKit 0.24 (via egui), winit 0.30.13, wgpu 30.0.1,
pulldown-cmark 0.13.4, Tokio 1.53. Exact versions are in `Cargo.lock`.

## Must-haves

| Requirement | Result | Evidence |
|---|---|---|
| Select from mid message 3 to mid message 40 with the mouse while a reply streams, scroll away and back, copy and paste exact text | Pass (kittest, real app) | `pass0-ui/checks/transcript-mouse` |
| The same with the keyboard only | Pass (kittest, real app) | `pass0-ui/checks/transcript-keyboard` |
| Selection survives the start block leaving the layout | Pass. Selection is stored as block ID plus character offset, independent of layout | both checks above |
| 2,000-message chat within the frame target (p95 under 33 ms) | Pass. Packaged app, 2,002 messages, reply streaming, a new screen laid out every frame: frame interval p95 7.3 ms on a 144 Hz display, UI-thread work p95 0.84 to 0.96 ms, maximum 2.1 ms (three samples) | `bench-scroll` |
| Accessibility tree with message boundaries and the selection | Pass at the AccessKit level. Document node "Conversation", one article per message ("Codex, message 41 of 2000"), paragraph, heading, list item and code nodes with text runs. The selected text AccessKit reports equals the visible part of the copied text | `transcript-accessibility`, `transcript-mouse` |
| VoiceOver reads message boundaries and the selected text | **Not tested.** Needs a person; see `e2e/scenarios/pass0-native-checks.md` | none yet |
| IME composition and Swedish text | Pass with injected IME events: Enter during composition does not send, the commit lands, Shift+Enter is a newline, "Räksmörgås på Åre, ÄÖ åäö ÅÄÖ" is exact | `composer-ime` |
| A real input method (dead keys, Japanese or Chinese) | **Not tested.** Needs a person | none yet |
| Keyboard navigation and visible focus | Pass. Tab reaches every sidebar row, the profile, the sidebar toggle, the transcript, Copy code and the composer, and every stop has a name; focus rings show after Tab and not after a click; Cmd+N opens a new chat with the composer focused | `shell-keyboard` |
| No repaint when idle | Pass. Idle window requests no repaint (kittest). Packaged app idle for 30 s: 0 to 41 frames per sample (the non-zero samples coincide with pointer activity on the desktop), 0.03 to 0.42 % of one core | `repaint-policy`, `bench-idle` |
| Orb capped near 20 fps | Pass after a fix: 19.5 fps measured. egui shortens scheduled repaints by its predicted frame time, which first gave 29 fps | `bench-orb` |
| Reduced motion | Pass (kittest): with reduced motion the orb asks only for the one-second clock tick. Not yet checked with the real system setting | `repaint-policy-reduced` |
| Narrow window | Pass: at 1024 by 720 the sidebar stays and the column narrows; below 824 points the sidebar collapses | `reference_screens` |
| Design fidelity | Pass with listed deviations. Real colours, Geist and Geist Mono, spacing and shapes; native screenshots match the reference layout closely | `pass0-screens/compare-*.png` |
| Windows | Compiles for Windows x64 with no warnings (`cargo xtask check`). **The Windows UI smoke check has not run**; Windows UI support is unverified | none yet |

## Measurements

All on the development Mac (Apple Silicon, macOS 27.0, 1x external display
at 144 Hz), release build of the packaged app, physical footprint from
`/usr/bin/footprint`. Pass 0 has no engine processes, so the app process is
the whole owned process family. One warm-up, then three samples.

| Workload | Result |
|---|---|
| Idle, long chat open, nothing running | 46 to 49 MB at rest; 0.03 to 0.42 % CPU |
| Rendering frames (any workload) | about 142 to 160 MB while frames are being drawn; about 95 MB of it is GPU memory that macOS reclaims a few seconds after frames stop |
| After scrolling through all 2,002 messages, then idle | 62 MB |
| Orb only | 19.5 fps, 4.4 to 4.9 % of one core |
| Launch, load 2,000 messages, 0.5 s settle, screenshot, exit | 0.9 to 1.0 s wall time per run. A separate launch-to-interactive time was not measured |

The provisional idle target (below 100 MiB) is met at rest. Memory while
rendering is above it; that GPU share is a Pass 3 investigation item, not a
Pass 0 blocker.

### Fixed during Pass 0, measured before and after

- Transcript memory: every laid-out block stayed cached, so after one sweep
  through the chat the app held 226 MB. The cache now keeps at most 400
  galleys near the viewport and remembers measured heights. Same workload
  afterwards: 160 MB while drawing, 62 MB once idle. Frame times unchanged.
- Orb frame rate: 29 fps before, 19.5 fps after.

## Orb CPU budget (resolved by decision 35)

Proposed target: under 3 % of one core with only the orb animating. Measured
in the packaged app (two samples each):

| Orb rate | CPU, one core |
|---|---|
| 20 fps | 5.0 % |
| 15 fps | 4.0 % |
| 12 fps | 3.2 % |

The cost is close to linear in frames: about 2.5 ms of CPU per frame across
all threads. A profile at 20 fps shows about 0.8 ms on the main thread (egui
pass 0.23 ms, wgpu submit 0.37 ms, tessellation 0.1 ms) and the rest in Metal
and Core Animation presenting the frame. egui redraws the whole window for
any animation (section 13 expects this), so lowering per-frame cost further
means drawing less, not tuning the orb.

Options for Rasmus: accept about 5 % at 20 fps; pick a lower rate after
looking at it (`BUKNO_ORB_FPS=12` with `--scenario working`); or ask for a
Pass 3 experiment with a separately composited native layer for the orb.

Follow-up the same day: two cheaper treatments are proposed in
[design/08-working-animation-proposal.md](design/08-working-animation-proposal.md).
Measured the same way, the stepped BuildGrid uses 1.7 to 2.1 % and the
StreakLabel at 12 fps uses 3.1 to 3.4 %. Rasmus chose the StreakLabel at
10 fps (decision 35): 2.53 to 2.58 % measured, under the budget. It is now
the default; `BUKNO_WORKING_MARK=orb` or `grid` shows the others.

## Deviations from the design, recorded rather than hidden

- **Text selection colour**: not in `tokens.json`. Uses `focus` at 28 %
  opacity. Needs a token.
- **Tabular figures**: egui's text engine has no OpenType feature support,
  so `tnum` cannot be turned on. Geist's default figures are used.
- **Inset highlight on the composer shadow**: drawn as a one-point line.
  Outer shadow layers use egui's blur.
- **Traffic lights**: moved into the 44-point titlebar by resizing AppKit's
  titlebar container and placing the buttons (measured at x 24, 44, 64,
  centred 22 points down, as in the reference). This relies on AppKit's view
  hierarchy and may need attention after a macOS update.
- **Not built in Pass 0**: right panel (Delegated work, To-do), change strip,
  model picker, permission presets, project add button, branch in the
  breadcrumb. The model and permission labels are display-only synthetic
  placeholders. Usage shows the design's Unavailable state.
- **Message header**: the provider name and model line are not part of the
  selectable text, so copy starts at the message body.

## Workarounds and risks for review

- egui's call to register an accessibility parent is crate-private. The
  transcript creates a child `Ui` per accessibility node to register
  parents through public API. The cost applies only when an accessibility
  client is active, which on this Mac was the case during profiling (other
  apps that use accessibility APIs turn it on).
- Blocks far outside the viewport are not in the accessibility tree. A
  selection end there is reported at the edge of the exposed text.
- egui_kittest runs one frame per queued input event; checks that read
  per-frame output step frame by frame.
- On this Mac the Xcode license has not been accepted, so builds link with
  the Command Line Tools (`DEVELOPER_DIR` in the uncommitted local Cargo
  config). Accepting the license removes that workaround.

## What would change the decision

A failure in the VoiceOver or real input method checks that cannot be fixed
inside the transcript or composer would trigger the GPUI comparison before
any further component work, as decision 13 says.
