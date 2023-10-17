use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use miette::{miette, IntoDiagnostic, WrapErr};
use minicbor::{Decode, Encode};

use ockam_core::api::{Reply, Request};
use ockam_core::{AsyncTryClone, Route};
use ockam_multiaddr::proto::{Node, Project, Service};
use ockam_multiaddr::{proto, MultiAddr, Protocol};
use ockam_node::api::Client;
use ockam_node::Context;
use ockam_transport_tcp::{TcpConnectionOptions, TcpTransport};

use crate::cli_state::CliState;
use crate::error::ApiError;
use crate::nodes::NODEMANAGER_ADDR;

/// This struct represents a node that has been started
/// on the same machine with a given node name
///
/// The methods on this struct allow a user to send requests containing a value of type `T`
/// and expect responses with a value of type `R`
#[derive(Clone)]
pub struct BackgroundNode {
    cli_state: CliState,
    node_name: String,
    to: Route,
    timeout: Option<Duration>,
    tcp_transport: Arc<TcpTransport>,
}

impl BackgroundNode {
    /// Create a new client to send requests to a running background node
    /// This function instantiates a TcpTransport. Since a TcpTransport can only be created once
    /// this function must only be called once
    ///
    /// The optional node name is used to locate the node. It is either
    /// a node specified by the user or the default node if no node name is given.
    pub async fn create(
        ctx: &Context,
        cli_state: &CliState,
        node_name: &Option<String>,
    ) -> miette::Result<BackgroundNode> {
        let tcp_transport = TcpTransport::create(ctx).await.into_diagnostic()?;
        let node_name = cli_state.get_node_name_or_default(node_name).await?;
        if !cli_state.is_node_running(&node_name).await? {
            return Err(miette!("The node '{}' is not running", node_name));
        }

        BackgroundNode::new(&tcp_transport, cli_state, node_name.as_str()).await
    }

    /// Create a new client to send requests to a running background node
    pub async fn new(
        tcp_transport: &TcpTransport,
        cli_state: &CliState,
        node_name: &str,
    ) -> miette::Result<BackgroundNode> {
        Ok(BackgroundNode {
            cli_state: cli_state.clone(),
            node_name: node_name.to_string(),
            to: NODEMANAGER_ADDR.into(),
            timeout: None,
            tcp_transport: Arc::new(tcp_transport.async_try_clone().await.into_diagnostic()?),
        })
    }

    // Set a different node name
    pub fn set_node_name(&mut self, node_name: &str) -> &Self {
        self.node_name = node_name.to_string();
        self
    }

    pub fn node_name(&self) -> String {
        self.node_name.clone()
    }

    /// Use a default timeout for making requests
    pub fn set_timeout(&mut self, timeout: Duration) -> &Self {
        self.timeout = Some(timeout);
        self
    }

    /// Send a request and expect a decodable response
    pub async fn ask<T, R>(&self, ctx: &Context, req: Request<T>) -> miette::Result<R>
    where
        T: Encode<()>,
        R: for<'b> Decode<'b, ()>,
    {
        self.ask_and_get_reply(ctx, req)
            .await?
            .success()
            .into_diagnostic()
    }

    /// Send a request and expect a decodable response and use a specific timeout
    pub async fn ask_with_timeout<T, R>(
        &self,
        ctx: &Context,
        req: Request<T>,
        timeout: Duration,
    ) -> miette::Result<R>
    where
        T: Encode<()>,
        R: for<'b> Decode<'b, ()>,
    {
        let client = self.make_client_with_timeout(Some(timeout)).await?;
        client
            .ask(ctx, req)
            .await
            .into_diagnostic()?
            .success()
            .into_diagnostic()
    }

    /// Send a request but don't decode the response
    pub async fn tell<T>(&self, ctx: &Context, req: Request<T>) -> miette::Result<()>
    where
        T: Encode<()>,
    {
        let client = self.make_client().await?;
        client
            .tell(ctx, req)
            .await
            .into_diagnostic()?
            .success()
            .into_diagnostic()
    }

    /// Send a request and expect either a decodable response or an API error.
    /// This method returns an error if the request cannot be sent of if there is any decoding error
    pub async fn ask_and_get_reply<T, R>(
        &self,
        ctx: &Context,
        req: Request<T>,
    ) -> miette::Result<Reply<R>>
    where
        T: Encode<()>,
        R: for<'b> Decode<'b, ()>,
    {
        let client = self.make_client().await?;
        client.ask(ctx, req).await.into_diagnostic()
    }

    /// Make a route to the node and connect using TCP
    async fn create_route(&self) -> miette::Result<Route> {
        let mut route = self.to.clone();
        let node_info = self.cli_state.get_node(&self.node_name).await?;
        let port = node_info.tcp_listener_port().expect(
            format!(
                "an api transport should have been started for node {}",
                &self.node_name
            )
            .as_str(),
        );
        let addr_str = format!("localhost:{port}");
        let addr = self
            .tcp_transport
            .connect(addr_str, TcpConnectionOptions::new())
            .await
            .into_diagnostic()?
            .sender_address()
            .clone();
        route.modify().prepend(addr);
        debug!("Sending requests to {route}");
        Ok(route)
    }

