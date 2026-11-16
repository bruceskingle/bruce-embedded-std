//! Copying text for the user to paste, e.g. a simulated device's address into an app, with the computer's own command
//! for it, pbcopy on macOS, clip on Windows, and wl-copy or xclip on Linux, so that the simulator needs no library for
//! it.

use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::anyhow;

/// The commands which copy their input, in the order tried.
fn commands() -> &'static [(&'static str, &'static [&'static str])] {
    if cfg!(target_os = "macos") {
        &[("pbcopy", &[])]
    } else if cfg!(windows) {
        &[("clip", &[])]
    } else {
        &[
            ("wl-copy", &[]),
            ("xclip", &["-selection", "clipboard"]),
            ("xsel", &["--clipboard", "--input"]),
        ]
    }
}

/// Copies text, with the first of commands which runs.
pub fn copy(text: &str) -> anyhow::Result<()> {
    for (program, args) in commands() {
        let Ok(mut child) = Command::new(program).args(*args).stdin(Stdio::piped()).spawn() else {
            continue;
        };
        if let Some(mut input) = child.stdin.take() {
            input.write_all(text.as_bytes())?;
        }
        if child.wait()?.success() {
            return Ok(());
        }
    }
    Err(anyhow!("There is no command to copy with, e.g. wl-copy or xclip"))
}
