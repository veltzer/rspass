//! Clipboard integration: wl-copy on Wayland, xclip/xsel on X11, pbcopy on
//! macOS. The clipboard is cleared after `$PASSWORD_STORE_CLIP_TIME`
//! (default 45) seconds by a detached child process, like pass(1).

use anyhow::{Context, Result, bail};
use std::io::Write;
use std::process::{Command, Stdio};

const DEFAULT_CLIP_TIME: u64 = 45;

/// The copy and clear command lines for the first available clipboard tool.
fn clipboard_tool() -> Result<(Vec<&'static str>, Vec<&'static str>)> {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() && which::which("wl-copy").is_ok() {
        return Ok((vec!["wl-copy"], vec!["wl-copy", "--clear"]));
    }
    if which::which("xclip").is_ok() {
        return Ok((vec!["xclip", "-selection", "clipboard"], vec!["xclip", "-selection", "clipboard"]));
    }
    if which::which("xsel").is_ok() {
        return Ok((vec!["xsel", "--clipboard", "--input"], vec!["xsel", "--clipboard", "--clear"]));
    }
    if which::which("pbcopy").is_ok() {
        return Ok((vec!["pbcopy"], vec!["pbcopy"]));
    }
    bail!("no clipboard tool found — install wl-clipboard, xclip, xsel, or use macOS pbcopy")
}

fn clip_time() -> u64 {
    std::env::var("PASSWORD_STORE_CLIP_TIME")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_CLIP_TIME)
}

fn pipe_to(argv: &[&str], input: &str) -> Result<()> {
    let mut child = Command::new(argv[0])
        .args(&argv[1..])
        .stdin(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to run {}", argv[0]))?;
    child
        .stdin
        .take()
        .context("clipboard tool stdin unavailable")?
        .write_all(input.as_bytes())
        .context("failed to write to clipboard tool")?;
    let status = child.wait().context("failed to wait for clipboard tool")?;
    if !status.success() {
        bail!("{} exited with {status}", argv[0]);
    }
    Ok(())
}

/// Copy `secret` to the clipboard and schedule a clear. Returns the number
/// of seconds until the clear so the caller can tell the user.
pub fn copy(secret: &str) -> Result<u64> {
    let (copy_cmd, clear_cmd) = clipboard_tool()?;
    pipe_to(&copy_cmd, secret)?;

    let seconds = clip_time();
    // Detached clearer: survives rspass exiting. `printf ''` feeds the tools
    // that clear by receiving empty input (xclip/pbcopy); wl-copy/xsel have
    // explicit clear flags and ignore stdin.
    let clear_line = clear_cmd.join(" ");
    Command::new("sh")
        .arg("-c")
        .arg(format!("sleep {seconds}; printf '' | {clear_line}"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("failed to spawn clipboard clearer")?;
    Ok(seconds)
}
