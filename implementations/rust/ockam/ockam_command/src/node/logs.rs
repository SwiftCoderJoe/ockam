use clap::Args;

use ockam_node::Context;

use crate::node::get_node_name;
use crate::util::node_rpc;
use crate::{docs, CommandGlobalOpts};

const LONG_ABOUT: &str = include_str!("./static/logs/long_about.txt");
const PREVIEW_TAG: &str = include_str!("../static/preview_tag.txt");
const AFTER_LONG_HELP: &str = include_str!("./static/logs/after_long_help.txt");

/// Get the stdout/stderr log file of a node
#[derive(Clone, Debug, Args)]
#[command(
long_about = docs::about(LONG_ABOUT),
before_help = docs::before_help(PREVIEW_TAG),
after_long_help = docs::after_help(AFTER_LONG_HELP)
)]
pub struct LogCommand {
    /// Name of the node to retrieve the logs from.
    node_name: Option<String>,

    /// Show the standard error log file.
    #[arg(long = "err")]
    show_err: bool,
}

impl LogCommand {
    pub fn run(self, opts: CommandGlobalOpts) {
        node_rpc(run_impl, (opts, self));
    }
}

async fn run_impl(
    _ctx: Context,
    (opts, cmd): (CommandGlobalOpts, LogCommand),
) -> miette::Result<()> {
    let node_name = get_node_name(&opts.state, &cmd.node_name).await;
    let log_file_path = if cmd.show_err {
        opts.state.node_stderr_log(&node_name)?
    } else {
        opts.state.node_stdout_log(&node_name)?
    };
    opts.terminal
        .stdout()
        .machine(log_file_path.display().to_string())
        .write_line()?;
    Ok(())
}
