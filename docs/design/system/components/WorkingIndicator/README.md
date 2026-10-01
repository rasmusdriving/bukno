# WorkingIndicator

The quiet live line at the end of the transcript while an agent works: what the agent is doing right now, carried by a `StreakLabel`, and its latest reasoning summary.

- Provide `provider`, `state` (`thinking`, `reading`, `tool`, `waiting`), `activity`, optional `summary`, `time`, and optional `trail` (recent events, shown as one quiet line).
- Only the light in the activity words moves. The summary fades in once when it changes and never shimmers. `mark` (`orb` or `grid`) shows the earlier treatments for reference.
- Show only what the engine reports: tool events, plan updates and public reasoning summaries. Never invent narration to keep it busy.
- This is the run's one live element while you watch the chat, so the header chip stays still. When reply text starts streaming, the indicator gives way to the text.
