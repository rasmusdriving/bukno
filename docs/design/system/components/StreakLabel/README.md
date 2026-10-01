# StreakLabel

Proposed working treatment (not yet approved). The activity words show the work themselves: "Reading the composer module" in a quiet tone, with a soft light passing through the letters. No separate moving mark.

- Provide `provider`, `state` and the activity text as children. The text must come from a real engine event (the current tool, file or reasoning summary), never invented narration.
- Resting text is `text-tertiary`. The light's core is `text-primary` with just over a quarter of the provider colour mixed in, so Codex work glints faintly blue and Claude work faintly orange. The provider colour never tints anything else.
- The light is a fixed 120-point band, whatever the length of the text, so a one-word label and a long command get the same light. One pass takes `loop-streak` (2200 ms), linear and continuous. The band starts and ends outside the words, so each pass begins with a short natural pause.
- The light is wide and soft. That is deliberate: a soft, slow band looks smooth even when a native renderer draws it at 12 to 15 frames a second, where the orb's crisp dots visibly judder.
- `waiting`: no light, plain `text-secondary`, beside the attention glyph and "Needs approval".
- Reduced motion, a hidden window or a finished run: no light, plain text.
- This changes the rule in the system README that activity text "never shimmers". The streak replaces the orb as the one live element, so the region still has exactly one moving thing.
- `phase` (0 to 1) freezes the light at one point of its pass, for previews and frame strips.
