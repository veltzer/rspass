//! End-to-end tests driving the real binary. Encryption tests create a
//! throwaway passphrase-less GPG key in an isolated GNUPGHOME so they never
//! touch the user's keyring, and skip cleanly when gpg is not installed.

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::path::Path;
use std::sync::OnceLock;
use tempfile::TempDir;

const TEST_KEY: &str = "rspass-test@example.com";

/// Shared gpg home with one generated test key. None when gpg is missing.
fn gpg_home() -> Option<&'static Path> {
    static HOME: OnceLock<Option<TempDir>> = OnceLock::new();
    HOME.get_or_init(|| {
        which::which("gpg").ok()?;
        let dir = TempDir::new().expect("create gpg home");
        // gpg refuses group/world-accessible homedirs.
        std::process::Command::new("chmod")
            .arg("700")
            .arg(dir.path())
            .status()
            .expect("chmod gpg home");
        let status = std::process::Command::new("gpg")
            .env("GNUPGHOME", dir.path())
            .args(["--batch", "--passphrase", "", "--quick-generate-key", TEST_KEY, "default", "default", "never"])
            .status()
            .expect("run gpg");
        status.success().then_some(dir)
    })
    .as_ref()
    .map(TempDir::path)
}

/// An rspass Command pointed at `store` with gpg isolated to the test home.
fn rspass(store: &Path, gpg_home: &Path) -> Command {
    let mut cmd = Command::cargo_bin("rspass").expect("rspass binary");
    cmd.env("PASSWORD_STORE_DIR", store)
        .env("GNUPGHOME", gpg_home)
        .env_remove("PASSWORD_STORE_GPG_OPTS");
    cmd
}

#[test]
fn version_prints_build_info() {
    Command::cargo_bin("rspass")
        .unwrap()
        .arg("version")
        .assert()
        .success()
        .stdout(predicate::str::contains("rspass").and(predicate::str::contains("GIT_SHA")));
}

#[test]
fn bare_invocation_shows_subcommands() {
    // With no arguments clap treats it as a usage error: the subcommand
    // list goes to stderr and the exit code is non-zero.
    let assert = Command::cargo_bin("rspass").unwrap().assert().failure();
    let out = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    for sub in ["init", "insert", "show", "generate", "ls"] {
        assert!(out.contains(sub), "bare rspass output is missing subcommand {sub}");
    }
}

#[test]
fn help_lists_all_subcommands() {
    let assert = Command::cargo_bin("rspass").unwrap().arg("--help").assert().success();
    let out = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    for sub in ["init", "insert", "show", "generate", "ls", "rm", "mv", "cp", "git", "grep", "find", "edit", "templates", "complete"] {
        assert!(out.contains(sub), "--help is missing subcommand {sub}");
    }
}

#[test]
fn completions_generate() {
    Command::cargo_bin("rspass")
        .unwrap()
        .args(["complete", "bash", "zsh", "fish"])
        .assert()
        .success()
        .stdout(predicate::str::contains("rspass"));
}

#[test]
fn uninitialized_store_gives_clear_error() {
    let store = TempDir::new().unwrap();
    Command::cargo_bin("rspass")
        .unwrap()
        .env("PASSWORD_STORE_DIR", store.path())
        .arg("ls")
        .assert()
        .failure()
        .stderr(predicate::str::contains("rspass init"));
}