    /// Make a response / request client connected to the node
    pub async fn make_client(&self) -> miette::Result<Client> {
        self.make_client_with_timeout(self.timeout).await
    }

    /// Make a response / request client connected to the node
    /// and specify a timeout for receiving responses
    pub async fn make_client_with_timeout(
        &self,
        timeout: Option<Duration>,
    ) -> miette::Result<Client> {
        let route = self.create_route().await?;
        Ok(Client::new(&route, timeout))
    }
}

/// Parses a node's input string for its name in case it's a `MultiAddr` string.
///
/// Ensures that the node's name will be returned if the input string is a `MultiAddr` of the `node` type
/// Examples: `n1` or `/node/n1` returns `n1`; `/project/p1` or `/tcp/n2` returns an error message.
pub fn parse_node_name(input: &str) -> miette::Result<String> {
    if input.is_empty() {
        return Err(miette!("Empty address in node name argument").into());
    }
    // Node name was passed as "n1", for example
    if !input.contains('/') {
        return Ok(input.to_string());
    }
    // Input has "/", so we process it as a MultiAddr
    let maddr = MultiAddr::from_str(input)
        .into_diagnostic()
        .wrap_err("Invalid format for node name argument")?;
    let err_message = String::from("A node MultiAddr must follow the format /node/<name>");
    if let Some(p) = maddr.iter().next() {
        if p.code() == proto::Node::CODE {
            let node_name = p
                .cast::<proto::Node>()
                .ok_or(miette!("Failed to parse the 'node' protocol"))?
                .to_string();
            if !node_name.is_empty() {
                return Ok(node_name);
            }
        }
    }
    Err(miette!(err_message).into())
}

/// Get address value from a string.
///
/// The input string can be either a plain address of a MultiAddr formatted string.
/// Examples: `/node/<name>`, `<name>`
pub fn extract_address_value(input: &str) -> Result<String, ApiError> {
    // we default to the `input` value
    let mut addr = input.to_string();
    // if input has "/", we process it as a MultiAddr
    if input.contains('/') {
        let maddr = MultiAddr::from_str(input)?;
        if let Some(p) = maddr.iter().next() {
            match p.code() {
                Node::CODE => {
                    addr = p
                        .cast::<Node>()
                        .ok_or(ApiError::message("Failed to parse `node` protocol"))?
                        .to_string();
                }
                Service::CODE => {
                    addr = p
                        .cast::<Service>()
                        .ok_or(ApiError::message("Failed to parse `service` protocol"))?
                        .to_string();
                }
                Project::CODE => {
                    addr = p
                        .cast::<Project>()
                        .ok_or(ApiError::message("Failed to parse `project` protocol"))?
                        .to_string();
                }
                code => return Err(ApiError::message(format!("Protocol {code} not supported"))),
            }
        } else {
            return Err(ApiError::message("invalid address protocol"));
        }
    }
    if addr.is_empty() {
        return Err(ApiError::message(format!(
            "Empty address in input: {input}"
        )));
    }
    Ok(addr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_node_name() {
        let test_cases = vec![
            ("", Err(())),
            ("test", Ok("test")),
            ("/test", Err(())),
            ("test/", Err(())),
            ("/node", Err(())),
            ("/node/", Err(())),
            ("/node/n1", Ok("n1")),
            ("/service/s1", Err(())),
            ("/project/p1", Err(())),
            ("/randomprotocol/rp1", Err(())),
            ("/node/n1/tcp", Err(())),
            ("/node/n1/test", Err(())),
            ("/node/n1/tcp/22", Ok("n1")),
        ];
        for (input, expected) in test_cases {
            if let Ok(addr) = expected {
                assert_eq!(parse_node_name(input).unwrap(), addr);
            } else {
                assert!(parse_node_name(input).is_err());
            }
        }
    }

    #[test]
    fn test_extract_address_value() {
        let test_cases = vec![
            ("", Err(())),
            ("test", Ok("test")),
            ("/test", Err(())),
            ("test/", Err(())),
            ("/node", Err(())),
            ("/node/", Err(())),
            ("/node/n1", Ok("n1")),
            ("/service/s1", Ok("s1")),
            ("/project/p1", Ok("p1")),
            ("/randomprotocol/rp1", Err(())),
            ("/node/n1/tcp", Err(())),
            ("/node/n1/test", Err(())),
            ("/node/n1/tcp/22", Ok("n1")),
        ];
        for (input, expected) in test_cases {
            if let Ok(addr) = expected {
                assert_eq!(extract_address_value(input).unwrap(), addr);
            } else {
                assert!(extract_address_value(input).is_err());
            }
        }
    }
}
