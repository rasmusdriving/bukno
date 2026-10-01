# ActivityLine

A single tool event in plain words, with a quiet icon and a duration or count on the right.

- Provide the text as children (inline `code` for commands and paths), `icon` and `meta`.
- `toggle` turns it into a disclosure for a collapsed group: "Worked for 2m 14s · 9 steps".
- When a new event replaces the latest line, crossfade it over `dur-crossfade` without moving the layout.
