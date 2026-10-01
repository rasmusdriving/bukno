# EngineCard

Setup card for one engine: whether Bukno found it, its version, and how the person is signed in.

- Provide `provider`, `status` (`ready`, `signin`, `missing`, `unsupported`, `checking`), `lines`, `text` and `actions`.
- Bukno uses the engines already installed. Sign-in happens in each engine's own tool; Bukno never handles Claude credentials.
- Outside the tested version range, say so plainly and do not guess.
