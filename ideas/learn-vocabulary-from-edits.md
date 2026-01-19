# Learn vocabulary from edits

## Summary

Track words/phrases that users frequently correct and feed them to `contextualStrings` to improve future recognition accuracy.

## Proposed Behavior

- When user edits transcription, detect new/changed words
- Store frequently corrected words in config (`~/.config/claudio/vocabulary.txt` or similar)
- Pass these to `SFSpeechRecognitionRequest.contextualStrings` on future sessions

## Implementation Ideas

- Diff original vs. edited text to find changed words
- Maintain a frequency count - only persist words corrected multiple times
- Load vocabulary file on startup, pass to speech recognizer
- Keep list bounded (Apple recommends ≤100 contextual strings)

## Use Cases

- Technical jargon the recognizer doesn't know
- Names of people, projects, or products
- Domain-specific terminology

## File Location Options

- `~/.config/claudio/vocabulary.txt` (user-level)
- `.claudio-vocab` in project root (project-level)
- Both, merged at runtime
