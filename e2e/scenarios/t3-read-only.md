# Stage 1: read-only T3 client, end to end

The real Bukno app, offscreen through egui_kittest, reads a real T3 server
through `crates/t3-client`. Every comparison is against T3's own HTTP
snapshots, not against Bukno's own state. The failure rows it proves are in
[t3-client-failure-paths.md](t3-client-failure-paths.md).

It never starts, stops or restarts the T3 server. It does create pairing
links and one revocation on that server, as a user would in T3's settings.

## Run it

On the machine that runs T3 Code (the Ubuntu box), from the repository:

```sh
BUKNO_EVIDENCE_DIR=/mnt/toshiba/dev/artifacts/bukno/<date>/t3-read-only/<run> \
BUKNO_T3_ADDRESS=http://100.127.119.35:3773 \
BUKNO_T3_LONG_THREAD=<id of a long chat> \
BUKNO_T3_PAGED_THREAD=<id of a chat with more than 10 user messages> \
BUKNO_T3_LIVE_THREAD=<id of a chat that will be working during the run> \
  sh scripts/t3-live-e2e.sh
```

- The script mints two fresh pairing links with T3's own `t3 pair`, into a
  private temporary file. The links are never printed. The test deletes the
  file after reading it.
- When the output says `LIVE WINDOW OPEN`, make the live chat do something in
  T3 (for example, let an agent run commands). It opens twice: once for live
  updates, once while the network is cut.
- `BUKNO_T3_CLI` and `BUKNO_T3_CLI_ENTRY` point at T3's command line (default:
  the installed T3 Code Nightly), used for `t3 pair`, `t3 auth session list`
  and `t3 auth session revoke`.
- The replay checks need no server: `cargo test -p bukno-t3-client --test replay`.

## What it does

| Step | Proves |
|---|---|
| Open Add environment from the sidebar, try a wrong address | F6: clear message, no link sent |
| Pair with the first fresh link | Token in the keychain; read-only scope (`orchestration:read`) on Bukno's side and in T3's session list |
| Compare the sidebar with `GET /api/orchestration/shell`, and models with `server.getConfig` | Projects, chats and models match T3 |
| Open the long chat from the sidebar, compare with `GET /api/orchestration/threads/:id` | Same messages and rows, in order, no duplicates |
| Open the paged chat, click Load older messages until done | History arrives in pages and then matches the full snapshot |
| Open the live chat for 40 s | New rows appear while it runs, then match T3 |
| Quit, point the saved address at a local relay, relaunch | Restart: token from the keychain, chat matches |
| Relay passes nothing for 30 s, then drops the sockets | F14 silent loss noticed within 15 s of pings; F12/F13 catch-up with no duplicate or missing rows |
| Relay refuses new connections and drops the old ones, then recovers | Server-gone behavior; catch-up matches again |
| Relay injects an unknown frame tag, an unknown stream item, an unknown turn item type and an unknown event type | F9: unsupported row shown, the rest logged, connection stays up |
| Read the client log; read T3's command receipts | F18: only read-only methods were sent |
| Revoke Bukno's session with `t3 auth session revoke`, reconnect | F3: "no longer accepts" message; pair again with the second link |
| Search the state folder and evidence for the tokens and links | F19: none found |

Evidence: `result.json` (every check with what was observed),
`screens/*.png`, `t3-client.log` and `t3-environments.json` (no tokens).
Screenshots show real chat content, so they stay in the artifacts folder,
never in Git.

## Not covered by this run

- A real T3 server restart: the agent that runs this check works inside that
  server. The relay's refuse-and-drop step stands in for it.
- Tokens past their 30-day expiry (checked from the stored expiry, not waited out).
- Wrong server identity (F7) and protocol mismatch (F8) through the app UI:
  F8 is checked with `t3-probe socket-protocol`; F7 needs a second T3 server.
- A slow consumer pushing the server past its 1,000-item buffer (F11).
- HTTPS addresses: refused with a clear message in this stage.
