use crate::{
    credential::validate_encoded_cred, fmt_log, fmt_ok, terminal::OckamColor, util::node_rpc,
    CommandGlobalOpts,
};
use clap::Args;
use colorful::Colorful;
use miette::{miette, IntoDiagnostic};
use ockam::identity::{Identities, Identity};
use ockam::Context;
use ockam_api::cli_state::random_name;
use ockam_api::cli_state::{CredentialConfig, StateDirTrait};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::{sync::Mutex, try_join};

#[derive(Clone, Debug, Args)]
pub struct StoreCommand {
    #[arg(hide_default_value = true, default_value_t = random_name())]
    pub credential_name: String,

    #[arg(long = "issuer")]
    pub issuer: String,

    #[arg(group = "credential_value", value_name = "CREDENTIAL_STRING", long)]
    pub credential: Option<String>,

    #[arg(group = "credential_value", value_name = "CREDENTIAL_FILE", long)]
    pub credential_path: Option<PathBuf>,

    #[arg()]
    pub vault: Option<String>,
}

impl StoreCommand {
    pub fn run(self, opts: CommandGlobalOpts) {
        node_rpc(run_impl, (opts, self));
    }
}

async fn run_impl(
    _ctx: Context,
    (opts, cmd): (CommandGlobalOpts, StoreCommand),
) -> miette::Result<()> {
    opts.terminal.write_line(&fmt_log!(
        "Storing credential {}...\n",
        cmd.credential_name.clone()
    ))?;

    let is_finished: Mutex<bool> = Mutex::new(false);

    let send_req = async {
        let cred_as_str = match (&cmd.credential, &cmd.credential_path) {
            (_, Some(credential_path)) => tokio::fs::read_to_string(credential_path)
                .await?
                .trim()
                .to_string(),
            (Some(credential), _) => credential.to_string(),
            _ => {
                *is_finished.lock().await = true;
                return crate::Result::Err(
                    miette!("Credential or Credential Path argument must be provided").into(),
                );
            }
        };

        let identities = match opts
            .state
            .get_identities_with_optional_vault_ame(&cmd.vault)
            .await
        {
            Ok(i) => i,
            Err(_) => {
                *is_finished.lock().await = true;
                return Err(miette!("Invalid state").into());
            }
        };

        let issuer = match identity(&cmd.issuer, identities.clone()).await {
            Ok(i) => i,
            Err(_) => {
                *is_finished.lock().await = true;
                return Err(miette!("Issuer is invalid {}", &cmd.issuer).into());
            }
        };

        let cred = hex::decode(&cred_as_str)?;
        if let Err(e) = validate_encoded_cred(&cred, identities, issuer.identifier()).await {
            *is_finished.lock().await = true;
            return Err(miette!("Credential is invalid\n{}", e).into());
        }

        // store
        opts.state.credentials.create(
            &cmd.credential_name,
            CredentialConfig::new(issuer.identifier().clone(), issuer.export()?, cred)?,
        )?;

        *is_finished.lock().await = true;

        Ok(cred_as_str)
    };

    let output_messages = vec![format!("Storing credential...")];

    let progress_output = opts
        .terminal
        .progress_output(&output_messages, &is_finished);

    let (credential, _) = try_join!(send_req, progress_output)?;

    opts.terminal
        .stdout()
        .machine(credential.to_string())
        .json(serde_json::json!(
            {
                "name": cmd.credential_name,
                "issuer": cmd.issuer,
                "credential": credential
            }
        ))
        .plain(fmt_ok!(
            "Credential {} stored\n",
            cmd.credential_name
                .to_string()
                .color(OckamColor::PrimaryResource.color())
        ))
        .write_line()?;

    Ok(())
}

async fn identity(identity: &str, identities: Arc<Identities>) -> miette::Result<Identity> {
    let identity_as_bytes = hex::decode(identity).into_diagnostic()?;

    let identity = identities
        .identities_creation()
        .import(None, &identity_as_bytes)
        .await
        .into_diagnostic()?;

    Ok(identity)
}
