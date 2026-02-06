import SwiftUI

struct TranscriptionView: View {
    @State private var engine = SpeechEngine()
    @State private var pulseAnimation = false

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            // Header: status indicator + controls
            HStack(spacing: 8) {
                statusIndicator
                statusLabel

                Spacer()

                controlButtons
            }

            // Transcription text
            transcriptionContent
        }
        .padding(24)
        .frame(width: 440, minHeight: 100)
        .glassEffect(.regular.tint(.clear))
        .overlay(
            RoundedRectangle(cornerRadius: 20)
                .strokeBorder(.white.opacity(0.15), lineWidth: 0.5)
        )
        .padding(12)
        .onAppear {
            engine.start()
        }
        .onDisappear {
            engine.stop()
        }
    }

    // MARK: - Subviews

    @ViewBuilder
    private var statusIndicator: some View {
        if !engine.isReady {
            // Loading spinner
            ProgressView()
                .controlSize(.small)
                .frame(width: 12, height: 12)
        } else if engine.isListening {
            // Pulsing red recording dot
            Circle()
                .fill(.red)
                .frame(width: 8, height: 8)
                .scaleEffect(pulseAnimation ? 1.3 : 1.0)
                .opacity(pulseAnimation ? 0.7 : 1.0)
                .animation(
                    .easeInOut(duration: 0.8).repeatForever(autoreverses: true),
                    value: pulseAnimation
                )
                .onAppear { pulseAnimation = true }
        } else {
            Circle()
                .fill(.gray.opacity(0.5))
                .frame(width: 8, height: 8)
        }
    }

    @ViewBuilder
    private var statusLabel: some View {
        Text(statusText)
            .font(.caption)
            .foregroundStyle(.secondary)
    }

    private var statusText: String {
        if let error = engine.error {
            return error
        }
        if !engine.isReady {
            return "Starting..."
        }
        if engine.isListening {
            return "Listening"
        }
        return "Idle"
    }

    @ViewBuilder
    private var controlButtons: some View {
        HStack(spacing: 12) {
            // Discard & restart
            Button {
                engine.restart()
            } label: {
                Image(systemName: "arrow.counterclockwise")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            .buttonStyle(.plain)
            .keyboardShortcut("d", modifiers: .command)
            .help("Discard and restart (⌘D)")

            // Submit
            Button {
                engine.finish()
            } label: {
                Text("Submit")
                    .font(.caption.weight(.medium))
                    .foregroundStyle(.primary)
                    .padding(.horizontal, 10)
                    .padding(.vertical, 4)
                    .glassEffect(.regular.interactive.tint(.blue.opacity(0.2)))
            }
            .buttonStyle(.plain)
            .keyboardShortcut(.return, modifiers: [])
            .help("Submit transcription (Return)")
        }
    }

    @ViewBuilder
    private var transcriptionContent: some View {
        if engine.transcription.isEmpty && engine.isReady {
            Text("Speak now...")
                .font(.title3)
                .foregroundStyle(.tertiary)
                .frame(maxWidth: .infinity, alignment: .leading)
        } else if engine.transcription.isEmpty {
            // Still loading, show nothing
            EmptyView()
        } else {
            Text(engine.transcription)
                .font(.title3)
                .foregroundStyle(.primary)
                .frame(maxWidth: .infinity, alignment: .leading)
                .textSelection(.enabled)
                .contentTransition(.numericText())
                .animation(.easeOut(duration: 0.15), value: engine.transcription)
        }
    }
}
