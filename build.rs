/// Build script for claudio
///
/// Handles platform-specific build configuration:
/// - Linux: Sets RPATH to $ORIGIN so the binary finds libvosk.so in the same directory
/// - Windows: Configures library search paths
/// - macOS: No special handling needed (uses native Speech framework)

fn main() {
    // Re-run if these change
    println!("cargo:rerun-if-env-changed=VOSK_LIB_PATH");
    println!("cargo:rerun-if-env-changed=VOSK_STRATEGY");

    #[cfg(target_os = "linux")]
    linux_config();

    #[cfg(target_os = "windows")]
    windows_config();
}

#[cfg(target_os = "linux")]
fn linux_config() {
    // Set RPATH to $ORIGIN so the binary looks for libvosk.so in the same directory
    // This allows distributing the binary alongside the library
    println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN");

    // If VOSK_LIB_PATH is set, add it to the library search path
    if let Ok(vosk_path) = std::env::var("VOSK_LIB_PATH") {
        println!("cargo:rustc-link-search=native={}", vosk_path);
    }

    // Also check for vosk-lib directory in project root (default fetch location)
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let vosk_lib_dir = std::path::Path::new(&manifest_dir).join("vosk-lib");
    if vosk_lib_dir.exists() {
        println!(
            "cargo:rustc-link-search=native={}",
            vosk_lib_dir.display()
        );
    }
}

#[cfg(target_os = "windows")]
fn windows_config() {
    // If VOSK_LIB_PATH is set, add it to the library search path
    if let Ok(vosk_path) = std::env::var("VOSK_LIB_PATH") {
        println!("cargo:rustc-link-search=native={}", vosk_path);
    }

    // Also check for vosk-lib directory in project root
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let vosk_lib_dir = std::path::Path::new(&manifest_dir).join("vosk-lib");
    if vosk_lib_dir.exists() {
        println!(
            "cargo:rustc-link-search=native={}",
            vosk_lib_dir.display()
        );
    }
}
