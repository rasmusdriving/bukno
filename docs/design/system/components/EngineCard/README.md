# EngineCard

Setup card for one engine: whether Bukno found it, its version, and how the person is signed in.

- Provide `provider`, `status` (`ready`, `signin`, `missing`, `unsupported`, `checking`), `lines`, `text` and `actions`.
- Bukno uses the engines already installed. Sign-in happens in each engine's own tool; Bukno never handles Claude credentials.
- Any installed version is used; there is no version gate. A version not yet tested with Bukno still shows `ready`, with "Not yet tested with Bukno" in `lines`.
- Use `unsupported` only when the installed version is not working with Bukno. Say so plainly and name the last working version, for example "Codex 0.160.0 is not working with Bukno. The last working version is 0.159.2." Actions: **Use 0.159.2** (primary), **Check again**, **Show details**.
- When Bukno is pinned to an earlier version, show it in `lines` with a **Try latest version** action.
