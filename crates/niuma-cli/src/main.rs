mod cli;
mod hook_runtime;
mod internal;
mod local_commands;
mod output;
mod system_commands;
mod tools;

use clap::Parser;
use cli::{Cli, Command, ToolArg};

fn main() {
    let cli = Cli::parse();
    let output = match cli.command.unwrap_or(Command::Status { tool: None }) {
        Command::Doctor => system_commands::doctor(),
        Command::Status { tool: None } => system_commands::status(),
        Command::Status {
            tool: Some(ToolArg::Codex),
        } => tools::codex::hook_commands::codex_hook_status(),
        Command::Codex(command) => return tools::codex::managed::run_codex_command(command.args),
        Command::Hook(command) => tools::codex::hook_commands::run_hook_command(command),
        Command::Internal(command) => return internal::run_internal_command(command),
        Command::SampleEvent => system_commands::sample_event(),
        Command::Reset => system_commands::reset(),
        Command::DismissBlocker => system_commands::dismiss_blocker(),
        Command::Serve => return local_commands::serve(),
    };
    output::print_response(&output);
}
