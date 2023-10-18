use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

use sqlx::sqlite::SqliteRow;
use sqlx::*;

use ockam::{FromSqlxError, SqlxDatabase, ToSqlxType, ToVoid};
use ockam_core::async_trait;
use ockam_core::Result;

use crate::identity::{NamedVault, VaultsRepository};

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
    async fn store_vault(&self, name: &str, path: PathBuf, is_aws_kms: bool) -> Result<()> {
        let query = query("INSERT OR REPLACE INTO vault VALUES (?1, ?2, ?3, ?4)")
            .bind(name.to_sql())
            .bind(path.to_sql())
            .bind(is_aws_kms.to_sql())
            .bind(false.to_sql());
        Ok(query.execute(&self.database.pool).await.void()?)
    }

    /// Delete a vault by name
    async fn delete_vault(&self, name: &str) -> Result<()> {
        let is_default = self.is_default(name).await?;
        let query = query("DELETE FROM vault WHERE name = $1").bind(name.to_sql());
        query.execute(&self.database.pool).await.void()?;

        // if the deleted vault was the default one, select another vault to be the default one
        if is_default {
            let vaults = self.get_named_vaults().await?;
            if let Some(vault) = vaults.first() {
                self.set_as_default(&vault.name()).await?;
            };
        }
        Ok(())
    }

    async fn set_as_default(&self, name: &str) -> Result<()> {
        let transaction = self.database.pool.acquire().await.into_core()?;
        // set the identifier as the default one
        let query1 = query("UPDATE vault SET is_default = ? WHERE name = ?")
            .bind(true.to_sql())
            .bind(name.to_sql());
        query1.execute(&self.database.pool).await.void()?;

        // set all the others as non-default
        let query2 = query("UPDATE vault SET is_default = ? WHERE name <> ?")
            .bind(false.to_sql())
            .bind(name.to_sql());
        query2.execute(&self.database.pool).await.void()?;
        transaction.close().await.into_core()
    }

    async fn is_default(&self, name: &str) -> Result<bool> {
        let query = query_as("SELECT * FROM vault WHERE name = $1").bind(name.to_sql());
        let row: Option<VaultRow> = query
            .fetch_optional(&self.database.pool)
            .await
            .into_core()?;
        Ok(row.map(|r| r.is_default()).unwrap_or(false))
    }

    async fn get_named_vaults(&self) -> Result<Vec<NamedVault>> {
        let query = query_as("SELECT * FROM vault");
        let rows: Vec<VaultRow> = query.fetch_all(&self.database.pool).await.into_core()?;
        rows.iter().map(|r| r.named_vault()).collect()
    }

    async fn get_vault_by_name(&self, name: &str) -> Result<Option<NamedVault>> {
        let query = query_as("SELECT * FROM vault WHERE name = $1").bind(name.to_sql());
        let row: Option<VaultRow> = query
            .fetch_optional(&self.database.pool)
            .await
            .into_core()?;
        row.map(|r| r.named_vault()).transpose()
    }

    async fn get_default_vault(&self) -> Result<Option<NamedVault>> {
        let query = query_as("SELECT * FROM vault WHERE is_default = $1").bind(true.to_sql());
        let row: Option<VaultRow> = query
            .fetch_optional(&self.database.pool)
            .await
            .into_core()?;
        row.map(|r| r.named_vault()).transpose()
    }

    async fn get_default_vault_name(&self) -> Result<Option<String>> {
        let query = query("SELECT name FROM vault WHERE is_default = $1").bind(true.to_sql());
        let row: Option<SqliteRow> = query
            .fetch_optional(&self.database.pool)
            .await
            .into_core()?;
        Ok(row.map(|r| r.get(0)))
    }
}

#[derive(FromRow)]
pub(crate) struct VaultRow {
    name: String,
    path: String,
    is_aws_kms: bool,
    is_default: bool,
}

impl VaultRow {
    pub(crate) fn named_vault(&self) -> Result<NamedVault> {
        Ok(NamedVault::new(
            self.name.clone(),
            PathBuf::from_str(self.path.as_str()).unwrap(),
            self.is_aws_kms,
            self.is_default,
        ))
    }

    pub(crate) fn is_default(&self) -> bool {
        self.is_default
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

        repository
            .store_vault("vault_name", "path".into(), false)
            .await?;
        let result = repository.get_vault_by_name("vault_name").await?;

        let expected = NamedVault::new("vault_name".to_string(), "path".into(), false, false);

        assert_eq!(result, Some(expected));
        Ok(())
    }

    /// HELPERS
    async fn create_repository(path: &Path) -> Result<Arc<dyn VaultsRepository>> {
        let db = SqlxDatabase::create(path).await?;
        Ok(Arc::new(VaultsSqlxDatabase::new(Arc::new(db))))
    }
}
