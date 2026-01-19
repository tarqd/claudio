//! Windows speech recognition using the native Windows.Media.SpeechRecognition API.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

use anyhow::Result;
use windows::{
    Foundation::TypedEventHandler,
    Globalization::Language,
    Media::SpeechRecognition::{
        SpeechContinuousRecognitionCompletedEventArgs,
        SpeechContinuousRecognitionResultGeneratedEventArgs,
        SpeechRecognizer as WinSpeechRecognizer, SpeechRecognizerState,
    },
};

pub struct SpeechRecognizerImpl {
    recognizer: Option<WinSpeechRecognizer>,
    transcription: Arc<Mutex<String>>,
    is_listening: Arc<AtomicBool>,
    is_ready: Arc<AtomicBool>,
}

impl SpeechRecognizerImpl {
    pub fn new(
        transcription: Arc<Mutex<String>>,
        is_listening: Arc<AtomicBool>,
        is_ready: Arc<AtomicBool>,
    ) -> Result<Self> {
        Ok(Self {
            recognizer: None,
            transcription,
            is_listening,
            is_ready,
        })
    }

    pub fn start(&mut self) -> Result<()> {
        // Create speech recognizer with system default language
        let language = Language::CreateLanguage(&windows::core::HSTRING::from("en-US"))
            .map_err(|e| anyhow::anyhow!("Failed to create language: {}", e))?;
        let recognizer = WinSpeechRecognizer::Create(&language)
            .map_err(|e| anyhow::anyhow!("Failed to create speech recognizer: {}", e))?;

        // Compile the default dictation grammar
        let compile_op = recognizer
            .CompileConstraintsAsync()
            .map_err(|e| anyhow::anyhow!("Failed to compile constraints: {}", e))?;

        // Block until compilation completes
        compile_op
            .get()
            .map_err(|e| anyhow::anyhow!("Failed to compile grammar: {}", e))?;

        // Get continuous recognition session
        let session = recognizer
            .ContinuousRecognitionSession()
            .map_err(|e| anyhow::anyhow!("Failed to get recognition session: {}", e))?;

        // Set up result handler for intermediate results (hypotheses)
        let transcription_for_result = Arc::clone(&self.transcription);
        let is_listening_for_result = Arc::clone(&self.is_listening);

        let result_handler = TypedEventHandler::new(
            move |_sender: &Option<_>,
                  args: &Option<SpeechContinuousRecognitionResultGeneratedEventArgs>| {
                if let Some(args) = args {
                    if let Ok(result) = args.Result() {
                        if let Ok(text) = result.Text() {
                            let text_str = text.to_string();
                            if !text_str.is_empty() {
                                if let Ok(mut trans) = transcription_for_result.lock() {
                                    *trans = text_str;
                                }
                                is_listening_for_result.store(true, Ordering::SeqCst);
                            }
                        }
                    }
                }
                Ok(())
            },
        );

        session
            .ResultGenerated(&result_handler)
            .map_err(|e| anyhow::anyhow!("Failed to register result handler: {}", e))?;

        // Set up completion handler
        let is_listening_for_complete = Arc::clone(&self.is_listening);
        let is_ready_for_complete = Arc::clone(&self.is_ready);

        let completed_handler = TypedEventHandler::new(
            move |_sender: &Option<_>,
                  _args: &Option<SpeechContinuousRecognitionCompletedEventArgs>| {
                is_listening_for_complete.store(false, Ordering::SeqCst);
                is_ready_for_complete.store(false, Ordering::SeqCst);
                Ok(())
            },
        );

        session
            .Completed(&completed_handler)
            .map_err(|e| anyhow::anyhow!("Failed to register completion handler: {}", e))?;

        // Start continuous recognition
        let start_op = session
            .StartAsync()
            .map_err(|e| anyhow::anyhow!("Failed to start recognition: {}", e))?;

        start_op
            .get()
            .map_err(|e| anyhow::anyhow!("Failed to start recognition session: {}", e))?;

        self.is_ready.store(true, Ordering::SeqCst);
        self.is_listening.store(true, Ordering::SeqCst);
        self.recognizer = Some(recognizer);

        Ok(())
    }

    pub fn stop(&mut self) {
        self.is_listening.store(false, Ordering::SeqCst);

        if let Some(ref recognizer) = self.recognizer {
            // Check if we're in a state where we can stop
            if let Ok(state) = recognizer.State() {
                if state == SpeechRecognizerState::Capturing
                    || state == SpeechRecognizerState::SoundStarted
                    || state == SpeechRecognizerState::SpeechDetected
                {
                    if let Ok(session) = recognizer.ContinuousRecognitionSession() {
                        if let Ok(stop_op) = session.StopAsync() {
                            let _ = stop_op.get();
                        }
                    }
                }
            }
        }

        self.recognizer = None;
    }
}

impl Drop for SpeechRecognizerImpl {
    fn drop(&mut self) {
        self.stop();
    }
}
