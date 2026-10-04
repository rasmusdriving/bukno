# Automatic T3 setup

Failure paths recorded before implementing the local adapter, 4 October 2026.

| Case | Expected result |
|---|---|
| Existing local server uses a different port | Read T3's runtime record, check its identity and connect without restarting it |
| Saved connection exists | Reuse its keychain sign-in and environment ID; do not create another session |
| T3 desktop is installed but closed | Open the installed app normally, wait for its server and pair through its CLI |
| CLI is installed but stopped | Start a loopback-only server in Bukno's separate T3 data folder |
| Runtime record is stale, malformed or points to another service | Do not send credentials to it or reuse its database; report a clear error for an occupied/live record |
| Running server has an incompatible protocol | Explain that T3 needs attention; do not start a competing server |
| T3 is missing | Offer one download button, use a pinned self-contained upstream release, verify its SHA-256 and start it without Node or shell setup |
| Offline, timeout, disk full or checksum mismatch | Keep a usable installation intact, remove staging files, show Retry and advanced pairing |
| Startup or pairing command hangs/fails | Bound the wait, keep the interface responsive, and keep command output and pairing secrets out of logs |
| Two setup requests overlap | Only one operation runs; never launch two servers from one client |
| Bukno closes or restarts | Leave attached T3 and persistent managed T3 running; reconnect on the next launch without a new server or sign-in |
| New server has no projects | Register the chosen chat folder through T3's CLI and open a T3 composer |
| Pairing/keychain fails | Do not claim Ready; keep a useful retry action and the existing manual pairing route |
| Remote server is already paired | Keep it connected; automatic local setup must not replace or remove it |
| Unsupported download platform | Explain the limitation and offer advanced connection; do not download the wrong architecture |
| A direct project is selected while T3 is connected | Keep its new chat and any pending direct send on the direct route |
| Remembered T3 project is removed or its environment forgotten | Clear the selection and offer a usable new-chat screen |
| T3 normalizes a chosen folder, or its project snapshot is delayed | Register a normal OS path, match the returned project ID and bound the wait with a retry message |
| Download progresses for more than five minutes | Keep downloading while bytes arrive; time out stalled reads rather than the entire request |
| A stopped managed server's PID is reused after reboot | Verify the endpoint first; ignore a failed record only with proof of a different process, and block uncertain live records |
| The clock changes while managed T3 is running | Reuse a verified live endpoint regardless of file timestamps; an unavailable endpoint must not allow a second writer |
| Process identity cannot be read or was not previously saved | Keep the live record blocking startup when its endpoint cannot be verified |
| T3 desktop is installed after Bukno created its own backend | Restart the existing managed database instead of silently choosing an empty desktop environment |
| Managed binary is missing, or a download was killed | Keep the incomplete installation for rollback until replacement succeeds; remove abandoned staging and successful repair backups under the setup lock |
| Download or replacement fails during repair | Retain the old installation and any repair backups; do not prune them before a successful replacement |
| Extraction runs while another environment is connected | Keep extraction and large file operations off the connection worker |

Live acceptance uses the real native onboarding and real T3 CLI/server. Retain
screenshots and a structured result outside Git. Check attached discovery,
download/start/pair, a Bukno restart, stopped managed-server startup, and an
invalid runtime record. Existing manual-pairing checks run with
`BUKNO_T3_AUTO_SETUP=0` to keep their pairing sessions isolated.
