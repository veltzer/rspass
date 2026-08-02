//! One handler per CLI subcommand. `main.rs` is the flat dispatch table;
//! everything that actually does work lives here.

use anyhow::{Context, Result, bail};
use std::fs;
use std::io::{BufRead, IsTerminal, Read, Write};
use std::path::Path;
use std::process::Command;

use crate::color;
use crate::gitops;
use crate::gpg;
use crate::runtime_flags;
use crate::store::Store;
use crate::template;

fn say(msg: &str) {
    if !runtime_flags::quiet() {
        println!("{msg}");
    }
}

/// Ask a yes/no question on the terminal. Non-tty stdin answers "no" so
/// scripts never hang; they should pass --force instead.
fn confirm(question: &str) -> Result<bool> {
    let stdin = std::io::stdin();
    if !stdin.is_terminal() {
        return Ok(false);
    }
    eprint!("{question} [y/N] ");
    std::io::stderr().flush().ok();
    let mut answer = String::new();
    stdin.lock().read_line(&mut answer).context("failed to read answer")?;
    Ok(matches!(answer.trim(), "y" | "Y" | "yes" | "Yes"))
}

/// Refuse to clobber an existing entry unless --force was given or the user
/// confirms interactively.
fn check_overwrite(path: &Path, name: &str, force: bool) -> Result<()> {
    if path.exists() && !force && !confirm(&format!("An entry already exists for {name}. Overwrite it?"))? {
        bail!("not overwriting {name}");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// init
// ---------------------------------------------------------------------------

pub fn init(store: &Store, path: Option<&str>, gpg_ids: &[String]) -> Result<()> {
    let target_dir = match path {
        Some(sub) => store.dir_path(sub)?,
        None => store.root().to_path_buf(),
    };
    fs::create_dir_all(&target_dir)
        .with_context(|| format!("failed to create {}", target_dir.display()))?;
    crate::platform::restrict_permissions(store.root(), 0o700)
        .with_context(|| format!("failed to set permissions on {}", store.root().display()))?;

    let gpg_id_file = target_dir.join(".gpg-id");
    fs::write(&gpg_id_file, format!("{}\n", gpg_ids.join("\n")))
        .with_context(|| format!("failed to write {}", gpg_id_file.display()))?;
    say(&format!(
        "Password store initialized for {} ({})",
        gpg_ids.join(", "),
        target_dir.display()
    ));

    // Re-encrypt everything in scope to the new recipients, like pass does.
    // list_entries returns names relative to the store root even when a
    // subfolder is given, so they can be fed straight back to entry_path.
    let entries = store.list_entries(path)?;
    for name in &entries {
        let entry_path = store.entry_path(name)?;
        let plaintext = gpg::decrypt(&entry_path)?;
        gpg::encrypt(&plaintext, &entry_path, &store.gpg_ids_for(name)?)?;
        if runtime_flags::verbose() {
            eprintln!("re-encrypted {name}");
        }
    }
    if !entries.is_empty() {
        say(&format!("Re-encrypted {} entries.", entries.len()));
    }

    gitops::commit(
        store.root(),
        &format!("Set GPG ids to {} ({}).", gpg_ids.join(", "), path.unwrap_or("store root")),
    )
}

// ---------------------------------------------------------------------------
// show / ls / find / grep
// ---------------------------------------------------------------------------

pub fn show(store: &Store, clip: Option<usize>, pass_name: &str) -> Result<()> {
    store.require_initialized()?;
    let entry = store.entry_path(pass_name)?;
    if !entry.is_file() {
        // pass(1) behavior: `pass show <dir>` lists the directory.
        if store.dir_path(pass_name)?.is_dir() {
            return ls(store, Some(pass_name));
        }
        bail!("{pass_name} is not in the password store");
    }
    let plaintext = gpg::decrypt(&entry)?;
    match clip {
        None => print!("{plaintext}"),
        Some(line_no) => {
            let line = plaintext
                .lines()
                .nth(line_no.saturating_sub(1))
                .with_context(|| format!("there is no line {line_no} in {pass_name}"))?;
            if line.is_empty() {
                bail!("line {line_no} of {pass_name} is empty");
            }
            let seconds = crate::clipboard::copy(line)?;
            say(&format!("Copied {pass_name} to clipboard. Will clear in {seconds} seconds."));
        }
    }
    Ok(())
}

pub fn ls(store: &Store, subfolder: Option<&str>) -> Result<()> {
    store.require_initialized()?;
    for line in store.tree(subfolder)? {
        println!("{line}");
    }
    Ok(())
}

pub fn find(store: &Store, terms: &[String]) -> Result<()> {
    store.require_initialized()?;
    let lowered: Vec<String> = terms.iter().map(|t| t.to_lowercase()).collect();
    say(&format!("Search terms: {}", terms.join(", ")));
    let mut found = false;
    for name in store.list_entries(None)? {
        let hay = name.to_lowercase();
        if lowered.iter().any(|t| hay.contains(t)) {
            println!("{name}");
            found = true;
        }
    }
    if !found {
        bail!("no matching entries found");
    }
    Ok(())
}

pub fn grep(store: &Store, search: &str, ignore_case: bool) -> Result<()> {
    store.require_initialized()?;
    let needle = if ignore_case { search.to_lowercase() } else { search.to_owned() };
    for name in store.list_entries(None)? {
        let plaintext = gpg::decrypt(&store.entry_path(&name)?)?;
        let mut header_printed = false;
        for line in plaintext.lines() {
            let hay = if ignore_case { line.to_lowercase() } else { line.to_owned() };
            if hay.contains(&needle) {
                if !header_printed {
                    println!("{}:", color::bold_blue(&name));
                    header_printed = true;
                }
                println!("{line}");
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// insert / edit / generate
// ---------------------------------------------------------------------------

pub fn insert(
    store: &Store,
    pass_name: &str,
    echo: bool,
    multiline: bool,
    force: bool,
    tpl: Option<&str>,
    vars: &[String],
) -> Result<()> {
    store.require_initialized()?;
    let entry = store.entry_path(pass_name)?;
    check_overwrite(&entry, pass_name, force)?;

    let body = if let Some(tpl_name) = tpl {
        template::render(store, tpl_name, vars)?
    } else if multiline {
        say(&format!("Enter contents of {pass_name} and press Ctrl+D when finished:"));
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf).context("failed to read stdin")?;
        buf
    } else if echo || !std::io::stdin().is_terminal() {
        // Non-tty stdin (piped input) reads one line, like `pass insert -e`.
        if echo && std::io::stdin().is_terminal() {
            eprint!("Enter password for {pass_name}: ");
            std::io::stderr().flush().ok();
        }
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line).context("failed to read password")?;
        format!("{}\n", line.trim_end_matches('\n'))
    } else {
        let first = rpassword::prompt_password(format!("Enter password for {pass_name}: "))
            .context("failed to read password")?;
        let second = rpassword::prompt_password(format!("Retype password for {pass_name}: "))
            .context("failed to read password")?;
        if first != second {
            bail!("the entered passwords do not match");
        }
        format!("{first}\n")
    };

    gpg::encrypt(&body, &entry, &store.gpg_ids_for(pass_name)?)?;
    gitops::commit(store.root(), &format!("Add given password for {pass_name} to store."))?;
    if tpl.is_some() {
        say(&format!("Created {pass_name} from template {}.", tpl.unwrap_or_default()));
    }
    Ok(())
}

pub fn edit(store: &Store, pass_name: &str) -> Result<()> {
    store.require_initialized()?;
    let entry = store.entry_path(pass_name)?;
    let existing = if entry.is_file() { Some(gpg::decrypt(&entry)?) } else { None };

    // Prefer /dev/shm so plaintext never touches a disk-backed filesystem.
    let tmp_dir = if Path::new("/dev/shm").is_dir() {
        tempfile::Builder::new().prefix("rspass-edit").tempdir_in("/dev/shm")
    } else {
        tempfile::Builder::new().prefix("rspass-edit").tempdir()
    }
    .context("failed to create temporary directory")?;
    let tmp_file = tmp_dir.path().join(
        Path::new(pass_name).file_name().context("invalid pass name")?,
    );
    fs::write(&tmp_file, existing.as_deref().unwrap_or(""))
        .context("failed to write temporary file")?;
    crate::platform::restrict_permissions(&tmp_file, 0o600)?;

    // Run through sh so $EDITOR values with arguments ("code --wait") work.
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_owned());
    let status = Command::new("sh")
        .arg("-c")
        .arg(format!("{editor} \"$1\""))
        .arg(&editor)
        .arg(&tmp_file)
        .status()
        .with_context(|| format!("failed to run editor {editor}"))?;
    if !status.success() {
        bail!("editor {editor} exited with {status}");
    }

    let new_content = fs::read_to_string(&tmp_file).context("failed to read edited file")?;
    if new_content.is_empty() {
        bail!("empty file — {pass_name} unchanged");
    }
    if existing.as_deref() == Some(new_content.as_str()) {
        say(&format!("{pass_name} unchanged."));
        return Ok(());
    }
    gpg::encrypt(&new_content, &entry, &store.gpg_ids_for(pass_name)?)?;
    let action = if existing.is_some() { "Edit" } else { "Add" };
    gitops::commit(store.root(), &format!("{action} password for {pass_name} using {editor}."))
}

pub fn generate(
    store: &Store,
    pass_name: &str,
    length: usize,
    no_symbols: bool,
    clip: bool,
    in_place: bool,
    force: bool,
) -> Result<()> {
    store.require_initialized()?;
    if length == 0 {
        bail!("password length must be at least 1");
    }
    let entry = store.entry_path(pass_name)?;
    if !in_place {
        check_overwrite(&entry, pass_name, force)?;
    }
    let password = crate::generate::generate_password(length, no_symbols)?;

    let body = if in_place {
        if !entry.is_file() {
            bail!("{pass_name} is not in the password store — cannot use --in-place");
        }
        let existing = gpg::decrypt(&entry)?;
        let rest: Vec<&str> = existing.lines().skip(1).collect();
        if rest.is_empty() {
            format!("{password}\n")
        } else {
            format!("{password}\n{}\n", rest.join("\n"))
        }
    } else {
        format!("{password}\n")
    };
    gpg::encrypt(&body, &entry, &store.gpg_ids_for(pass_name)?)?;

    let verb = if in_place { "Replace" } else { "Add" };
    gitops::commit(store.root(), &format!("{verb} generated password for {pass_name}."))?;

    if clip {
        let seconds = crate::clipboard::copy(&password)?;
        say(&format!("Copied {pass_name} to clipboard. Will clear in {seconds} seconds."));
    } else {
        say(&format!("The generated password for {} is:", color::bold(pass_name)));
        println!("{}", color::yellow(&password));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// rm / mv / cp
// ---------------------------------------------------------------------------

pub fn rm(store: &Store, pass_name: &str, recursive: bool, force: bool) -> Result<()> {
    store.require_initialized()?;
    let entry = store.entry_path(pass_name)?;
    let dir = store.dir_path(pass_name)?;

    let (target, is_dir) = if entry.is_file() {
        (entry, false)
    } else if dir.is_dir() {
        if !recursive {
            bail!("{pass_name} is a directory — use --recursive to delete it");
        }
        (dir, true)
    } else {
        bail!("{pass_name} is not in the password store");
    };

    if !force && !confirm(&format!("Are you sure you would like to delete {pass_name}?"))? {
        bail!("not deleting {pass_name}");
    }
    if is_dir {
        fs::remove_dir_all(&target).with_context(|| format!("failed to remove {}", target.display()))?;
    } else {
        fs::remove_file(&target).with_context(|| format!("failed to remove {}", target.display()))?;
        remove_empty_parents(target.parent(), store.root());
    }
    say(&format!("Removed {pass_name}."));
    gitops::commit(store.root(), &format!("Remove {pass_name} from store."))
}

/// After deleting an entry, sweep now-empty parent directories up to the
/// store root, like pass's rmdir -p.
fn remove_empty_parents(mut dir: Option<&Path>, root: &Path) {
    while let Some(d) = dir {
        if d == root || !d.starts_with(root) {
            break;
        }
        if fs::remove_dir(d).is_err() {
            break; // not empty (or gone) — stop
        }
        dir = d.parent();
    }
}

pub fn mv_or_cp(store: &Store, old: &str, new: &str, force: bool, is_move: bool) -> Result<()> {
    store.require_initialized()?;
    let old_entry = store.entry_path(old)?;
    let old_dir = store.dir_path(old)?;

    if old_entry.is_file() {
        // Destination ending in '/' targets a directory, like mv(1).
        let new_name = if new.ends_with('/') {
            format!("{new}{}", Path::new(old).file_name().unwrap_or_default().to_string_lossy())
        } else {
            new.to_owned()
        };
        let new_entry = store.entry_path(&new_name)?;
        check_overwrite(&new_entry, &new_name, force)?;
        fs::create_dir_all(new_entry.parent().context("destination has no parent")?)
            .context("failed to create destination directory")?;
        if is_move {
            fs::rename(&old_entry, &new_entry)
                .with_context(|| format!("failed to move {old} to {new_name}"))?;
            remove_empty_parents(old_entry.parent(), store.root());
        } else {
            fs::copy(&old_entry, &new_entry)
                .with_context(|| format!("failed to copy {old} to {new_name}"))?;
        }
        reencrypt_if_recipients_differ(store, old, &new_name)?;
    } else if old_dir.is_dir() {
        let new_dir = store.dir_path(new)?;
        if new_dir.exists() && !force {
            bail!("{new} already exists — use --force to overwrite");
        }
        copy_dir(&old_dir, &new_dir)?;
        if is_move {
            fs::remove_dir_all(&old_dir)
                .with_context(|| format!("failed to remove {}", old_dir.display()))?;
        }
    } else {
        bail!("{old} is not in the password store");
    }

    let verb = if is_move { "Rename" } else { "Copy" };
    say(&format!("{verb} {old} to {new}."));
    gitops::commit(store.root(), &format!("{verb} {old} to {new}."))
}

/// pass(1) re-encrypts moved entries when the destination resolves to a
/// different .gpg-id set. Compare recipient lists and re-encrypt on change.
fn reencrypt_if_recipients_differ(store: &Store, old: &str, new: &str) -> Result<()> {
    let old_ids = store.gpg_ids_for(old)?;
    let new_ids = store.gpg_ids_for(new)?;
    if old_ids != new_ids {
        let path = store.entry_path(new)?;
        let plaintext = gpg::decrypt(&path)?;
        gpg::encrypt(&plaintext, &path, &new_ids)?;
        if runtime_flags::verbose() {
            eprintln!("re-encrypted {new} for {}", new_ids.join(", "));
        }
    }
    Ok(())
}

fn copy_dir(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst).with_context(|| format!("failed to create {}", dst.display()))?;
    for entry in fs::read_dir(src).with_context(|| format!("failed to read {}", src.display()))? {
        let entry = entry.context("failed to read directory entry")?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir(&from, &to)?;
        } else {
            fs::copy(&from, &to)
                .with_context(|| format!("failed to copy {} to {}", from.display(), to.display()))?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// templates
// ---------------------------------------------------------------------------

pub fn templates_list(store: &Store) -> Result<()> {
    store.require_initialized()?;
    let names = template::list(store)?;
    if names.is_empty() {
        say(&format!(
            "No templates. Put Tera templates in {}/{}/<name>.tera",
            store.root().display(),
            crate::store::TEMPLATES_DIR
        ));
        return Ok(());
    }
    for name in names {
        println!("{name}");
    }
    Ok(())
}

pub fn templates_show(store: &Store, name: &str) -> Result<()> {
    store.require_initialized()?;
    print!("{}", template::source(store, name)?);
    Ok(())
}
