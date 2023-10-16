use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

use sqlx::sqlite::SqliteRow;
use sqlx::*;

use ockam::identity::Vault;
use ockam::{FromSqlxError, SqlxDatabase, ToSqlxType, ToVoid};
use ockam_core::async_trait;
use ockam_core::Result;

#[async_trait]
pub trait VaultsRepository: Send + Sync + 'static {
    async fn name_vault(&self, name: &str, path: PathBuf) -> Result<()>;
    async fn get_vault_by_name(&self, name: &str) -> Result<Option<NamedVault>>;
    async fn get_default_vault(&self) -> Result<Option<NamedVault>>;
    async fn get_default_vault_name(&self) -> Result<Option<String>>;
}

pub struct VaultsSqlxDatabase {
    database: Arc<SqlxDatabase>,
}

impl VaultsSqlxDatabase {
    pub fn new(database: Arc<SqlxDatabase>) -> Self {
        Self { database }
    }

    /// Create a new in-memory database
    pub fn create() -> Arc<Self> {
        Arc::new(Self::new(Arc::new(SqlxDatabase::in_memory())))
    }
}

#[async_trait]
impl VaultsRepository for VaultsSqlxDatabase {
    async fn name_vault(&self, name: &str, path: PathBuf) -> Result<()> {
        let query = query("INSERT OR REPLACE INTO vault VALUES (?1, ?2, ?3)")
            .bind(name.to_sql())
            .bind(path.to_sql())
            .bind(false.to_sql());
        Ok(query.execute(&self.database.pool).await.void()?)
    }

    async fn get_vault_by_name(&self, name: &str) -> Result<Option<NamedVault>> {
        let query = query_as("SELECT * FROM vault where name = $1").bind(name.to_sql());
        let row: Option<VaultRow> = query
            .fetch_optional(&self.database.pool)
            .await
            .into_core()?;
        row.map(|r| r.named_vault()).transpose()
    }

    async fn get_default_vault(&self) -> Result<Option<NamedVault>> {
        let query = query_as("SELECT * FROM vault where is_default = $1").bind(true.to_sql());
        let row: Option<VaultRow> = query
            .fetch_optional(&self.database.pool)
            .await
            .into_core()?;
        row.map(|r| r.named_vault()).transpose()
    }

    async fn get_default_vault_name(&self) -> Result<Option<String>> {
        let query = query("SELECT name FROM vault where is_default = $1").bind(true.to_sql());
        let row: Option<SqliteRow> = query
            .fetch_optional(&self.database.pool)
            .await
            .into_core()?;
        Ok(row.map(|r| r.get(0)))
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct NamedVault {
    name: String,
    path: PathBuf,
    is_default: bool,
}

impl NamedVault {
    pub fn new(name: String, path: PathBuf, is_default: bool) -> Self {
        Self {
            name,
            path,
            is_default,
        }
    }
    pub fn name(&self) -> String {
        self.name.clone()
    }

    pub fn path(&self) -> PathBuf {
        self.path.clone()
    }

    pub fn is_default(&self) -> bool {
        self.is_default
    }

    pub async fn vault(&self) -> Result<Vault> {
        Vault::create_with_persistent_storage_path(&self.path).await
    }
}

#[derive(FromRow)]
pub(crate) struct VaultRow {
    name: String,
    path: String,
    is_default: bool,
}

impl VaultRow {
    pub(crate) fn named_vault(&self) -> Result<NamedVault> {
        Ok(NamedVault::new(
            self.name.clone(),
            PathBuf::from_str(self.path.as_str()).unwrap(),
            self.is_default,
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

        repository.name_vault("vault_name", "path".into()).await?;
        let result = repository.get_vault_by_name("vault_name").await?;

        let expected = NamedVault::new("vault_name".to_string(), "path".into(), false);

        assert_eq!(result, Some(expected));
        Ok(())
    }

    /// HELPERS
    async fn create_repository(path: &Path) -> Result<Arc<dyn VaultsRepository>> {
        let db = SqlxDatabase::create(path).await?;
        Ok(Arc::new(VaultsSqlxDatabase::new(Arc::new(db))))
    }
}
