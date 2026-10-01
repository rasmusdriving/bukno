# TodoList

The selected agent's own plan, shown in the right panel under Delegated work, so the chat holds only the conversation.

- Provide `provider`, `agent` (who the list belongs to), optional `task`, and `steps` (`state`, `title`, `detail`).
- It follows the open chat: the parent's plan, or a delegated task's plan when that task is open in the main pane.
- Steps come from Codex plan updates and Claude todo lists. The count and bar are simple done-of-total; never a time estimate.
- With no plan yet, it says so instead of showing an empty list.
