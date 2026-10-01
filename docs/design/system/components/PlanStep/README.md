# PlanStep

One item of the provider's own plan or todo list, with a state glyph and an optional detail line.

- Provide `state` (`done`, `active`, `pending`, `failed`), `title`, optional `detail` and `provider` for the active arc.
- Plans come from Codex plan updates and Claude todo lists. Do not invent steps.
- Steps are shown inside `TodoList` in the right panel, not in the chat.
- Done steps drop to `text-secondary` so the active step leads.
