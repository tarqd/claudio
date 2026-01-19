//! Linux speech recognition using Vosk offline speech recognition.
//!
//! Requires a Vosk model to be downloaded and available. The model path
//! can be configured via:
//! 1. `VOSK_MODEL_PATH` environment variable
//! 2. `~/.local/share/vosk/model` (default)
//!
//! Download models from: https://alphacephei.com/vosk/models

use std::env;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread;

use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use vosk::{Model, Recognizer};

pub struct SpeechRecognizerImpl {
    transcription: Arc<Mutex<String>>,
    is_listening: Arc<AtomicBool>,
    is_ready: Arc<AtomicBool>,
    stop_signal: Arc<AtomicBool>,
    stream_handle: Option<thread::JoinHandle<()>>,
}

impl SpeechRecognizerImpl {
    pub fn new(
        transcription: Arc<Mutex<String>>,
        is_listening: Arc<AtomicBool>,
        is_ready: Arc<AtomicBool>,
    ) -> Result<Self> {
        // Verify model exists at startup
        let model_path = Self::get_model_path()?;
        if !model_path.exists() {
            return Err(anyhow!(
                "Vosk model not found at: {}\n\
                 Download a model from https://alphacephei.com/vosk/models\n\
                 and extract it to ~/.local/share/vosk/model\n\
                 or set VOSK_MODEL_PATH environment variable",
                model_path.display()
            ));
        }

        Ok(Self {
            transcription,
            is_listening,
            is_ready,
            stop_signal: Arc::new(AtomicBool::new(false)),
            stream_handle: None,
        })
    }

    fn get_model_path() -> Result<PathBuf> {
        // Check environment variable first
        if let Ok(path) = env::var("VOSK_MODEL_PATH") {
            return Ok(PathBuf::from(path));
        }

        // Default to ~/.local/share/vosk/model
        let home = env::var("HOME").map_err(|_| anyhow!("HOME environment variable not set"))?;
        Ok(PathBuf::from(home).join(".local/share/vosk/model"))
    }

    pub fn start(&mut self) -> Result<()> {
        self.stop_signal.store(false, Ordering::SeqCst);

        let model_path = Self::get_model_path()?;
        let transcription = Arc::clone(&self.transcription);
        let is_listening = Arc::clone(&self.is_listening);
        let is_ready = Arc::clone(&self.is_ready);
        let stop_signal = Arc::clone(&self.stop_signal);

        // Spawn audio capture thread
        let handle = thread::spawn(move || {
            if let Err(e) = Self::run_recognition(
                model_path,
                transcription,
                is_listening,
                is_ready,
                stop_signal,
            ) {
                eprintln!("Speech recognition error: {}", e);
            }
        });

        self.stream_handle = Some(handle);
        Ok(())
    }

    fn run_recognition(
        model_path: PathBuf,
        transcription: Arc<Mutex<String>>,
        is_listening: Arc<AtomicBool>,
        is_ready: Arc<AtomicBool>,
        stop_signal: Arc<AtomicBool>,
    ) -> Result<()> {
        // Load the Vosk model
        let model = Model::new(model_path.to_string_lossy()).ok_or_else(|| {
            anyhow!(
                "Failed to load Vosk model from {}",
                model_path.display()
            )
        })?;

        // Set up audio capture
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| anyhow!("No input device available"))?;

        let config = device.default_input_config()?;
        let sample_rate = config.sample_rate().0 as f32;
        let channels = config.channels() as usize;

        // Create recognizer with the sample rate
        let mut recognizer = Recognizer::new(&model, sample_rate).ok_or_else(|| {
            anyhow!("Failed to create Vosk recognizer")
        })?;

        recognizer.set_words(true);
        recognizer.set_partial_words(true);

        // Buffer for audio samples
        let audio_buffer: Arc<Mutex<Vec<i16>>> = Arc::new(Mutex::new(Vec::new()));
        let audio_buffer_for_callback = Arc::clone(&audio_buffer);

        // Build the input stream
        let stream = device.build_input_stream(
            &config.into(),
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                // Convert f32 samples to i16 and collect
                let samples: Vec<i16> = data
                    .chunks(channels)
                    .map(|frame| {
                        // Average channels to mono
                        let sum: f32 = frame.iter().sum();
                        let mono = sum / channels as f32;
                        (mono * 32767.0) as i16
                    })
                    .collect();

                if let Ok(mut buffer) = audio_buffer_for_callback.lock() {
                    buffer.extend(samples);
                }
            },
            |err| {
                eprintln!("Audio stream error: {}", err);
            },
            None,
        )?;

        stream.play()?;
        is_ready.store(true, Ordering::SeqCst);
        is_listening.store(true, Ordering::SeqCst);

        // Process audio in a loop
        while !stop_signal.load(Ordering::SeqCst) {
            // Get accumulated samples
            let samples: Vec<i16> = {
                let mut buffer = audio_buffer.lock().unwrap();
                std::mem::take(&mut *buffer)
            };

            if !samples.is_empty() {
                // Feed to recognizer
                let _ = recognizer.accept_waveform(&samples);

                // Get partial result for real-time feedback
                let partial = recognizer.partial_result().partial;
                if !partial.is_empty() {
                    if let Ok(mut trans) = transcription.lock() {
                        *trans = partial.to_string();
                    }
                }
            }

            // Small sleep to avoid busy-waiting
            thread::sleep(std::time::Duration::from_millis(50));
        }

        // Get final result
        let final_result = recognizer.final_result();
        if let Some(result) = final_result.single() {
            if !result.text.is_empty() {
                if let Ok(mut trans) = transcription.lock() {
                    *trans = result.text.to_string();
                }
            }
        }

        is_listening.store(false, Ordering::SeqCst);
        Ok(())
    }

    pub fn stop(&mut self) {
        self.stop_signal.store(true, Ordering::SeqCst);
        self.is_listening.store(false, Ordering::SeqCst);

        // Wait for the thread to finish
        if let Some(handle) = self.stream_handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for SpeechRecognizerImpl {
    fn drop(&mut self) {
        self.stop();
    }
}
