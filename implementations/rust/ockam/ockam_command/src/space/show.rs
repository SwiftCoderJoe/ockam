use clap::Args;
use miette::IntoDiagnostic;

use ockam::Context;
use ockam_api::cli_state::{SpaceConfig, StateDirTrait, StateItemTrait};
use ockam_api::cloud::space::{Space, Spaces, self};
use ockam_api::nodes::InMemoryNode;

use crate::output::Output;
use crate::util::api::CloudOpts;
use crate::util::node_rpc;
use crate::{docs, CommandGlobalOpts};

const LONG_ABOUT: &str = include_str!("./static/show/long_about.txt");
const PREVIEW_TAG: &str = include_str!("../static/preview_tag.txt");
const AFTER_LONG_HELP: &str = include_str!("./static/show/after_long_help.txt");

/// Show the details of a space
#[derive(Clone, Debug, Args)]
#[command(
    arg_required_else_help = false,
    long_about = docs::about(LONG_ABOUT),
    before_help = docs::before_help(PREVIEW_TAG),
    after_long_help = docs::after_help(AFTER_LONG_HELP)
)]
pub struct ShowCommand {
    /// Name of the space.
    #[arg(display_order = 1001)]
    pub name: Option<String>,

    #[command(flatten)]
    pub cloud_opts: CloudOpts,
}

impl ShowCommand {
    pub fn run(self, options: CommandGlobalOpts) {
        node_rpc(rpc, (options, self));
    }
}

async fn rpc(ctx: Context, (opts, cmd): (CommandGlobalOpts, ShowCommand)) -> miette::Result<()> {
    run_impl(&ctx, opts, cmd).await
}

async fn run_impl(ctx: &Context, opts: CommandGlobalOpts, cmd: ShowCommand) -> miette::Result<()> {
    let space_names = match &cmd.name {
        Some(it) => Vec::from([it]),
        None => Vec::new()
    };

    let mut concatenated_string = String::new();

    for space_name in &space_names {
        let id = opts.state.spaces.get(space_name)?.config().id.clone();

        // Send request
        let node = InMemoryNode::start(ctx, &opts.state).await?;
        let controller = node.create_controller().await?;
        let space: Space = controller.get_space(ctx, id).await?;

        concatenated_string.push_str("Space output for space ");
        concatenated_string.push_str(&space_name);
        concatenated_string.push_str(&space.output()?);
        concatenated_string.push_str("Space json for space ");
        concatenated_string.push_str(&space_name);
        concatenated_string.push_str(&serde_json::to_string_pretty(&space).into_diagnostic()?);

        opts.state
            .spaces
            .overwrite(&space_name, SpaceConfig::from(&space))?;
    }

    opts.terminal
        .stdout()
        .plain(concatenated_string)
        .write_line()?;
    
    Ok(())
}
