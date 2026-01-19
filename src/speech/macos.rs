//! macOS speech recognition using the native Speech framework.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Result};
use block2::RcBlock;
use objc2::rc::Retained;
use objc2::AllocAnyThread;
use objc2_avf_audio::{AVAudioEngine, AVAudioPCMBuffer, AVAudioTime};
use objc2_foundation::{NSError, NSLocale, NSOperationQueue};
use objc2_speech::{
    SFSpeechAudioBufferRecognitionRequest, SFSpeechRecognitionResult,
    SFSpeechRecognitionTask, SFSpeechRecognizer, SFSpeechRecognizerAuthorizationStatus,
};
use std::ptr::NonNull;

pub struct SpeechRecognizerImpl {
    recognizer: Retained<SFSpeechRecognizer>,
    audio_engine: Retained<AVAudioEngine>,
    request: Option<Retained<SFSpeechAudioBufferRecognitionRequest>>,
    task: Option<Retained<SFSpeechRecognitionTask>>,
    transcription: Arc<Mutex<String>>,
    is_listening: Arc<AtomicBool>,
    is_ready: Arc<AtomicBool>,
    // Keep blocks alive
    _tap_block: Option<RcBlock<dyn Fn(NonNull<AVAudioPCMBuffer>, NonNull<AVAudioTime>)>>,
    _handler: Option<RcBlock<dyn Fn(*mut SFSpeechRecognitionResult, *mut NSError)>>,
}

impl SpeechRecognizerImpl {
    pub fn new(
        transcription: Arc<Mutex<String>>,
        is_listening: Arc<AtomicBool>,
        is_ready: Arc<AtomicBool>,
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

        // Set a custom operation queue for callbacks (CLI apps don't have a main run loop)
        let queue = NSOperationQueue::new();
        unsafe {
            recognizer.setQueue(&queue);
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
            is_ready,
            _tap_block: None,
            _handler: None,
        })
    }

    pub fn start(&mut self) -> Result<()> {
        // Check authorization status
        let auth_status = unsafe { SFSpeechRecognizer::authorizationStatus() };

        // Request authorization if not determined
        if auth_status.0 == 0 {
            let auth_granted = Arc::new(Mutex::new(None));
            let auth_granted_clone = Arc::clone(&auth_granted);

            let handler = block2::RcBlock::new(move |status: SFSpeechRecognizerAuthorizationStatus| {
                if let Ok(mut granted) = auth_granted_clone.lock() {
                    *granted = Some(status.0 == 3);
                }
            });

            unsafe {
                SFSpeechRecognizer::requestAuthorization(&handler);
            }

            // Wait for authorization response (with timeout)
            for _ in 0..50 {
                thread::sleep(Duration::from_millis(100));
                if let Ok(granted) = auth_granted.lock() {
                    if let Some(is_granted) = *granted {
                        if !is_granted {
                            return Err(anyhow!(
                                "Speech recognition permission denied. Please grant permission in System Settings."
                            ));
                        }
                        break;
                    }
                }
            }

            // Final check
            let final_status = unsafe { SFSpeechRecognizer::authorizationStatus() };
            if final_status.0 != 3 {
                return Err(anyhow!(
                    "Speech recognition not authorized. Please grant permission when prompted."
                ));
            }
        } else if auth_status.0 != 3 {
            return Err(anyhow!(
                "Speech recognition permission denied. Please enable it in System Settings > Privacy & Security > Speech Recognition."
            ));
        }

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
        let is_listening_for_tap = Arc::clone(&self.is_listening);
        let is_ready_for_tap = Arc::clone(&self.is_ready);

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
                let text = formatted_string.to_string();

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
        let buffer_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let tap_block = RcBlock::new(
            move |buffer: NonNull<AVAudioPCMBuffer>, _when: NonNull<AVAudioTime>| {
                // Count audio buffers and set ready after warmup period
                let count = buffer_count.fetch_add(1, Ordering::SeqCst);
                if count >= 10 {
                    // After ~10 buffers (~200ms at 1024 samples/buffer), we're ready
                    is_ready_for_tap.store(true, Ordering::SeqCst);
                    is_listening_for_tap.store(true, Ordering::SeqCst);
                }
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

        self.request = Some(request);
        self.task = Some(task);
        self._tap_block = Some(tap_block);
        self._handler = Some(handler);

        // is_listening will be set to true by the tap callback once audio is flowing

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
        self._tap_block = None;
        self._handler = None;
    }
}

impl Drop for SpeechRecognizerImpl {
    fn drop(&mut self) {
        self.stop();
    }
}
