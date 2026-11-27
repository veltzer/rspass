//! Unix-specific OS operations, isolated behind named wrappers.
//!
//! rspass is Linux-only (the platform the release matrix builds). This
//! module keeps `std::os::unix` imports and `libc` calls out
//! of the rest of the codebase.

/// Reset SIGPIPE to default behavior so piping to head/less doesn't cause errors.
pub fn reset_sigpipe() {
    // SAFETY: signal(2) with SIG_DFL installs no handler, so there is no
    // callback to be sound about; it runs first thing in main(), before any
    // thread exists.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

/// Restrict a path to owner-only access (0700 for dirs, 0600 for files).
/// The password store must never be group/world readable.
pub fn restrict_permissions(path: &std::path::Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}
