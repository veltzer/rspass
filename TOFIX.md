# TOFIX

Findings from a code scan on 2026-10-04.

## Medium

- `src/commands.rs:452-461` - moving or copying a *directory* never re-encrypts its entries; only the single-entry branch calls `reencrypt_if_recipients_differ` (`commands.rs:451`). Moving `personal/` under a subfolder with a different `.gpg-id` leaves every entry readable only by the old recipients (pass(1) re-encrypts in this case); walk the copied entries and re-encrypt each one whose recipient set changed.
- `src/clipboard.rs:70-73` - the detached clearer wipes the clipboard unconditionally after the timeout, destroying whatever the user copied in the meantime; pass(1) only clears/restores when the clipboard still holds the secret. Check the clipboard content (or restore the previous content) before clearing.
- `src/commands.rs:487-502` - `copy_dir` recurses forever when the destination is inside the source (`rspass mv dir dir/sub` / `cp`): it creates `dst` first, then `read_dir(src)` sees it and copies it into itself until the path is too long. Reject a destination that starts with the source path before copying.

## Low

- `src/commands.rs:454-457` - with `--force` onto an existing directory, `copy_dir` merges into it instead of replacing it, so stale entries survive a "forced" move; remove the destination first (or document the merge).
- `src/template.rs:67` - `chrono::Local::now().format(format).to_string()` panics on an invalid strftime specifier (e.g. `now(format="%Q")`); format with `write!` into a `String` and turn the `fmt::Error` into a `tera::Error`.
- `src/gitops.rs:37` - `commit()` runs `git add -A`, so any unrelated pending change in the store is swept into the "Add given password for X" commit; stage only the paths the command touched, as pass(1) does.
- `docs/src/introduction.md:1-3` - the mdbook is a single sentence; the usage, template and store-location docs exist only in `README.md`. Move or copy them into the book so the published docs are useful.
