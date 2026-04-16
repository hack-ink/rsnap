# Validate macOS Native Capture Window Layer

Goal: Run the manual validation matrix for the macOS-native passive capture shells and explicit
key-focus shell.

Read this when: You changed macOS capture-window/focus code, native AppKit shell bridging, or the
Frozen text/scroll key-focus path and need to verify behavior on a live machine.

Inputs: A macOS machine with Screen Recording permission granted, a build of `rsnap`, and the
capture-session contract in `docs/spec/capture-session.md`.

Depends on: `docs/spec/capture-session.md`,
`docs/reference/macos-native-capture-window-layer.md`

Verification: Confirm that pointer capture stays passive, text/scroll keyboard routes still work,
and no capture flow regresses into visible focus theft or missing IME behavior.

## Validation matrix

1. Live window click
   - Start capture with another app frontmost.
   - Hover a target window, then click to enter Frozen mode.
   - Verify the target app does not remain blurred after selection completes.
   - Verify rsnap does not need a visible Dock/key-window activation to complete the click flow.

2. Live drag region
   - Start capture over another app.
   - Press-drag-release to create a region freeze.
   - Verify auxiliary live widgets disappear on press and do not reappear during handoff.
   - Verify the target app remains the apparent frontmost app after selection completes.

3. Frozen text editing
   - Enter Frozen mode, switch to the text tool, and click to start editing.
   - Type plain ASCII text.
   - Verify text appears and `Esc` cancels text editing instead of the whole session.
   - Use an IME preedit flow and confirm marked text and commit both work.

4. Scroll capture keyboard flow
   - Enter scroll capture from a dragged-region freeze.
   - Verify `Space`, save shortcut, pause, undo, and `Esc` still work.
   - Verify these controls continue working even though live pointer interaction stays passive.

5. Session exit cleanup
   - Cancel capture from live mode and from Frozen mode.
   - Restart capture immediately after each exit.
   - Verify the next session does not inherit stale focus, stale key ownership, or missing
     pointer input.
