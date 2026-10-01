# UsageMeter

Cached usage for one provider window, shown above the profile row.

- Provide `provider`, `window` ("5h"), `left` (percent remaining), `state` and `updated`.
- `fresh` shows the value. `stale` keeps the value, dims the bar and says when it was read. `unavailable` shows no bar and the word Unavailable, never zero.
- Usage is shared per provider and account, never per chat. Do not poll while idle.
- `reset` adds the reset time under the bar in the profile menu ("Resets 16:20"). The sidebar keeps the compact form.
