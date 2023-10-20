use clap::Args;
use itertools::Itertools;
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
        Some(it) => Vec::from([it.to_owned()]),
        None => {
            let all_spaces = opts.state.spaces.list()?.iter().map(|item| item.config().name.to_owned()).collect_vec();
            let selected_spaces = opts.terminal.select_multiple(String::from("Select one or more spaces that you would like to show info for"), all_spaces);

            let mut confirm_message = String::from("Show info for these spaces: ");
            confirm_message.push_str(&selected_spaces.join(","));

            // If the user didn't select anything or declines, we can return an empty vec which will cause build_list to show a message
            if !selected_spaces.is_empty() && opts.terminal.confirm_interactively(confirm_message) {
                selected_spaces
            } else {
                Vec::new()
            }
        }
    };

    // Create controller
    let node = InMemoryNode::start(ctx, &opts.state).await?;
    let controller = node.create_controller().await?;

    let mut all_spaces_info = Vec::new();

    for space_name in &space_names {
        let id = opts.state.spaces.get(space_name)?.config().id.clone();

        let space: Space = controller.get_space(ctx, id).await?;

        all_spaces_info.push(space.output()?);

        // What to do with `serde_json::to_string_pretty(&space).intoDiagnostic()?` ?

        opts.state
            .spaces
            .overwrite(&space_name, SpaceConfig::from(&space))?;
    }

    let output = opts.terminal.build_list(&all_spaces_info, "Space Info", "No spaces were asked for")?;
    opts.terminal
        .stdout()
        .plain(output)
        .write_line()?;

    Ok(())
}
