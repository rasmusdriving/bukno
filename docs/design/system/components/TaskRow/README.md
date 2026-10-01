# TaskRow

One task in Delegated work: the assignment first, then provider and state, then the latest activity.

- Provide `title`, `provider`, optional `model`, `state`, `activity`, `selected` and `child`.
- The parent (the coordinating chat) sits first; its children are indented.
- A task that needs you gets a `surface-hover` card and the words "Needs approval". The open task gets `surface-selected`.
- Clicking a task opens it in the main pane. Leave unused panel space empty.
