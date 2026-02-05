# Claudio

A voice-to-text CLI tool that uses native speech recognition for real-time transcription with a beautiful terminal UI.

## Features

- **Real-time transcription** using native speech recognition
- **Cross-platform** — macOS (Speech framework), Windows (Speech Recognition), Linux (Vosk)
- **Elegant inline TUI** with animated text rendering
- **Pipe to any command** — stdout works cleanly with pipes
- **Inline editing** — edit transcription in-place or open `$EDITOR`
- **Visual feedback** with animated shimmer effects for unsettled text
- **Minimal interface** that gets out of your way

## Requirements

- macOS, Windows, or Linux
- Microphone access
- Speech Recognition permissions (macOS) or Vosk model (Linux)

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

Pipe the transcription directly to another command:

```bash
claudio | pbcopy                # Copy to clipboard
claudio | claude "translate to Spanish"
claudio | gh issue create --body-file -
```

The TUI renders via `/dev/tty`, so the interface displays normally while stdout flows cleanly to the next command.

You can also use `--` to exec a command with the transcription as stdin:

```bash
claudio -- claude "Summarize this in one sentence"
```

## Controls

### Recording

- **Enter** — Finish recording and submit transcription
- **Ctrl+D** — Clear and restart (keeps recording)
- **Ctrl+E** — Enter inline editing mode
- **Ctrl+Shift+E** — Open transcription in `$EDITOR`
- **Ctrl+C** — Cancel and exit

### Editing (after Ctrl+E)

- **Ctrl+S** — Save edits and resume recording
- **Ctrl+E** — Escalate to `$EDITOR`
- **Ctrl+D** / **Escape** — Discard edits and resume recording
- **Arrow keys**, **Home**, **End** — Navigate
- **Backspace**, **Delete** — Edit text

## Visual States

- **Gray braille spinner** - Microphone warming up
- **Pulsing red dot** - Recording and listening
- **Cyan shimmer** - Unsettled text (still being processed)
- **Bright white** - Confirmed text

## How it Works

1. The app uses the platform's native speech recognition to capture and transcribe audio in real-time
2. Text appears with a smooth fade-in animation as it's being transcribed
3. Once confirmed by the recognition engine, text settles to bright white
4. Optionally edit the transcription inline (Ctrl+E) or in `$EDITOR` (Ctrl+Shift+E)
5. Press Enter to finalize and output/pipe the transcription

## Permissions

**macOS** — On first run, grant both Microphone and Speech Recognition access when prompted.

**Linux** — Download a [Vosk model](https://alphacephei.com/vosk/models) and ensure your user has access to audio capture devices.

## Examples

```bash
# Basic transcription
claudio

# Pipe to Claude AI
claudio | claude "translate to Spanish"

# Create a git commit message
claudio | git commit -F -

# Save to file
claudio > notes.txt

# Copy to clipboard
claudio | pbcopy
```

## Building

```bash
cargo build --release
```

The project uses:
- `termwiz` for terminal rendering (uses `/dev/tty` directly, enabling piped stdout)
- `objc2-speech` / `objc2-avf-audio` for macOS speech recognition
- `windows` crate for Windows speech recognition
- `vosk` / `cpal` for Linux speech recognition

## License

MIT
