//! Thin wrapper around the gpg binary, exactly like pass(1): rspass never
//! implements crypto itself, it shells out to gpg2/gpg with the same option
//! set pass uses. `$PASSWORD_STORE_GPG_OPTS` is honored for extra options.

use anyhow::{Context, Result, bail};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::runtime_flags;

/// Options pass(1) always passes to gpg.
const GPG_OPTS: &[&str] = &["--quiet", "--yes", "--compress-algo=none", "--no-encrypt-to", "--batch", "--use-agent"];

/// Locate the gpg binary: gpg2 if present, else gpg.
fn gpg_binary() -> Result<String> {
    for name in ["gpg2", "gpg"] {
        if which::which(name).is_ok() {
            return Ok(name.to_owned());
        }
    }
    bail!("gpg not found on PATH — install GnuPG")
}

fn base_command() -> Result<Command> {
    let mut cmd = Command::new(gpg_binary()?);
    cmd.args(GPG_OPTS);
    if let Ok(extra) = std::env::var("PASSWORD_STORE_GPG_OPTS") {
        cmd.args(extra.split_whitespace());
    }
    Ok(cmd)
}

fn log_command(cmd: &Command) {
    if runtime_flags::verbose() {
        eprintln!("+ {cmd:?}");
    }
}

/// Decrypt a .gpg file and return its plaintext.
pub fn decrypt(path: &Path) -> Result<String> {
    let mut cmd = base_command()?;
    cmd.arg("--decrypt").arg(path);
    log_command(&cmd);
    let output = cmd
        .stderr(Stdio::inherit())
        .output()
        .context("failed to run gpg")?;
    if !output.status.success() {
        bail!("gpg failed to decrypt {}", path.display());
    }
    String::from_utf8(output.stdout).context("decrypted content is not valid UTF-8")
}

/// Encrypt plaintext to `path` for the given recipients. Writes via a
/// temporary file in the same directory and renames, so a failed gpg run
/// never leaves a truncated entry behind.
pub fn encrypt(plaintext: &str, path: &Path, recipients: &[String]) -> Result<()> {
    let parent = path.parent().context("entry path has no parent directory")?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("failed to create {}", parent.display()))?;
    let tmp = tempfile::NamedTempFile::new_in(parent).context("failed to create temporary file")?;

    let mut cmd = base_command()?;
    cmd.arg("--encrypt");
    for r in recipients {
        cmd.arg("--recipient").arg(r);
    }
    // --yes above lets gpg overwrite the (empty) temp file we created.
    cmd.arg("--output").arg(tmp.path());
    log_command(&cmd);
    let mut child = cmd
        .stdin(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .context("failed to run gpg")?;
    child
        .stdin
        .take()
        .context("gpg stdin unavailable")?
        .write_all(plaintext.as_bytes())
        .context("failed to write plaintext to gpg")?;
    let status = child.wait().context("failed to wait for gpg")?;
    if !status.success() {
        bail!("gpg failed to encrypt to {}", path.display());
    }
    tmp.persist(path)
        .with_context(|| format!("failed to write {}", path.display()))?;
    crate::platform::restrict_permissions(path, 0o600)
        .with_context(|| format!("failed to set permissions on {}", path.display()))?;
    Ok(())
}
