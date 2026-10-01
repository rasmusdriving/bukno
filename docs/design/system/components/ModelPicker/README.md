# ModelPicker

Model picker opened from the composer's model control: the current model on top, a thick reasoning slider, then fast mode. Clicking the model row opens the model list.

- Provide `groups` (one per provider, `codex` then `claude`) with `models`. Each model can carry its own `levels`, default `effort`, `details` (one line per level) and `fastAvailable`.
- The first view is about effort, because that is what people change most. The model row shows the model and where it runs ("ChatGPT model · Codex").
- The model list has two sections: **ChatGPT models** (runs in Codex) and **Claude models** (runs in Claude Code). Picking a model returns to the effort view with that model's levels.
- In a new chat both sections are open, and the model decides the provider. In an existing chat pass `lockedProvider`; the other section explains that switching is a handoff, tagged Later.
- Levels and their descriptions come from the selected engine and model. Never show a level the engine does not offer.
- The preview is clickable: try the model row, a model, the slider and the switch.
