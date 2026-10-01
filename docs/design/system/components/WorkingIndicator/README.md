# WorkingIndicator

The quiet live line at the end of the transcript while an agent works: a `ThinkingOrb`, what the agent is doing right now, and its latest reasoning summary.

- Provide `mark` (`orb`, the approved default, or `grid` for the proposed `BuildGrid`), `provider`, `state` (`thinking`, `reading`, `tool`, `waiting`), `activity`, optional `summary`, `time`, and optional `trail` (recent events, shown as one quiet line).
- Only the orb moves. The activity and summary fade in once when they change; nothing shimmers or slides.
- Show only what the engine reports: tool events, plan updates and public reasoning summaries. Never invent narration to keep it busy.
- This is the run's one live element while you watch the chat, so the header chip stays still. When reply text starts streaming, the indicator gives way to the text.
