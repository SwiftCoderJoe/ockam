use clap::Args;

use crate::node::NodeOpts;
use crate::util::extract_address_value;
use ockam::Context;
use ockam_api::nodes::models;
use ockam_core::api::Request;

use crate::util::{node_rpc, Rpc};
use crate::CommandGlobalOpts;

#[derive(Clone, Debug, Args)]
pub struct ShowCommand {
    #[command(flatten)]
    pub node_opts: NodeOpts,

    /// Tcp Listener ID
    pub id: String,
}

impl ShowCommand {
    pub fn run(self, options: CommandGlobalOpts) {
        node_rpc(run_impl, (options, self));
    }
}

async fn run_impl(
    ctx: Context,
    (opts, cmd): (CommandGlobalOpts, ShowCommand),
) -> crate::Result<()> {
    let node = extract_address_value(&cmd.node_opts.api_node)?;
    let mut rpc = Rpc::background(&ctx, &opts, &node)?;
    rpc.request(Request::get(format!("/node/tcp/listener/{}", &cmd.id)))
        .await?;
    let listener_info = rpc.parse_response::<models::transport::TransportStatus>()?;

    println!("TCP Listener:");
    println!("  ID: {}", listener_info.tid);
    println!("  Type: {}", listener_info.tt);
    println!("  Mode: {}", listener_info.tm);
    println!("  Socket address: {}", listener_info.socket_addr);
    println!("  Worker address: {}", listener_info.worker_addr);

    Ok(())
}
