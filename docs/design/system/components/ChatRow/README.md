# ChatRow

Sidebar row for one chat: its title, and on the right the owning provider's mark or its live state.

- Provide `title`, `provider`, `state`, `selected` and `child` (indented under a project).
- The right slot shows the muted provider mark at rest, the spinning ring while working, the attention disc when it needs you, and an unread dot for a new result.
- Selection is a `surface-selected` fill only. Keyboard focus adds `focus-ring`; the two never look the same.
- Chats without a project sit under Chats with no indent. Starting a chat never requires a project.
