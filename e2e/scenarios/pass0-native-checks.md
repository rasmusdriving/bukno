# Pass 0 native checks (by hand, in the packaged app)

These are the checks egui_kittest cannot make: what VoiceOver actually says,
real input methods, and launching from Finder. Run them on the Mac with the
packaged app and record the result in the evidence folder next to the
automatic run (`<date>/<build>/macos/synthetic/pass0-native/procedure.md`).

Build the app first:

```bash
cargo xtask package --platform macos
```

It is written to `~/Library/Caches/bukno/cargo-target/bukno-package/Bukno.app`.
Every launch is the synthetic scenario mode: no engine is started, state goes
to a temporary folder, and the titlebar says "Synthetic scenario · no engines".

## 1. Finder launch

1. In Finder, press Cmd+Shift+G, paste the folder above, double-click Bukno.
2. Expected: the window opens at 1440 by 900 with the traffic lights centred
   in the 44-point titlebar, over the sidebar. No terminal window appears.
3. Drag the window by the empty titlebar area; double-click it to zoom.

## 2. VoiceOver

Long chat: in Terminal run
`open ~/Library/Caches/bukno/cargo-target/bukno-package/Bukno.app --args --scenario long-chat`.

1. Turn on VoiceOver (Cmd+F5).
2. Move into the conversation (VO+Right until "Conversation, document").
3. Interact (VO+Shift+Down) and move through messages with VO+Right.
   Expected: each message is announced as "You, message N of 2000" or
   "Codex, message N of 2000", then its text.
4. Click inside a message and drag to select text across two or three
   messages. Expected: VoiceOver reads the selected text when the selection
   changes, with a pause between messages and no words joined. (VoiceOver's
   own "read selected text" command, listed in VoiceOver Help under text
   commands, should read the same.)
5. Press Tab to the composer. Expected: "Follow up with Codex, text field".
6. Tab through the sidebar. Expected: each row is read by name, the selected
   chat is announced as selected, Search is announced as dimmed.

Known limit: blocks far outside the viewport are not in the accessibility
tree. A selection that reaches far offscreen is read up to the edge of what
is in view.

## 3. Input methods and Swedish text

1. Swedish keyboard: type "Räksmörgås på Åre, ÅÄÖ" into the composer.
   Expected: exact text, nothing doubled.
2. U.S. International or ABC Extended: type Option+U then A to compose "ä".
   Press Enter while the dead key is pending. Expected: Enter finishes the
   character and does not send.
3. Japanese or Chinese input method, if installed: type a word, keep the
   candidate window open, press Enter to commit. Expected: committed, not sent.
   Then press Enter again. Expected: sent.
4. Shift+Enter inserts a newline; Enter sends.

## 4. Keyboard only, in the real window

1. Without the mouse, Tab to the transcript. Option+Down moves block by
   block, Shift+Option+Down extends, Cmd+C copies, Tab reaches the composer,
   Cmd+V pastes. Expected: the pasted text matches the highlighted text.
2. Focus rings show on every focused control after Tab, and never on a
   control that was just clicked.

## 5. Reduced motion

1. System Settings > Accessibility > Display > Reduce motion on (or launch
   with the environment variable `BUKNO_REDUCED_MOTION=1` to test without
   changing the setting).
2. Launch with `--scenario working`. Expected: the orb is still and the
   words "Working" and the elapsed time remain.

Record each step as Pass, Fail or Not run, with a note for anything
unexpected.
