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

    match cli.command {
        Commands::Complete { shells } => {
            for shell in shells {
                cli::print_completions(shell);
            }
        }
        Commands::Cp {
            force,
            old_path,
            new_path,
        } => {
            commands::mv_or_cp(&store, &old_path, &new_path, force, false)?;
        }
        Commands::Edit { pass_name } => {
            commands::edit(&store, &pass_name)?;
        }
        Commands::Find { pass_names } => {
            commands::find(&store, &pass_names)?;
        }
        Commands::Generate {
            no_symbols,
            clip,
            in_place,
            force,
            pass_name,
            length,
        } => {
            commands::generate(
                &store, &pass_name, length, no_symbols, clip, in_place, force,
            )?;
        }
        Commands::Git { args } => {
            gitops::passthrough(store.root(), &args)?;
        }
        Commands::Grep {
            ignore_case,
            search_string,
        } => {
            commands::grep(&store, &search_string, ignore_case)?;
        }
        Commands::Init { path, gpg_ids } => {
            commands::init(&store, path.as_deref(), &gpg_ids)?;
        }
        Commands::Insert {
            echo,
            multiline,
            force,
            template,
            var,
            pass_name,
        } => {
            commands::insert(
                &store,
                &pass_name,
                echo,
                multiline,
                force,
                template.as_deref(),
                &var,
            )?;
        }
        Commands::Ls { subfolder } => {
            commands::ls(&store, subfolder.as_deref())?;
        }
        Commands::Mv {
            force,
            old_path,
            new_path,
        } => {
            commands::mv_or_cp(&store, &old_path, &new_path, force, true)?;
        }
        Commands::Rm {
            recursive,
            force,
            pass_name,
        } => {
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
            println!(
                "rspass {} by {}",
                env!("CARGO_PKG_VERSION"),
                env!("CARGO_PKG_AUTHORS")
            );
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
