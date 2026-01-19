//! Speech recognition module using macOS Speech framework via objc2-speech
//!
//! On non-Apple platforms, provides a mock implementation for testing.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

use anyhow::Result;

#[cfg(target_os = "macos")]
use anyhow::anyhow;

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::AllocAnyThread;
    use objc2_avf_audio::{AVAudioEngine, AVAudioPCMBuffer, AVAudioTime};
    use objc2_foundation::{NSError, NSLocale};
    use objc2_speech::{
        SFSpeechAudioBufferRecognitionRequest, SFSpeechRecognitionResult,
        SFSpeechRecognitionTask, SFSpeechRecognizer,
    };
    use std::ptr::NonNull;

    pub struct SpeechRecognizerImpl {
        recognizer: Retained<SFSpeechRecognizer>,
        audio_engine: Retained<AVAudioEngine>,
        request: Option<Retained<SFSpeechAudioBufferRecognitionRequest>>,
        task: Option<Retained<SFSpeechRecognitionTask>>,
        transcription: Arc<Mutex<String>>,
        is_listening: Arc<AtomicBool>,
    }

    impl SpeechRecognizerImpl {
        pub fn new(
            transcription: Arc<Mutex<String>>,
            is_listening: Arc<AtomicBool>,
        ) -> Result<Self> {
            // Create speech recognizer with default locale
            let recognizer = unsafe {
                let locale = NSLocale::currentLocale();
                SFSpeechRecognizer::initWithLocale(SFSpeechRecognizer::alloc(), &locale)
            }
            .ok_or_else(|| anyhow!("Failed to create speech recognizer"))?;

            // Check if speech recognition is available
            let available = unsafe { recognizer.isAvailable() };
            if !available {
                return Err(anyhow!(
                    "Speech recognition is not available. Please check system permissions."
                ));
            }

            // Create audio engine
            let audio_engine = unsafe { AVAudioEngine::new() };

            Ok(Self {
                recognizer,
                audio_engine,
                request: None,
                task: None,
                transcription,
                is_listening,
            })
        }

        pub fn start(&mut self) -> Result<()> {
            // Create recognition request
            let request = unsafe { SFSpeechAudioBufferRecognitionRequest::new() };

            unsafe {
                request.setShouldReportPartialResults(true);
            }

            // Get input node
            let input_node = unsafe { self.audio_engine.inputNode() };

            // Get recording format
            let format = unsafe { input_node.outputFormatForBus(0) };

            // Set up the recognition handler
            let transcription = Arc::clone(&self.transcription);
            let is_listening = Arc::clone(&self.is_listening);

            let handler = RcBlock::new(
                move |result: *mut SFSpeechRecognitionResult, error: *mut NSError| {
                    if !error.is_null() {
                        return;
                    }

                    if result.is_null() {
                        return;
                    }

                    let result = unsafe { &*result };
                    let best_transcription = unsafe { result.bestTranscription() };
                    let formatted_string = unsafe { best_transcription.formattedString() };
                    let text = unsafe { formatted_string.to_string() };

                    if let Ok(mut trans) = transcription.lock() {
                        *trans = text;
                    }

                    let is_final = unsafe { result.isFinal() };
                    if is_final {
                        is_listening.store(false, Ordering::SeqCst);
                    }
                },
            );

            // Start recognition task
            let task = unsafe {
                self.recognizer
                    .recognitionTaskWithRequest_resultHandler(&request, &handler)
            };

            // Install tap on input node to capture audio
            let request_for_tap = request.clone();
            let tap_block = RcBlock::new(
                move |buffer: NonNull<AVAudioPCMBuffer>, _when: NonNull<AVAudioTime>| {
                    unsafe {
                        request_for_tap.appendAudioPCMBuffer(buffer.as_ref());
                    }
                },
            );

            unsafe {
                // Convert RcBlock to raw pointer for the C API
                let tap_block_ptr =
                    &*tap_block as *const block2::Block<_> as *mut block2::Block<_>;
                input_node.installTapOnBus_bufferSize_format_block(
                    0,
                    1024,
                    Some(&format),
                    tap_block_ptr,
                );
            }

            // Prepare and start audio engine
            unsafe {
                self.audio_engine.prepare();
                self.audio_engine
                    .startAndReturnError()
                    .map_err(|e| anyhow!("Failed to start audio engine: {:?}", e))?;
            }

            self.is_listening.store(true, Ordering::SeqCst);
            self.request = Some(request);
            self.task = Some(task);

            Ok(())
        }

        pub fn stop(&mut self) {
            self.is_listening.store(false, Ordering::SeqCst);

            unsafe {
                self.audio_engine.stop();
                let input_node = self.audio_engine.inputNode();
                input_node.removeTapOnBus(0);
            }

            if let Some(ref request) = self.request {
                unsafe {
                    request.endAudio();
                }
            }

            if let Some(ref task) = self.task {
                unsafe {
                    task.cancel();
                }
            }

            self.request = None;
            self.task = None;
        }
    }

    impl Drop for SpeechRecognizerImpl {
        fn drop(&mut self) {
            self.stop();
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod mock {
    use super::*;
    use std::thread;
    use std::time::Duration;

    /// Mock speech recognizer for non-macOS platforms (demo/testing purposes)
    pub struct SpeechRecognizerImpl {
        transcription: Arc<Mutex<String>>,
        is_listening: Arc<AtomicBool>,
        stop_signal: Arc<AtomicBool>,
    }

    impl SpeechRecognizerImpl {
        pub fn new(
            transcription: Arc<Mutex<String>>,
            is_listening: Arc<AtomicBool>,
        ) -> Result<Self> {
            Ok(Self {
                transcription,
                is_listening,
                stop_signal: Arc::new(AtomicBool::new(false)),
            })
        }

        pub fn start(&mut self) -> Result<()> {
            self.is_listening.store(true, Ordering::SeqCst);
            self.stop_signal.store(false, Ordering::SeqCst);

            let transcription = Arc::clone(&self.transcription);
            let is_listening = Arc::clone(&self.is_listening);
            let stop_signal = Arc::clone(&self.stop_signal);

            // Simulate speech recognition with demo text
            thread::spawn(move || {
                let demo_words = [
                    "Hello",
                    "world,",
                    "this",
                    "is",
                    "a",
                    "demo",
                    "of",
                    "speech",
                    "recognition.",
                    "The",
                    "words",
                    "fade",
                    "in",
                    "as",
                    "they",
                    "are",
                    "transcribed...",
                ];

                for word in demo_words.iter() {
                    if stop_signal.load(Ordering::SeqCst) {
                        break;
                    }

                    thread::sleep(Duration::from_millis(400));

                    if let Ok(mut trans) = transcription.lock() {
                        if !trans.is_empty() {
                            trans.push(' ');
                        }
                        trans.push_str(word);
                    }
                }

                is_listening.store(false, Ordering::SeqCst);
            });

            Ok(())
        }

        pub fn stop(&mut self) {
            self.stop_signal.store(true, Ordering::SeqCst);
            self.is_listening.store(false, Ordering::SeqCst);
        }
    }

    impl Drop for SpeechRecognizerImpl {
        fn drop(&mut self) {
            self.stop();
        }
    }
}

#[cfg(target_os = "macos")]
pub use macos::SpeechRecognizerImpl as SpeechRecognizer;

#[cfg(not(target_os = "macos"))]
pub use mock::SpeechRecognizerImpl as SpeechRecognizer;
