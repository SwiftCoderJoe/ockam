use clap::{arg, Args};

use colorful::Colorful;
use ockam::Context;
use ockam_api::cli_state::StateDirTrait;

use crate::{fmt_log, terminal::OckamColor, util::node_rpc, CommandGlobalOpts};

use super::CredentialOutput;

#[derive(Clone, Debug, Args)]
pub struct ListCommand {
    #[arg()]
    pub vault: Option<String>,
}

impl ListCommand {
    pub fn run(self, opts: CommandGlobalOpts) {
        node_rpc(run_impl, (opts, self));
    }
}

async fn run_impl(
    _ctx: Context,
    (opts, cmd): (CommandGlobalOpts, ListCommand),
) -> miette::Result<()> {
    opts.terminal
        .write_line(&fmt_log!("Listing Credentials...\n"))?;

    let vault_name = opts.state.get_vault_name_or_default(&cmd.vault).await?;
    let mut credentials: Vec<CredentialOutput> = Vec::new();

    for cred_state in opts.state.credentials.list()? {
        let cred = CredentialOutput::try_from_state(&opts, &cred_state, &vault_name).await?;
        credentials.push(cred);
    }

    let list = opts.terminal.build_list(
        &credentials,
        "Credentials",
        &format!(
            "No Credentials found for vault: {}",
            vault_name.color(OckamColor::PrimaryResource.color())
        ),
    )?;

    opts.terminal.stdout().plain(list).write_line()?;

    Ok(())
}
