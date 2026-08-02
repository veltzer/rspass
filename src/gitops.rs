//! Git integration for the store, like pass(1): if the store is a git
//! repository, every mutation is committed automatically; `rspass git ...`
//! passes through to git with the store as the working directory.

use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::Command;

use crate::runtime_flags;

fn is_repo(store_root: &Path) -> bool {
    store_root.join(".git").exists()
}

fn run_git(store_root: &Path, args: &[&str]) -> Result<()> {
    if runtime_flags::verbose() {
        eprintln!("+ git -C {} {}", store_root.display(), args.join(" "));
    }
    let status = Command::new("git")
        .arg("-C")
        .arg(store_root)
        .args(args)
        .status()
        .context("failed to run git")?;
    if !status.success() {
        bail!("git {} failed with {status}", args.join(" "));
    }
    Ok(())
}

/// Stage everything and commit with `message`. A no-op when the store is not
/// a git repository or there is nothing to commit.
pub fn commit(store_root: &Path, message: &str) -> Result<()> {
    if !is_repo(store_root) {
        return Ok(());
    }
    run_git(store_root, &["add", "-A"])?;
    let has_changes = !Command::new("git")
        .arg("-C")
        .arg(store_root)
        .args(["diff", "--cached", "--quiet"])
        .status()
        .context("failed to run git")?
        .success();
    if has_changes {
        run_git(store_root, &["commit", "-m", message])?;
    }
    Ok(())
}

/// `rspass git <args>` passthrough. `git init` is allowed on a non-repo
/// store (that's how you turn versioning on); everything else requires the
/// repo to exist.
pub fn passthrough(store_root: &Path, args: &[String]) -> Result<()> {
    let is_init = args.first().is_some_and(|a| a == "init");
    if !is_repo(store_root) && !is_init {
        bail!(
            "{} is not a git repository — try \"rspass git init\"",
            store_root.display()
        );
    }
    let status = Command::new("git")
        .arg("-C")
        .arg(store_root)
        .args(args)
        .status()
        .context("failed to run git")?;
    if !status.success() {
        bail!("git exited with {status}");
    }
    if is_init {
        // Fresh repo: record the current store contents as the first commit.
        commit(store_root, "Add current contents of password store.")?;
    }
    Ok(())
}
