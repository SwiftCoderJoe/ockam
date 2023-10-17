use std::net::{SocketAddr, TcpListener};

use ockam_core::Result;

use crate::error::{ApiError, ParseError};

pub fn get_free_address() -> Result<SocketAddr, ApiError> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    let res = format!("127.0.0.1:{port}")
        .parse()
        .map_err(ParseError::from)?;
    Ok(res)
}
