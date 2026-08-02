use std::borrow::Cow;

/// Color is enabled based on the global runtime flag (--color auto|always|never).
/// Default resolution: off unless `--color always`, or `--color auto` with a tty
/// stdout and no `NO_COLOR` env var set.
fn no_color() -> bool {
    !crate::runtime_flags::color_enabled()
}

fn wrap<'a>(code: &str, text: &'a str) -> Cow<'a, str> {
    if no_color() {
        Cow::Borrowed(text)
    } else {
        Cow::Owned(format!("\x1b[{code}m{text}\x1b[0m"))
    }
}

pub fn red(text: &str) -> Cow<'_, str> {
    wrap("31", text)
}

pub fn yellow(text: &str) -> Cow<'_, str> {
    wrap("33", text)
}

pub fn bold(text: &str) -> Cow<'_, str> {
    wrap("1", text)
}

pub fn bold_blue(text: &str) -> Cow<'_, str> {
    wrap("1;34", text)
}
