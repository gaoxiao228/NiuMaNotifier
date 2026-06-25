use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(name = "niuma")]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Option<Command>,
}

#[derive(Subcommand)]
pub(crate) enum Command {
    Doctor,
    Status { tool: Option<ToolArg> },
    Codex(CodexCommand),
    Hook(HookCommand),
    Internal(InternalRootCommand),
    SampleEvent,
    Reset,
    DismissBlocker,
    Serve,
}

#[derive(Args)]
#[command(disable_help_flag = true, disable_version_flag = true)]
pub(crate) struct CodexCommand {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub(crate) args: Vec<String>,
}

#[derive(Args)]
pub(crate) struct HookCommand {
    #[arg(value_enum)]
    pub(crate) tool: ToolArg,
    #[arg(long)]
    pub(crate) install: bool,
    #[arg(long)]
    pub(crate) uninstall: bool,
    #[arg(long)]
    pub(crate) doctor: bool,
}

#[derive(Args)]
pub(crate) struct InternalRootCommand {
    #[command(subcommand)]
    pub(crate) command: InternalCommand,
}

#[derive(Subcommand)]
pub(crate) enum InternalCommand {
    Hook {
        #[arg(value_enum)]
        tool: ToolArg,
        #[arg(long)]
        source: Option<String>,
    },
}

#[derive(Clone, ValueEnum)]
pub(crate) enum ToolArg {
    Codex,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn parse_codex_args(argv: &[&str]) -> Vec<String> {
        let cli = Cli::try_parse_from(argv).unwrap();
        match cli.command.unwrap() {
            Command::Codex(command) => command.args,
            _ => panic!("expected codex command"),
        }
    }

    #[test]
    fn parses_codex_flags_as_trailing_args() {
        assert_eq!(
            parse_codex_args(&["niuma", "codex", "--model", "gpt-5"]),
            vec!["--model".to_string(), "gpt-5".to_string()]
        );
        assert_eq!(
            parse_codex_args(&["niuma", "codex", "-c", "model=gpt-5"]),
            vec!["-c".to_string(), "model=gpt-5".to_string()]
        );
    }

    #[test]
    fn preserves_hyphen_values_after_codex_subcommand() {
        assert_eq!(
            parse_codex_args(&["niuma", "codex", "exec", "--help"]),
            vec!["exec".to_string(), "--help".to_string()]
        );
    }

    #[test]
    fn preserves_codex_help_and_version_flags() {
        assert_eq!(
            parse_codex_args(&["niuma", "codex", "--help"]),
            vec!["--help".to_string()]
        );
        assert_eq!(
            parse_codex_args(&["niuma", "codex", "-h"]),
            vec!["-h".to_string()]
        );
        assert_eq!(
            parse_codex_args(&["niuma", "codex", "--version"]),
            vec!["--version".to_string()]
        );
        assert_eq!(
            parse_codex_args(&["niuma", "codex", "-V"]),
            vec!["-V".to_string()]
        );
    }
}
