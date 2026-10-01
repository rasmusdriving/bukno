# Icon

Stroke icons on a 16px grid, drawn in `currentColor` with a 1.5px round stroke.

- Provide `name` (see `Bukno.iconNames`), optional `size` (14 or 16), `color` and `label`.
- Icons without `label` are decorative and hidden from assistive tech. Give icon-only buttons a label through `IconButton` instead.
- Keep icons in `text-secondary` or `text-tertiary`. Colour them only when they carry state, such as the lightning.
- Never use emoji as icons.
