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
    Hook(HookCommand),
    Internal(InternalRootCommand),
    SampleEvent,
    Reset,
    DismissBlocker,
    Serve,
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
