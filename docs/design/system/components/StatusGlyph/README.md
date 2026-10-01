# StatusGlyph

One 16px glyph for every run and task state, so state is readable at a glance without colour alone.

- `working` spins in the provider colour. Use it once per run: the chat header and the chat's sidebar row.
- `breathe` pulses gently for running child tasks in Delegated work. `active` is the still arc for the plan step in progress.
- `waiting` is the neutral attention disc: the run needs an approval or an answer.
- `done`, `pending`, `queued`, `failed`, `unknown`, `reconnecting` and `stopped` never move, except `reconnecting`, which turns slowly.
- Pass `still` to freeze motion; the OS reduced-motion setting does the same automatically.
