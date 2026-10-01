# EffortSlider

Stepped reasoning control that snaps to the levels the selected model exposes.

- Provide `provider`, `levels` and `value`. Pass `onChange` to control it, or leave it out and it keeps its own state.
- `size="lg"` is the thick, segmented slider used in the model picker: each level is a cell, filled cells step from dim to bright along the provider's effort ramp, and a light knob with a lightning sits on the current level.
- Only the highest level gets the moving highlight and a soft glow, and only while the picker is open. With reduced motion both stay still.
- Arrow keys move one level. The current level is always written out above the slider.
- The small default size is for dense settings rows.
