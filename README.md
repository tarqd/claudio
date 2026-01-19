# Claudio

A voice-to-text CLI tool for macOS that uses the Speech framework for real-time transcription with a beautiful terminal UI.

## Features

- **Real-time transcription** using macOS Speech Recognition
- **Elegant inline TUI** with animated text rendering
- **Pipe to any command** for seamless integration with AI tools
- **Visual feedback** with animated shimmer effects for unsettled text
- **Minimal interface** that gets out of your way

## Requirements

- macOS (uses the native Speech framework)
- Microphone access
- Speech Recognition permissions

## Installation

```bash
cargo install --path .
```

Or build from source:

```bash
cargo build --release
# Binary will be at target/release/claudio
```

## Usage

### Basic usage

Simply run `claudio` to start recording. Speak into your microphone and watch the transcription appear in real-time:

```bash
claudio
```

Press **Enter** to finish and output the transcription to stdout.

### Pipe to commands

Use `--` to pipe the transcription directly to another command:

```bash
claudio -- claude "Summarize this in one sentence"
claudio -- pbcopy  # Copy to clipboard
claudio -- gh issue create --body-file -
```

The TUI runs on stderr, so you see the interface while the transcription flows cleanly to the command's stdin.

## Controls

### Recording Mode
- **Enter** - Finish recording and submit transcription
- **Ctrl+E** - Enter inline edit mode
- **Ctrl+D** - Discard and restart
- **Ctrl+C** - Cancel and exit

### Edit Mode
- **Ctrl+S** - Confirm edits and resume recording
- **Ctrl+E** - Open in `$EDITOR` for complex edits
- **Ctrl+D** - Discard edits and resume recording (Esc also works)

## Visual States

- **Gray braille spinner** - Microphone warming up
- **Pulsing red dot** - Recording and listening
- **Cyan shimmer** - Unsettled text (still being processed)
- **Bright white** - Confirmed text

## How it Works

1. The app uses macOS Speech framework to capture and transcribe audio in real-time
2. Text appears with a smooth fade-in animation as it's being transcribed
3. Once confirmed by the recognition engine, text settles to bright white
4. Press Enter to finalize and output/pipe the transcription

## Permissions

On first run, macOS will prompt you for:
- Microphone access
- Speech Recognition access

Grant both permissions for the app to work.

## Examples

```bash
# Basic transcription
claudio

# Send to Claude AI
claudio -- claude "translate to Spanish"

# Create a git commit message
claudio -- git commit -F -

# Save to file
claudio > notes.txt

# Pipe to any command
claudio -- pbcopy
```

## Building

```bash
cargo build --release
```

The project uses:
- `ratatui` for the terminal UI
- `crossterm` for terminal control
- `objc2-speech` for macOS Speech framework bindings
- `objc2-avf-audio` for audio capture

## License

MIT
