# AgentTurn

One provider reply: the provider name in its colour, optional model meta, then the body.

- Provide `provider`, optional `meta` ("GPT-6.1 Sol · High") and the body as children: paragraphs, Markdown lists and tables, `CodeBlock`s, and one collapsed `ActivityLine` ("Worked for 2m 14s") for a finished turn.
- Plans and todo lists do not go in the chat. They live in `TodoList` in the right panel.
- While the agent is still working, end the transcript with a `WorkingIndicator`.
