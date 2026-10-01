# ChangeStrip

Compact strip directly above the composer: run or delegation status on the left, aggregate line counts on the right.

- Provide `status`, `statusState`, `added`, `removed`, and `files` for the expanded list.
- Expanded, it lists changed files with counts and opens each in the person's editor. No embedded diff viewer.
- Label the scope: say when files already had changes before this run. Outside Git, omit counts rather than invent them.
