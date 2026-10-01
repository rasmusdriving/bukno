# Button

Text buttons. One `primary` per view, `secondary` for raised actions on the composer, `ghost` for quiet ones.

- Provide the label as children, and optionally `icon`, `iconRight`, `kbd` and `size="sm"`.
- `primary` uses `action` and `on-action`: Send, Allow once, Continue.
- `secondary` carries `shadow-raised` so it lifts off `surface-composer`: Stop, Allow for this chat.
- `danger` is text in `negative` for destructive actions in menus and settings, always with a clear verb.
- Labels are verbs in sentence case. No icons without a reason.
