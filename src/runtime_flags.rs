//! Global runtime flags, initialized once from CLI arguments in `main`
//! before any command runs, then read from anywhere without threading a
//! flags struct through every call chain.

use std::sync::OnceLock;

#[derive(Debug, Clone, Copy)]
pub struct RuntimeFlags {
    pub verbose: bool,
    pub quiet: bool,
    pub color_enabled: bool,
}

static FLAGS: OnceLock<RuntimeFlags> = OnceLock::new();

/// Initialize the runtime flags. Must be called exactly once, before any read.
pub fn init(flags: RuntimeFlags) {
    FLAGS.set(flags).expect("runtime flags initialized twice");
}

fn get() -> RuntimeFlags {
    FLAGS.get().copied().unwrap_or(RuntimeFlags {
        verbose: false,
        quiet: false,
        color_enabled: false,
    })
}

pub fn color_enabled() -> bool {
    get().color_enabled
}

pub fn verbose() -> bool {
    get().verbose
}

pub fn quiet() -> bool {
    get().quiet
}