#[test]
fn sneaky_paths_are_rejected() {
    let store = TempDir::new().unwrap();
    fs::write(store.path().join(".gpg-id"), "whatever\n").unwrap();
    Command::cargo_bin("rspass")
        .unwrap()
        .env("PASSWORD_STORE_DIR", store.path())
        .args(["show", "../../etc/passwd"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("sneaky"));
}

#[test]
fn init_insert_show_roundtrip() {
    let Some(gpg) = gpg_home() else {
        eprintln!("gpg not installed — skipping");
        return;
    };
    let store = TempDir::new().unwrap();

    rspass(store.path(), gpg)
        .args(["init", TEST_KEY])
        .assert()
        .success()
        .stdout(predicate::str::contains("initialized"));

    rspass(store.path(), gpg)
        .args(["insert", "web/example"])
        .write_stdin("hunter2\n")
        .assert()
        .success();
    assert!(store.path().join("web/example.gpg").is_file());

    rspass(store.path(), gpg)
        .args(["show", "web/example"])
        .assert()
        .success()
        .stdout("hunter2\n");

    // ls: the tree shows the entry without its .gpg suffix.
    rspass(store.path(), gpg)
        .arg("ls")
        .assert()
        .success()
        .stdout(predicate::str::contains("Password Store").and(predicate::str::contains("example")));
}

#[test]
fn generate_rm_and_find() {
    let Some(gpg) = gpg_home() else {
        eprintln!("gpg not installed — skipping");
        return;
    };
    let store = TempDir::new().unwrap();
    rspass(store.path(), gpg).args(["init", TEST_KEY]).assert().success();

    rspass(store.path(), gpg)
        .args(["generate", "--no-symbols", "site/login", "32"])
        .assert()
        .success();
    let assert = rspass(store.path(), gpg).args(["show", "site/login"]).assert().success();
    let pw = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert_eq!(pw.trim_end().len(), 32);
    assert!(pw.trim_end().chars().all(|c| c.is_ascii_alphanumeric()));

    rspass(store.path(), gpg)
        .args(["find", "login"])
        .assert()
        .success()
        .stdout(predicate::str::contains("site/login"));

    rspass(store.path(), gpg)
        .args(["rm", "--force", "site/login"])
        .assert()
        .success();
    assert!(!store.path().join("site").exists(), "empty parent dir should be swept");
}

#[test]
fn insert_from_tera_template() {
    let Some(gpg) = gpg_home() else {
        eprintln!("gpg not installed — skipping");
        return;
    };
    let store = TempDir::new().unwrap();
    rspass(store.path(), gpg).args(["init", TEST_KEY]).assert().success();

    let tpl_dir = store.path().join(".templates");
    fs::create_dir_all(&tpl_dir).unwrap();
    fs::write(
        tpl_dir.join("login.tera"),
        "{{ gen_password(length=20, symbols=false) }}\nuser: {{ user }}\nurl: {{ url }}\n",
    )
    .unwrap();

    rspass(store.path(), gpg)
        .args(["templates", "list"])
        .assert()
        .success()
        .stdout("login\n");

    rspass(store.path(), gpg)
        .args(["insert", "--template", "login", "--var", "user=alice", "--var", "url=https://example.com", "web/tpl"])
        .assert()
        .success();

    let assert = rspass(store.path(), gpg).args(["show", "web/tpl"]).assert().success();
    let body = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let mut lines = body.lines();
    assert_eq!(lines.next().unwrap().len(), 20);
    assert_eq!(lines.next().unwrap(), "user: alice");
    assert_eq!(lines.next().unwrap(), "url: https://example.com");
}

#[test]
fn mv_and_cp_entries() {
    let Some(gpg) = gpg_home() else {
        eprintln!("gpg not installed — skipping");
        return;
    };
    let store = TempDir::new().unwrap();
    rspass(store.path(), gpg).args(["init", TEST_KEY]).assert().success();
    rspass(store.path(), gpg)
        .args(["insert", "a/one"])
        .write_stdin("secret-one\n")
        .assert()
        .success();

    rspass(store.path(), gpg).args(["cp", "a/one", "b/copy"]).assert().success();
    rspass(store.path(), gpg).args(["mv", "a/one", "c/moved"]).assert().success();

    rspass(store.path(), gpg).args(["show", "b/copy"]).assert().success().stdout("secret-one\n");
    rspass(store.path(), gpg).args(["show", "c/moved"]).assert().success().stdout("secret-one\n");
    assert!(!store.path().join("a").exists());
}
