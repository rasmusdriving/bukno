# StreakLabel

The working treatment, approved by Rasmus on 1 October 2026 (decision 35). It replaced the `ThinkingOrb`. The activity words show the work themselves: "Reading the composer module" in a quiet tone, with a soft light passing through the letters. No separate moving mark.

- Provide `provider`, `state` and the activity text as children. The text must come from a real engine event (the current tool, file or reasoning summary), never invented narration.
- Resting text is `text-tertiary`. The light's core is `text-primary` with just over a quarter of the provider colour mixed in, so Codex work glints faintly blue and Claude work faintly orange. The provider colour never tints anything else.
- The light is a fixed 120-point band, whatever the length of the text, so a one-word label and a long command get the same light. One pass takes `loop-streak` (2200 ms), linear and continuous. The band starts and ends outside the words, so each pass begins with a short natural pause.
- The light is wide and soft. That is deliberate: a soft, slow band looks smooth even when a native renderer draws it at 12 to 15 frames a second, where the orb's crisp dots visibly judder.
- `waiting`: no light, plain `text-secondary`, beside the attention glyph and "Needs approval".
- Reduced motion, a hidden window or a finished run: no light, plain text.
- The streak is the one live element at the end of the chat; there is no separate moving mark.
- Native rate: 10 frames a second (2.5 % of one core measured, against a budget of 3 %). The light moves about 14 points per frame on a typical label, which the soft band hides.
- `phase` (0 to 1) freezes the light at one point of its pass, for previews and frame strips.
- `native-10fps.gif` is the native app's rendering at its real 10 fps rate (from the UI check `streak_frame_rates`).
