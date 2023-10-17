use clap::Args;
use colorful::Colorful;
use miette::miette;

use ockam_node::Context;

use crate::util::node_rpc;
use crate::{docs, fmt_ok, CommandGlobalOpts};

const LONG_ABOUT: &str = include_str!("./static/default/long_about.txt");
const AFTER_LONG_HELP: &str = include_str!("./static/default/after_long_help.txt");

/// Change the default node
#[derive(Clone, Debug, Args)]
#[command(
long_about = docs::about(LONG_ABOUT),
after_long_help = docs::after_help(AFTER_LONG_HELP)
)]
pub struct DefaultCommand {
    /// Name of the node to set as default
    node_name: String,
}

impl DefaultCommand {
    pub fn run(self, opts: CommandGlobalOpts) {
        node_rpc(run_impl, (opts, self));
    }
}

async fn run_impl(
    _cxt: Context,
    (opts, cmd): (CommandGlobalOpts, DefaultCommand),
) -> miette::Result<()> {
    if opts.state.is_default_node(&cmd.node_name).await? {
        Err(miette!(
            "The node '{}' is already the default",
            cmd.node_name
        ))
    } else {
        opts.state.set_default_node(&cmd.node_name).await?;
        opts.terminal
            .stdout()
            .plain(fmt_ok!("The node '{}' is now the default", cmd.node_name))
            .machine(&cmd.node_name)
            .write_line()?;
        Ok(())
    }
}
