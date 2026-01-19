# Claudio

Voice-to-text CLI using macOS Speech framework with a slick Ratatui TUI.

## Quick Reference

```bash
# Build
cargo build

# Run (outputs transcription to stdout)
claudio

# Pipe to command
claudio -- pbcopy
```

## Architecture

### Two-Buffer Transcription Model

The app uses a **frozen + live** buffer architecture:

- `frozen_text: String` - Text that has been edited/finalized (owned by app)
- `live_transcription: Arc<Mutex<String>>` - Current recognition session output (shared with speech callback)

Display is always `frozen_text + live_transcription`. This enables:
- Clean separation between edited content and active recognition
- Animation only applies to live buffer
- Recognition callback remains simple (full replacement)

### Key Files

- `src/main.rs` - TUI, event loop, rendering, edit mode
- `src/speech.rs` - macOS Speech framework bindings via objc2

### Controls

| Key | Action |
|-----|--------|
| Enter | Submit full transcription |
| Ctrl+E | Edit in $EDITOR |
| Ctrl+R | Restart (clear everything) |
| Ctrl+C | Cancel (exit 130) |

### Edit Mode Flow

1. Stop recognition
2. Combine frozen + live → temp file
3. Suspend TUI (disable raw mode, clear viewport)
4. Spawn `$VISUAL` → `$EDITOR` → `vi`
5. Restore TUI
6. If editor succeeded: `frozen = edited`, `live = ""`
7. Restart recognition

### Animation System

- New characters fade cyan → white over 1.5s
- `animation_start_index` tracks where new content begins in live buffer
- Frozen text always renders as settled white
- 30 FPS render loop with inline viewport

### macOS Speech API Notes

- Uses `SFSpeechAudioBufferRecognitionRequest` with partial results
- **No pause/resume** - must stop and start new session
- **No continuation context** - each session is independent
- `contextualStrings` only helps with vocabulary hints, not prior context
- Audio engine warmup takes ~200ms (10 buffers)

### Mock Mode

On non-macOS platforms, `speech.rs` provides a mock implementation that types out demo text. Useful for UI testing.

## Common Tasks

### Adding a new keybinding

1. Add handler in `run_app()` key event match
2. Update status line in the `status_spans` vec
3. Implement the action method on `App`

### Changing animation timing

- `CHAR_DELAY_MS` - delay between character appearances
- Fade duration is hardcoded to 1500ms in `build_transcription_spans()`

### Testing without microphone

Build on Linux or use the mock mode - it simulates transcription with demo text.
