# ThinkingOrb

A small dotted sphere that shows an agent is working, drawn on a 2D canvas. It is the live mark in the chat while you watch a run.

- Provide `provider`, `state` (`thinking`, `reading`, `tool`, `waiting`) and `size` (32 in the chat, 20 inline).
- The dots are warm neutral; near dots are larger and brighter, far dots smaller and fainter, so depth comes from size and ink alone. The sphere turns slowly (about 20 seconds a turn) and breathes very slightly.
- The provider colour appears only where the work is:
  - `thinking`: a soft band of light drifts up and down the sphere.
  - `reading`: a band sweeps around it, like a page being scanned.
  - `tool`: two small points orbit the sphere with short tails while a command or edit runs.
  - `waiting`: the sphere stops and loses its colour until you answer.
- Map states from real engine events only: reasoning, file reads and searches, commands and edits, approvals and questions.
- It pauses when scrolled out of view or when the window is hidden, caps the pixel ratio at 2, and shows a still frame with reduced motion.
- Native: draw the same projected dots with the renderer's circle primitive at up to 30 frames a second, and stop repainting when idle or hidden.
