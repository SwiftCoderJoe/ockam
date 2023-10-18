use clap::{arg, Args};
use colorful::Colorful;
use miette::IntoDiagnostic;
use ockam::Context;
use ockam_api::cli_state::{StateDirTrait, StateItemTrait};

use crate::output::CredentialAndPurposeKeyDisplay;
use crate::{credential::validate_encoded_cred, util::node_rpc, CommandGlobalOpts};

#[derive(Clone, Debug, Args)]
pub struct ShowCommand {
    #[arg()]
    pub credential_name: String,

    #[arg()]
    pub vault: Option<String>,
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
    let cred = opts.state.credentials.get(&cmd.credential_name)?;
    let cred_config = cred.config();

    let identities = opts
        .state
        .get_identities_with_optional_vault_ame(&cmd.vault)
        .await?;
    identities
        .identities_creation()
        .import(
            Some(&cred_config.issuer_identifier),
            &cred_config.encoded_issuer_change_history,
        )
        .await
        .into_diagnostic()?;

    let is_verified = match validate_encoded_cred(
        &cred_config.encoded_credential,
        identities,
        &cred_config.issuer_identifier,
    )
    .await
    {
        Ok(_) => "✔︎".light_green(),
        Err(_) => "✕".light_red(),
    };

    let cred = cred_config.credential()?;
    println!("Credential: {} {is_verified}", cmd.credential_name);
    println!("{}", CredentialAndPurposeKeyDisplay(cred));

    Ok(())
}
