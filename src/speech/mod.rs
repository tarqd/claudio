//! Speech recognition module with platform-specific implementations.
//!
//! - macOS: Native Speech framework via objc2-speech
//! - Windows: Native Windows.Media.SpeechRecognition API
//! - Linux: Vosk offline speech recognition
//! - Other platforms: Mock implementation for testing/development

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
mod linux;

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
mod mock;

// Re-export the appropriate implementation as SpeechRecognizer
#[cfg(target_os = "macos")]
pub use macos::SpeechRecognizerImpl as SpeechRecognizer;

#[cfg(target_os = "windows")]
pub use windows::SpeechRecognizerImpl as SpeechRecognizer;

#[cfg(target_os = "linux")]
pub use linux::SpeechRecognizerImpl as SpeechRecognizer;

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
pub use mock::SpeechRecognizerImpl as SpeechRecognizer;
