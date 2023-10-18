use std::fmt::Write;

use clap::Args;
use miette::IntoDiagnostic;
use ockam_node::Context;

use crate::util::node_rpc;
use crate::{docs, CommandGlobalOpts};

const LONG_ABOUT: &str = include_str!("./static/show/long_about.txt");
const PREVIEW_TAG: &str = include_str!("../static/preview_tag.txt");
const AFTER_LONG_HELP: &str = include_str!("./static/show/after_long_help.txt");

/// Show the details of a vault
#[derive(Clone, Debug, Args)]
#[command(
    long_about = docs::about(LONG_ABOUT),
    before_help = docs::before_help(PREVIEW_TAG),
    after_long_help = docs::after_help(AFTER_LONG_HELP)
)]
pub struct ShowCommand {
    /// Name of the vault
    pub name: Option<String>,
}

impl ShowCommand {
    pub fn run(self, opts: CommandGlobalOpts) {
        node_rpc(run_impl, (opts, self));
    }
}

async fn run_impl(
    _ctx: Context,
    (opts, cmd): (CommandGlobalOpts, ShowCommand),
) -> miette::Result<()> {
    let named_vault = opts.state.get_named_vault_or_default(&cmd.name).await?;
    let json = serde_json::to_string_pretty(&named_vault).into_diagnostic()?;

    let plain = {
        let mut buf = String::new();

        writeln!(buf, "Vault:").into_diagnostic()?;
        for line in named_vault.to_string().lines() {
            writeln!(buf, "{:2}{}", "", line).into_diagnostic()?;
        }
        buf
    };

    opts.terminal
        .stdout()
        .json(json)
        .plain(plain)
        .write_line()?;

    Ok(())
}
