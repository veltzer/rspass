use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
use clap_complete::{generate, Shell};

#[derive(Parser)]
#[command(name = "rspass")]
#[command(version = concat!(env!("CARGO_PKG_VERSION")))]
#[command(about = "Rust password manager - a clone of pass(1), the standard unix password store", long_about = None)]
#[command(arg_required_else_help = true)]
pub struct Cli {
    /// Show extra detail (gpg commands, git commits, resolved paths)
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Suppress all output except errors and requested secrets
    #[arg(short, long, global = true)]
    pub quiet: bool,

    /// Password store directory (overrides `$PASSWORD_STORE_DIR` and the
    /// default ~/.password-store)
    #[arg(long, global = true, value_name = "DIR")]
    pub store: Option<String>,

    /// When to use ANSI color output: auto (tty only), always, or never.
    /// Also honored via the `NO_COLOR` env var (sets mode to never).
    #[arg(long, global = true, value_enum, default_value = "auto")]
    pub color: ColorMode,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum ColorMode {
    /// Enable color if stdout is a tty and `NO_COLOR` is not set
    Auto,
    /// Always emit ANSI color escapes
    Always,
    /// Never emit ANSI color escapes
    Never,
}

// Subcommand variants are kept in alphabetical order by their display name (kebab-case
// of the variant). Clap renders subcommands in declaration order, so this list IS the
// help output. Always insert new variants in alphabetical position.
#[derive(Subcommand)]
pub enum Commands {
    /// Generate shell completion scripts (no store needed)
    Complete {
        /// The shells to generate completions for
        #[arg(value_enum, required = true)]
        shells: Vec<Shell>,
    },
    /// Copy a password or directory to a new path (requires store)
    Cp {
        /// Overwrite the destination if it exists
        #[arg(short, long)]
        force: bool,
        /// Existing password name or directory
        old_path: String,
        /// New password name or directory
        new_path: String,
    },
    /// Edit a password with $EDITOR, re-encrypting on save (requires store)
    Edit {
        /// Name of the password to edit (created if missing)
        pass_name: String,
    },
    /// List passwords whose names match the given terms (requires store)
    Find {
        /// Search terms (case-insensitive substring match on names)
        #[arg(required = true)]
        pass_names: Vec<String>,
    },
    /// Generate a new password and store it encrypted (requires store)
    Generate {
        /// Use only alphanumeric characters (no symbols)
        #[arg(short, long)]
        no_symbols: bool,
        /// Copy the password to the clipboard instead of printing it
        #[arg(short, long)]
        clip: bool,
        /// Replace only the first line of an existing entry, keeping the rest
        #[arg(short, long, conflicts_with = "force")]
        in_place: bool,
        /// Overwrite an existing entry without asking
        #[arg(short, long)]
        force: bool,
        /// Name of the password to generate
        pass_name: String,
        /// Password length
        #[arg(default_value = "25")]
        length: usize,
    },
    /// Run a git command inside the password store (requires store)
    Git {
        /// Arguments passed through to git verbatim
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Search inside decrypted passwords for a string (requires store)
    Grep {
        /// Case-insensitive search
        #[arg(short, long)]
        ignore_case: bool,
        /// Text to search for
        search_string: String,
    },
    /// Initialize a new store or re-encrypt an existing one (no store needed)
    Init {
        /// Only apply to this subfolder of the store
        #[arg(short, long, value_name = "SUBFOLDER")]
        path: Option<String>,
        /// GPG key ids that entries are encrypted to
        #[arg(required = true)]
        gpg_ids: Vec<String>,
    },
    /// Insert a new password, prompting for its value (requires store)
    Insert {
        /// Echo the password as it is typed (default: hidden, typed twice)
        #[arg(short, long, conflicts_with = "multiline")]
        echo: bool,
        /// Read a multi-line entry from stdin until EOF
        #[arg(short, long)]
        multiline: bool,
        /// Overwrite an existing entry without asking
        #[arg(short, long)]
        force: bool,
        /// Render a Tera template from <store>/.templates/<NAME>.tera as the
        /// entry body instead of prompting
        #[arg(short, long, value_name = "NAME")]
        template: Option<String>,
        /// Template variable, KEY=VALUE (repeatable, only with --template)
        #[arg(long, value_name = "KEY=VALUE", requires = "template")]
        var: Vec<String>,
        /// Name of the password to insert
        pass_name: String,
    },
    /// List passwords as a tree (requires store)
    Ls {
        /// Subfolder to list
        subfolder: Option<String>,
    },
    /// Move a password or directory to a new path (requires store)
    Mv {
        /// Overwrite the destination if it exists
        #[arg(short, long)]
        force: bool,
        /// Existing password name or directory
        old_path: String,
        /// New password name or directory
        new_path: String,
    },
    /// Remove a password or directory from the store (requires store)
    Rm {
        /// Delete a whole directory recursively
        #[arg(short, long)]
        recursive: bool,
        /// Delete without asking for confirmation
        #[arg(short, long)]
        force: bool,
        /// Name of the password or directory to remove
        pass_name: String,
    },
    /// Decrypt and print a password (requires store)
    Show {
        /// Copy line N (default 1, the password) to the clipboard instead of
        /// printing anything
        #[arg(short, long, value_name = "LINE", num_args = 0..=1, default_missing_value = "1")]
        clip: Option<usize>,
        /// Name of the password to show
        pass_name: String,
    },
    /// Manage Tera entry templates in <store>/.templates (requires store)
    Templates {
        #[command(subcommand)]
        action: TemplatesAction,
    },
    /// Print version information (no store needed)
    Version,
}

#[derive(Subcommand)]
pub enum TemplatesAction {
    /// List available templates
    List,
    /// Print a template's source
    Show {
        /// Template name (without the .tera extension)
        name: String,
    },
}

/// Recursively set `hide_short_help = true` on all arguments in a command and its subcommands.
fn hide_all_flags(cmd: clap::Command) -> clap::Command {
    let cmd = cmd.mut_args(|arg| {
        if arg.get_long().is_some() || arg.get_short().is_some() {
            arg.hide_short_help(true)
        } else {
            arg
        }
    });
    cmd.mut_subcommands(hide_all_flags)
}

/// Parse CLI arguments with all flags hidden from short help (`-h`).
/// Use `--help` to see all flags.
pub fn parse_cli() -> Cli {
    let cmd = hide_all_flags(Cli::command());
    let matches = cmd.get_matches();
    Cli::from_arg_matches(&matches).expect("failed to parse CLI arguments")
}

/// Generate shell completions and print to stdout.
pub fn print_completions(shell: Shell) {
    let mut cmd = Cli::command();
    generate(shell, &mut cmd, "rspass", &mut std::io::stdout());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Completion scripts for the common shells must generate without error.
    #[test]
    fn completion_scripts_generate() {
        for shell in [Shell::Bash, Shell::Zsh, Shell::Fish] {
            let mut cmd = Cli::command();
            let mut buf = Vec::new();
            generate(shell, &mut cmd, "rspass", &mut buf);
            assert!(!buf.is_empty(), "empty completion script for {shell}");
        }
    }

    #[test]
    fn cli_asserts() {
        Cli::command().debug_assert();
    }
}
