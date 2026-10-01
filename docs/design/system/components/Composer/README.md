# Composer

The one composer: a rounded input over a compact toolbar, anchored near the bottom of the canvas.

- Provide `provider`, `model`, `effort`, `level` (1 to 5 on the effort ramp), `fast`, `permission`, `running`, `value` and `placeholder`.
- Left cluster: add files, then the permission control. Right cluster: the model control, then the primary action.
- The primary action is Send when idle, Stop while running with an empty draft, and Send plus a small Stop when a follow-up is typed during a run.
- The placeholder names the recipient. No provider logo appears anywhere inside the composer.
- Sits `size-inset-bottom` above the window edge, with the change strip directly above it.
