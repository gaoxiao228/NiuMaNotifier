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
    CodexSessions,
    CodexSend(CodexSendCommand),
    CodexInterrupt(CodexInterruptCommand),
    Claude(ClaudeCommand),
    ClaudeSessions,
    ClaudeSend(ClaudeSendCommand),
    ClaudeInterrupt(ClaudeInterruptCommand),
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
pub(crate) struct CodexSendCommand {
    pub(crate) wrapper_session_id: String,
    #[arg(allow_hyphen_values = true)]
    pub(crate) message: String,
}

#[derive(Args)]
pub(crate) struct CodexInterruptCommand {
    pub(crate) wrapper_session_id: String,
}

#[derive(Args)]
#[command(disable_help_flag = true, disable_version_flag = true)]
pub(crate) struct ClaudeCommand {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub(crate) args: Vec<String>,
}

#[derive(Args)]
pub(crate) struct ClaudeSendCommand {
    pub(crate) wrapper_session_id: String,
    #[arg(allow_hyphen_values = true)]
    pub(crate) message: String,
}

#[derive(Args)]
pub(crate) struct ClaudeInterruptCommand {
    pub(crate) wrapper_session_id: String,
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
    ClaudeCode,
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

    fn parse_claude_args(argv: &[&str]) -> Vec<String> {
        let cli = Cli::try_parse_from(argv).unwrap();
        match cli.command.unwrap() {
            Command::Claude(command) => command.args,
            _ => panic!("expected claude command"),
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

    #[test]
    fn parses_codex_sessions_as_top_level_command() {
        let cli = Cli::try_parse_from(["niuma", "codex-sessions"]).unwrap();

        assert!(matches!(cli.command.unwrap(), Command::CodexSessions));
    }

    #[test]
    fn parses_codex_send_with_session_and_message() {
        let cli = Cli::try_parse_from(["niuma", "codex-send", "niuma_codex_1", "继续"]).unwrap();

        match cli.command.unwrap() {
            Command::CodexSend(command) => {
                assert_eq!(command.wrapper_session_id, "niuma_codex_1");
                assert_eq!(command.message, "继续");
            }
            _ => panic!("expected codex-send command"),
        }
    }

    #[test]
    fn parses_codex_send_message_that_starts_with_hyphen() {
        let cli = Cli::try_parse_from(["niuma", "codex-send", "niuma_codex_1", "--继续"]).unwrap();

        match cli.command.unwrap() {
            Command::CodexSend(command) => assert_eq!(command.message, "--继续"),
            _ => panic!("expected codex-send command"),
        }
    }

    #[test]
    fn parses_codex_interrupt_with_session() {
        let cli = Cli::try_parse_from(["niuma", "codex-interrupt", "niuma_codex_1"]).unwrap();

        match cli.command.unwrap() {
            Command::CodexInterrupt(command) => {
                assert_eq!(command.wrapper_session_id, "niuma_codex_1");
            }
            _ => panic!("expected codex-interrupt command"),
        }
    }

    #[test]
    fn parses_claude_flags_as_trailing_args() {
        assert_eq!(
            parse_claude_args(&["niuma", "claude", "--model", "sonnet"]),
            vec!["--model".to_string(), "sonnet".to_string()]
        );
    }

    #[test]
    fn parses_claude_send_and_interrupt_commands() {
        let cli = Cli::try_parse_from(["niuma", "claude-send", "niuma_claude_1", "继续"]).unwrap();
        match cli.command.unwrap() {
            Command::ClaudeSend(command) => {
                assert_eq!(command.wrapper_session_id, "niuma_claude_1");
                assert_eq!(command.message, "继续");
            }
            _ => panic!("expected claude-send command"),
        }

        let cli = Cli::try_parse_from(["niuma", "claude-interrupt", "niuma_claude_1"]).unwrap();
        match cli.command.unwrap() {
            Command::ClaudeInterrupt(command) => {
                assert_eq!(command.wrapper_session_id, "niuma_claude_1");
            }
            _ => panic!("expected claude-interrupt command"),
        }
    }
}
