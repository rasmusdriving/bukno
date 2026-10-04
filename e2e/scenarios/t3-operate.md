# Stage 2: working in T3 chats from Bukno, end to end

The real Bukno app, offscreen through egui_kittest, works in a real T3 server
through `crates/t3-client`: new chats with Codex and Claude, approvals, a
question, queue, steer, Stop and Resume, a restart in the middle of a run,
lost replies and an offline send. Every count is checked against T3's own
HTTP projection, not against Bukno's state. The failure rows it proves are in
[t3-operate-failure-paths.md](t3-operate-failure-paths.md).

It never starts, stops or restarts the T3 server. It adds one disposable
project to T3, creates one pairing link, and leaves the project and its two
chats in T3 for inspection.

## Run it

On the machine that runs T3 Code (the Ubuntu box), from the repository:

```sh
BUKNO_EVIDENCE_DIR=/mnt/toshiba/dev/artifacts/bukno/<date>/t3-operate/<run> \
BUKNO_T3_ADDRESS=http://127.0.0.1:3773 \
  sh scripts/t3-operate-e2e.sh
```

- The script makes a Git repository under `~/.cache/bukno-e2e/` with one
  committed file and one uncommitted change in `notes.md`, and adds it with
  `t3 project add`.
- It mints the pairing link with `t3 pair` into a private temporary file. The
  link is never printed; the test deletes the file after reading it.
- Models default to `Codex · GPT 6.1 Sol` and `Claude · Claude Sonnet 5.5` as the
  model menu names them; `BUKNO_T3_CODEX_MODEL` and `BUKNO_T3_CLAUDE_MODEL`
  change them.
- It uses a little real Codex and Claude usage: about ten short turns.

## What it does

| Step | Proves |
|---|---|
| Pair with a fresh link | The sign-in has `orchestration:read orchestration:operate` (O1) |
| New chat in the project from the sidebar, Codex model, first message | `orchestration.launchThread`; the message is in T3 once (O19) |
| Approval card; Enter in the empty composer; Allow once | The card waits for its own keys (O9); T3 records `accept` |
| Follow-up; Deny | T3 records `decline` |
| `sleep 40` turn; Enter queues, Steer now, Alt+Enter steers | Intents `queued_turn`, `promoted_queued_to_steer`, `steer`; the caption says "Queued for Codex's next turn" (O6) |
| Queue one more; Stop; Resume | Run `interrupted`; the message waits, held; Resume sends it (O10, O11) |
| New Claude chat; approval; a question with Tea and Coffee | Claude through T3; the option's value goes back (O21) |
| Draft typed in Claude chat; Codex turn running; quit; relaunch | The running chat reopens and Stop works; the draft comes back (O15, O16) |
| Relay passes the request but drops replies, then cuts | The message is found in the chat and shown as sent, once (O3) |
| Relay holds the request, then cuts | "Not confirmed" with Send again; sent once afterwards (O3) |
| Relay refuses connections | Send is unavailable, the draft stays, nothing reaches T3 (O5) |
| Relay drops the reply, Bukno quits, relaunch | The saved send is checked with its IDs: shown as sent, draft cleared, once in T3 (O3, O15) |
| Every sent text counted in T3 | Each exactly once (O19) |
| `notes.md` compared with its start | The uncommitted change is untouched (O18) |
| Log, state folder and evidence searched | No token, link or chat text (O20) |

## Evidence

`result.json` (every check with what was observed), `screens/*.png`,
`t3-client.log`, `t3-client-state.json` (drafts only), `demo.mp4` and
`captions.srt`. Screens and video show only the disposable project's chats.

The video is the real app's own frames from this run, rendered by the
harness at 1440 by 900 with a drawn pointer, not a capture of a desktop
window. Interaction plays in real time at 10 frames a second; waits for a
provider are sped up five times, and each new caption holds the picture for
two seconds. Captions name each step.

Claude Haiku 4.5 is listed by this server but its requests fail there
("unknown provider for model claude-haiku-4-5", seen in run 1), so the check
uses Claude Sonnet 5.5.

## Not covered by this run

- A real T3 server restart; the relay's refuse-and-drop stands in for it (O17).
- An approval answered elsewhere while the card shows (O7), a provider session
  that ended under a pending request (O8), and queue changes made from another
  client (O12): source only.
- Launch replies lost in transit (O13): the same code path as O3, not cut.
- A Stage 1 read-only sign-in refused at send time (O1 negative path): the
  check is in the client before sending; not exercised against the server.
