//! Mock speech recognizer for platforms without native support.
//!
//! Provides a demo implementation that simulates speech recognition
//! for testing and development purposes.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread;
use std::time::Duration;

use anyhow::Result;

pub struct SpeechRecognizerImpl {
    transcription: Arc<Mutex<String>>,
    is_listening: Arc<AtomicBool>,
    is_ready: Arc<AtomicBool>,
    stop_signal: Arc<AtomicBool>,
}

impl SpeechRecognizerImpl {
    pub fn new(
        transcription: Arc<Mutex<String>>,
        is_listening: Arc<AtomicBool>,
        is_ready: Arc<AtomicBool>,
    ) -> Result<Self> {
        Ok(Self {
            transcription,
            is_listening,
            is_ready,
            stop_signal: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn start(&mut self) -> Result<()> {
        self.is_ready.store(true, Ordering::SeqCst);
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
