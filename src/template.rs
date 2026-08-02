//! Tera entry templates, following rsconstruct's templating pattern: each
//! render gets a fresh `Tera` instance, custom functions are registered on
//! it, and sibling templates are added so `{% include %}` resolves.
//!
//! Templates live in `<store>/.templates/*.tera`. `rspass insert
//! --template login --var user=alice web/example` renders the template as
//! the entry body; `--var KEY=VALUE` values land in the Tera context, and
//! the built-in functions below are available:
//!
//! - `gen_password(length=25, symbols=true)` — a fresh random password
//! - `now(format="%Y-%m-%d")` — the current local time
//!
//! Example `.templates/login.tera`:
//!
//! ```text
//! {{ gen_password(length=32) }}
//! user: {{ user }}
//! created: {{ now() }}
//! ```

use anyhow::{Context as AnyhowContext, Result, bail};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use tera::{Context as TeraContext, Function, Tera, Value as TeraValue, to_value};

use crate::generate::generate_password;
use crate::store::{Store, TEMPLATES_DIR};

/// `gen_password(length=25, symbols=true)` — generate a random password.
struct GenPasswordFunction;

impl Function for GenPasswordFunction {
    fn call(&self, args: &HashMap<String, TeraValue>) -> tera::Result<TeraValue> {
        let length = match args.get("length") {
            Some(v) => v
                .as_u64()
                .ok_or_else(|| tera::Error::msg("gen_password: length must be a positive integer"))?
                as usize,
            None => 25,
        };
        if length == 0 {
            return Err(tera::Error::msg("gen_password: length must be at least 1"));
        }
        let symbols = match args.get("symbols") {
            Some(v) => v
                .as_bool()
                .ok_or_else(|| tera::Error::msg("gen_password: symbols must be a boolean"))?,
            None => true,
        };
        let password = generate_password(length, !symbols)
            .map_err(|e| tera::Error::msg(format!("gen_password: {e}")))?;
        Ok(to_value(password)?)
    }
}

/// `now(format="%Y-%m-%d")` — current local time, chrono strftime format.
struct NowFunction;

impl Function for NowFunction {
    fn call(&self, args: &HashMap<String, TeraValue>) -> tera::Result<TeraValue> {
        let format = match args.get("format") {
            Some(v) => v
                .as_str()
                .ok_or_else(|| tera::Error::msg("now: format must be a string"))?,
            None => "%Y-%m-%d",
        };
        Ok(to_value(chrono::Local::now().format(format).to_string())?)
    }
}

fn templates_dir(store: &Store) -> PathBuf {
    store.root().join(TEMPLATES_DIR)
}

fn template_path(store: &Store, name: &str) -> Result<PathBuf> {
    Store::check_sneaky(name)?;
    Ok(templates_dir(store).join(format!("{name}.tera")))
}

/// List template names (without the .tera extension), sorted.
pub fn list(store: &Store) -> Result<Vec<String>> {
    let dir = templates_dir(store);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut names = Vec::new();
    for entry in fs::read_dir(&dir).with_context(|| format!("failed to read {}", dir.display()))? {
        let path = entry.context("failed to read templates directory entry")?.path();
        if path.is_file() && path.extension().is_some_and(|e| e == "tera") {
            names.push(path.file_stem().unwrap_or_default().to_string_lossy().into_owned());
        }
    }
    names.sort();
    Ok(names)
}

/// Read a template's source text.
pub fn source(store: &Store, name: &str) -> Result<String> {
    let path = template_path(store, name)?;
    if !path.is_file() {
        bail!("no template named {name} in {}", templates_dir(store).display());
    }
    fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))
}

/// Render a template with `--var KEY=VALUE` variables into an entry body.
pub fn render(store: &Store, name: &str, vars: &[String]) -> Result<String> {
    let main_source = source(store, name)?;

    // Fresh Tera instance per render, functions registered on it — same
    // shape as rsconstruct's render_template.
    let mut tera = Tera::default();
    tera.register_function("gen_password", GenPasswordFunction);
    tera.register_function("now", NowFunction);
    // No HTML escaping: entries are plain text.
    tera.set_escape_fn(std::string::ToString::to_string);

    // Register every sibling template so {% include "other.tera" %} works.
    let dir = templates_dir(store);
    for other in list(store)? {
        if other == name {
            continue;
        }
        let path = dir.join(format!("{other}.tera"));
        let content =
            fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
        tera.add_raw_template(&format!("{other}.tera"), &content)
            .with_context(|| format!("failed to parse template {}", path.display()))?;
    }
    tera.add_raw_template("entry", &main_source)
        .with_context(|| format!("failed to parse template {name}"))?;

    let mut context = TeraContext::new();
    for var in vars {
        let (key, value) = var
            .split_once('=')
            .with_context(|| format!("invalid --var {var:?} — expected KEY=VALUE"))?;
        context.insert(key, value);
    }

    let mut rendered = tera
        .render("entry", &context)
        .with_context(|| format!("failed to render template {name}"))?;
    if !rendered.ends_with('\n') {
        rendered.push('\n');
    }
    Ok(rendered)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_with_template(body: &str) -> (tempfile::TempDir, Store) {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::locate(Some(tmp.path().to_str().unwrap())).unwrap();
        let dir = tmp.path().join(TEMPLATES_DIR);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("login.tera"), body).unwrap();
        (tmp, store)
    }

    #[test]
    fn renders_vars_and_functions() {
        let (_tmp, store) = store_with_template("{{ gen_password(length=12, symbols=false) }}\nuser: {{ user }}\n");
        let out = render(&store, "login", &["user=alice".to_owned()]).unwrap();
        let mut lines = out.lines();
        let password = lines.next().unwrap();
        assert_eq!(password.len(), 12);
        assert!(password.chars().all(|c| c.is_ascii_alphanumeric()));
        assert_eq!(lines.next().unwrap(), "user: alice");
    }

    #[test]
    fn undefined_variable_fails() {
        let (_tmp, store) = store_with_template("user: {{ user }}\n");
        assert!(render(&store, "login", &[]).is_err());
    }

    #[test]
    fn lists_templates_sorted() {
        let (tmp, store) = store_with_template("x\n");
        fs::write(tmp.path().join(TEMPLATES_DIR).join("api.tera"), "y\n").unwrap();
        assert_eq!(list(&store).unwrap(), vec!["api", "login"]);
    }
}
