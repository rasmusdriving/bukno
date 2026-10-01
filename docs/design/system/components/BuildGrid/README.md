# BuildGrid

Proposed replacement for `ThinkingOrb` (not yet approved). The working mark at the end of the transcript: nine small blocks on a 3 by 3 grid, with work placed one block at a time. Bukno is build plus knowledge; the mark shows something being built, calmly, in hard steps.

- Provide `provider`, `state` (`thinking`, `reading`, `tool`, `waiting`) and `size` (32 in the chat, 20 inline, 16 in a row).
- Blocks have four levels: empty (`surface-raised`), a faint trace (`text-disabled`), placed (`text-secondary`) and the newest block in the provider colour. The provider colour only ever marks the newest block, so it shows where the work is without tinting the mark.
- `thinking`: a three-block trail walks the grid in a winding path and turns back at each end. Nothing jumps.
- `reading`: rows light top to bottom, like lines being read, then rest for one step.
- `tool`: the centre block holds, and one block turns around it like a gear while a command or edit runs.
- `waiting`: still and neutral, no provider colour, until you answer.
- It moves in hard steps of `step-working` (160 ms), never tweens. Six changes a second is the whole animation, which keeps a native renderer at about six frames a second instead of twenty or more. Do not add easing between steps; the trail's three levels already make it read as smooth.
- With reduced motion, or when the window is hidden, it shows one still step and the words stay.
- Map states from real engine events only, as for the orb.
- Geometry at 32: blocks 6 points with a 3-point gap and a 1.7-point radius, centred. Scale every value with `size`.
