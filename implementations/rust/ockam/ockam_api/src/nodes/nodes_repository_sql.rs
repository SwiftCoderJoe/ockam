use std::str::FromStr;
use std::sync::Arc;

use sqlx::*;

use ockam::identity::Identifier;
use ockam::{FromSqlxError, SqlxDatabase, ToSqlxType, ToVoid};
use ockam_core::async_trait;
use ockam_core::errcode::{Kind, Origin};
use ockam_core::Result;
use ockam_multiaddr::MultiAddr;

use crate::config::lookup::InternetAddress;

#[async_trait]
pub trait NodesRepository: Send + Sync + 'static {
    async fn store_node(&self, node_info: &NodeInfo) -> Result<()>;
    async fn get_nodes(&self) -> Result<Vec<NodeInfo>>;
}

pub struct NodesSqlxDatabase {
    database: Arc<SqlxDatabase>,
}

impl NodesSqlxDatabase {
    pub fn new(database: Arc<SqlxDatabase>) -> Self {
        Self { database }
    }

    /// Create a new in-memory database
    pub fn create() -> Arc<Self> {
        Arc::new(Self::new(Arc::new(SqlxDatabase::in_memory())))
    }
}

#[async_trait]
impl NodesRepository for NodesSqlxDatabase {
    async fn store_node(&self, node_info: &NodeInfo) -> Result<()> {
        let query = query("INSERT OR REPLACE INTO node VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)")
            .bind(node_info.name.to_sql())
            .bind(node_info.identifier.to_sql())
            .bind(node_info.verbosity.to_sql())
            .bind(node_info.is_default.to_sql())
            .bind(node_info.is_authority.to_sql())
            .bind(
                node_info
                    .tcp_listener_address
                    .as_ref()
                    .map(|a| a.to_string().to_sql()),
            )
            .bind(node_info.pid.map(|p| p.to_sql()));
        Ok(query.execute(&self.database.pool).await.void()?)
    }

    async fn get_nodes(&self) -> Result<Vec<NodeInfo>> {
        let query = query_as("SELECT * FROM node");
        let rows: Vec<NodeRow> = query.fetch_all(&self.database.pool).await.into_core()?;
        rows.iter().map(|r| r.node_info()).collect()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct NodeInfo {
    name: String,
    identifier: Identifier,
    verbosity: u8,
    is_default: bool,
    is_authority: bool,
    tcp_listener_address: Option<InternetAddress>,
    pid: Option<u32>,
}

impl NodeInfo {
    pub fn new(
        name: String,
        identifier: Identifier,
        verbosity: u8,
        is_default: bool,
        is_authority: bool,
        tcp_listener_address: Option<InternetAddress>,
        pid: Option<u32>,
    ) -> Self {
        Self {
            name,
            identifier,
            verbosity,
            is_default,
            is_authority,
            tcp_listener_address,
            pid,
        }
    }
    pub fn name(&self) -> String {
        self.name.clone()
    }

    pub fn identifier(&self) -> Identifier {
        self.identifier.clone()
    }

    pub fn verbosity(&self) -> u8 {
        self.verbosity
    }

    pub fn is_default(&self) -> bool {
        self.is_default
    }

    pub fn is_authority_node(&self) -> bool {
        self.is_authority
    }

    pub fn tcp_listener_port(&self) -> Option<u16> {
        self.tcp_listener_address.as_ref().map(|t| t.port())
    }

    pub fn tcp_listener_address(&self) -> Option<InternetAddress> {
        self.tcp_listener_address.clone()
    }

    pub fn tcp_listener_multi_address(&self) -> Result<MultiAddr> {
        self.tcp_listener_address
            .as_ref()
            .ok_or(ockam::Error::new(
                Origin::Api,
                Kind::Internal,
                "no transport has been set on the node".to_string(),
            ))
            .and_then(|t| t.multi_addr())
    }

    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    pub fn is_running(&self) -> bool {
        self.pid.is_some()
    }
}

#[derive(FromRow)]
pub(crate) struct NodeRow {
    name: String,
    identifier: String,
    verbosity: u8,
    is_default: bool,
    is_authority: bool,
    tcp_listener_address: Option<String>,
    pid: Option<u32>,
}

impl NodeRow {
    pub(crate) fn node_info(&self) -> Result<NodeInfo> {
        Ok(NodeInfo::new(
            self.name.clone(),
            Identifier::from_str(self.identifier.as_str())?,
            self.verbosity,
            self.is_default,
            self.is_authority,
            self.tcp_listener_address
                .clone()
                .and_then(|a| InternetAddress::new(a.as_str())),
            self.pid,
        ))
    }
}

#[cfg(test)]
mod test {
    use std::path::Path;

    use tempfile::NamedTempFile;

    use super::*;

    #[tokio::test]
    async fn test_repository() -> Result<()> {
        let file = NamedTempFile::new().unwrap();
        let repository = create_repository(file.path()).await?;
        let identifier = Identifier::from_str("Ie92f183eb4c324804ef4d62962dea94cf095a265").unwrap();

        let node_info = NodeInfo::new(
            "node_name".to_string(),
            identifier,
            0,
            false,
            false,
            None,
            None,
        );

        repository.store_node(&node_info).await?;
        let result = repository.get_nodes().await?;
        assert_eq!(result, vec![node_info]);
        Ok(())
    }

    /// HELPERS
    async fn create_repository(path: &Path) -> Result<Arc<dyn NodesRepository>> {
        let db = SqlxDatabase::create(path).await?;
        Ok(Arc::new(NodesSqlxDatabase::new(Arc::new(db))))
    }
}
