# Visual diff for edited text

## Summary

After using Ctrl+E to edit transcription, visually distinguish edited portions from original text.

## Proposed Behavior

- Edited text could have a subtle visual indicator (e.g., slightly different shade or dim highlight)
- Helps users see at a glance what was manually corrected vs. transcribed
- Should be subtle enough not to distract during continued dictation

## Implementation Ideas

- Track original text before opening editor
- Compute simple diff when editor returns
- Render changed portions with distinct style (e.g., slightly dimmer white, faint underline)
- Could use `similar` crate for diffing

## Considerations

- Keep it subtle - visual cue, not a full diff view
- May want to fade the distinction over time or after next edit
- Could be optional/configurable
