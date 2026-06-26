mod cli;
mod hook_runtime;
mod internal;
mod local_commands;
mod output;
mod system_commands;
mod tools;

use clap::{CommandFactory, Parser};
use cli::{Cli, Command, ToolArg};

fn main() {
    let cli = Cli::parse();
    let Some(command) = cli.command else {
        // 裸 niuma 只展示命令入口，避免误把当前 Local API 主状态当成命令执行结果。
        Cli::command().print_help().expect("CLI help 必须可打印");
        println!();
        return;
    };

    let output = match command {
        Command::Doctor => system_commands::doctor(),
        Command::Status { tool: None } => system_commands::status(),
        Command::Status {
            tool: Some(ToolArg::Codex),
        } => tools::codex::hook_commands::codex_hook_status(),
        Command::Codex(command) => return tools::codex::managed::run_codex_command(command.args),
        Command::CodexSessions => tools::codex::sessions::codex_sessions(),
        Command::CodexSend(command) => {
            tools::codex::send::codex_send(command.wrapper_session_id, command.message)
        }
        Command::CodexInterrupt(command) => {
            tools::codex::interrupt::codex_interrupt(command.wrapper_session_id)
        }
        Command::Hook(command) => tools::codex::hook_commands::run_hook_command(command),
        Command::Internal(command) => return internal::run_internal_command(command),
        Command::SampleEvent => system_commands::sample_event(),
        Command::Reset => system_commands::reset(),
        Command::DismissBlocker => system_commands::dismiss_blocker(),
        Command::Serve => return local_commands::serve(),
    };
    output::print_response(&output);
}
