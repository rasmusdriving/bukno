# ApprovalCard

Approval or question from the running agent, docked above the composer so it never scrolls away.

- Provide `provider`, `kind` (`command`, `edit`, `question`), `title`, `command`, `reason`, `meta`, and `options` for questions.
- Allow once is the primary action with Enter; Deny is Esc. Allow for this chat is explicit and scoped.
- Say how the engine protects the action, for example "Runs in the Codex sandbox, no network".
- An approval expires with its run and can never approve a later one.
