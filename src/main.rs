#![deny(clippy::all)]
#![warn(clippy::pedantic)]
#![warn(clippy::nursery)]
#![deny(warnings)]

// The pedantic/nursery allow list, copied from rsconstruct's policy: every
// entry is a decision, not a backlog item. The bar for adding here is:
// clippy's preferred form is not clearly better, AND the lint fires broadly
// enough that per-site `#[allow]`s would be worse than one crate-level entry.

// Numeric casts on human-facing values (lengths, line numbers) where the
// range is known-small and a lossy cast is the intent.
#![allow(clippy::cast_possible_truncation)]

// Match arms are kept separate when they mean different things, even where
// they currently share a body.
#![allow(clippy::match_same_arms)]

// Suggests `map_or`/`map_or_else`, which is less readable than `if let`
// once the branches are more than an expression each.
#![allow(clippy::option_if_let_else)]

// Fires on the CLI dispatch match in main.rs and the per-command handlers.
// These are flat dispatch tables; splitting them produces indirection
// without reducing the amount to read.
#![allow(clippy::too_many_lines)]

// CLI argument structs are flag bags by nature — clap derives one bool per
// `--flag`. Grouping them into sub-structs to satisfy a 3-bool limit would
// obscure the one-field-per-option mapping that makes them readable.
#![allow(clippy::struct_excessive_bools)]

// Command handlers take many primitives because each maps 1:1 to a CLI flag.
#![allow(clippy::fn_params_excessive_bools)]

mod cli;
mod clipboard;
mod color;
mod commands;
mod generate;
mod gitops;
mod gpg;
mod platform;
mod runtime_flags;
mod store;
mod template;

use anyhow::Result;
use cli::Commands;
use store::Store;

fn main() -> std::process::ExitCode {
    platform::reset_sigpipe();

    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{} {:#}", color::red("Error:"), err);
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let cli = cli::parse_cli();

    // Initialize runtime flags from CLI arguments (once, before any reads).
    let color_enabled = match cli.color {
        cli::ColorMode::Always => true,
        cli::ColorMode::Never => false,
        cli::ColorMode::Auto => {
            // Disable if NO_COLOR is set to a non-empty value (per the
            // no-color.org spec, an empty value does NOT disable color)
            // or if stdout is not a tty.
            std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty())
                && std::io::IsTerminal::is_terminal(&std::io::stdout())
        }
    };
    runtime_flags::init(runtime_flags::RuntimeFlags {
        verbose: cli.verbose,
        quiet: cli.quiet,
        color_enabled,
    });

    let store = Store::locate(cli.store.as_deref())?;

    // Bare `rspass` lists the store, like bare `pass`.
    let command = cli.command.unwrap_or(Commands::Ls { subfolder: None });

    match command {
        Commands::Complete { shells } => {
            for shell in shells {
                cli::print_completions(shell);
            }
        }
        Commands::Cp { force, old_path, new_path } => {
            commands::mv_or_cp(&store, &old_path, &new_path, force, false)?;
        }
        Commands::Edit { pass_name } => {
            commands::edit(&store, &pass_name)?;
        }
        Commands::Find { pass_names } => {
            commands::find(&store, &pass_names)?;
        }
        Commands::Generate { no_symbols, clip, in_place, force, pass_name, length } => {
            commands::generate(&store, &pass_name, length, no_symbols, clip, in_place, force)?;
        }
        Commands::Git { args } => {
            gitops::passthrough(store.root(), &args)?;
        }
        Commands::Grep { ignore_case, search_string } => {
            commands::grep(&store, &search_string, ignore_case)?;
        }
        Commands::Init { path, gpg_ids } => {
            commands::init(&store, path.as_deref(), &gpg_ids)?;
        }
        Commands::Insert { echo, multiline, force, template, var, pass_name } => {
            commands::insert(&store, &pass_name, echo, multiline, force, template.as_deref(), &var)?;
        }
        Commands::Ls { subfolder } => {
            commands::ls(&store, subfolder.as_deref())?;
        }
        Commands::Mv { force, old_path, new_path } => {
            commands::mv_or_cp(&store, &old_path, &new_path, force, true)?;
        }
        Commands::Rm { recursive, force, pass_name } => {
            commands::rm(&store, &pass_name, recursive, force)?;
        }
        Commands::Show { clip, pass_name } => {
            commands::show(&store, clip, &pass_name)?;
        }
        Commands::Templates { action } => match action {
            cli::TemplatesAction::List => commands::templates_list(&store)?,
            cli::TemplatesAction::Show { name } => commands::templates_show(&store, &name)?,
        },
        Commands::Version => {
            println!("rspass {} by {}", env!("CARGO_PKG_VERSION"), env!("CARGO_PKG_AUTHORS"));
            println!("GIT_DESCRIBE: {}", env!("GIT_DESCRIBE"));
            println!("GIT_SHA: {}", env!("GIT_SHA"));
            println!("GIT_BRANCH: {}", env!("GIT_BRANCH"));
            println!("GIT_DIRTY: {}", env!("GIT_DIRTY"));
            println!("RUSTC_SEMVER: {}", env!("RUSTC_SEMVER"));
            println!("RUST_EDITION: {}", env!("RUST_EDITION"));
            println!("BUILD_TIMESTAMP: {}", env!("BUILD_TIMESTAMP"));
        }
    }
    Ok(())
}
