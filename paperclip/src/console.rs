// src/console.rs
// Enables ANSI escape-code processing on the Windows console so that the
// `colored` crate's output renders as colour instead of leaking raw codes
// like "←[33m". Without this, cmd.exe prints the escape sequences literally.
//
// This uses the `colored` crate's own Windows helper rather than calling the
// Win32 API directly — no extra dependency, no unsafe code. It's a one-time,
// best-effort setup call made at startup; the result is ignored, so if the
// console can't be switched (e.g. stdout is redirected to a file) we simply
// carry on uncoloured, which is the right behaviour for piped output anyway.

/// Turns on virtual-terminal (ANSI) processing for stdout on Windows.
/// Safe to call once at program start; a no-op cost on other platforms.
pub fn enable_ansi() {
    // set_virtual_terminal returns Result<(), ()>; we don't care if it fails.
    let _ = colored::control::set_virtual_terminal(true);
}
