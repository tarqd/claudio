import Foundation
import Speech
import AVFoundation

/// Observable speech recognition engine using macOS native Speech framework.
/// Mirrors the behavior of the Rust `macos.rs` implementation.
@MainActor
@Observable
final class SpeechEngine {
    var transcription: String = ""
    var isListening: Bool = false
    var isReady: Bool = false
    var error: String?

    private var recognizer: SFSpeechRecognizer?
    private var audioEngine: AVAudioEngine?
    private var request: SFSpeechAudioBufferRecognitionRequest?
    private var task: SFSpeechRecognitionTask?
    private var bufferCount: Int = 0

    init() {
        recognizer = SFSpeechRecognizer(locale: .current)
    }

    func start() {
        Task { @MainActor in
            do {
                try await requestPermissions()
                try startRecognition()
            } catch {
                self.error = error.localizedDescription
            }
        }
    }

    func stop() {
        audioEngine?.stop()
        audioEngine?.inputNode.removeTap(onBus: 0)
        request?.endAudio()
        task?.cancel()

        audioEngine = nil
        request = nil
        task = nil
        isListening = false
    }

    /// Stop listening and return the final transcription, printing to stdout.
    func finish() {
        stop()
        let text = transcription.trimmingCharacters(in: .whitespacesAndNewlines)
        if !text.isEmpty {
            // Print to stdout like the TUI mode does
            FileHandle.standardOutput.write(Data((text + "\n").utf8))
        }
        NSApplication.shared.terminate(nil)
    }

    func restart() {
        stop()
        transcription = ""
        isReady = false
        bufferCount = 0
        start()
    }

    // MARK: - Private

    private func requestPermissions() async throws {
        // Request speech recognition permission
        let speechStatus = await withCheckedContinuation { continuation in
            SFSpeechRecognizer.requestAuthorization { status in
                continuation.resume(returning: status)
            }
        }

        guard speechStatus == .authorized else {
            throw ClaudioError.permissionDenied(
                "Speech recognition permission denied. Grant access in System Settings > Privacy & Security > Speech Recognition."
            )
        }

        // Request microphone permission
        let micGranted = await AVAudioApplication.requestRecordPermission()
        guard micGranted else {
            throw ClaudioError.permissionDenied(
                "Microphone permission denied. Grant access in System Settings > Privacy & Security > Microphone."
            )
        }
    }

    private func startRecognition() throws {
        guard let recognizer, recognizer.isAvailable else {
            throw ClaudioError.unavailable("Speech recognition is not available on this system.")
        }

        let audioEngine = AVAudioEngine()
        let request = SFSpeechAudioBufferRecognitionRequest()
        request.shouldReportPartialResults = true

        let inputNode = audioEngine.inputNode
        let format = inputNode.outputFormat(forBus: 0)

        // Recognition handler
        let task = recognizer.recognitionTask(with: request) { [weak self] result, error in
            Task { @MainActor [weak self] in
                guard let self else { return }

                if let error {
                    // Ignore cancellation errors during normal stop
                    if (error as NSError).code != 216 { // kAFAssistantErrorDomain canceled
                        self.error = error.localizedDescription
                    }
                    return
                }

                guard let result else { return }

                self.transcription = result.bestTranscription.formattedString

                if result.isFinal {
                    self.isListening = false
                }
            }
        }

        // Audio tap - feeds audio buffers to the recognition request
        inputNode.installTap(onBus: 0, bufferSize: 1024, format: format) { [weak self] buffer, _ in
            request.append(buffer)

            Task { @MainActor [weak self] in
                guard let self else { return }
                self.bufferCount += 1
                if self.bufferCount >= 10 {
                    self.isReady = true
                    self.isListening = true
                }
            }
        }

        audioEngine.prepare()
        try audioEngine.start()

        self.audioEngine = audioEngine
        self.request = request
        self.task = task
    }
}

enum ClaudioError: LocalizedError {
    case permissionDenied(String)
    case unavailable(String)

    var errorDescription: String? {
        switch self {
        case .permissionDenied(let msg), .unavailable(let msg):
            return msg
        }
    }
}
