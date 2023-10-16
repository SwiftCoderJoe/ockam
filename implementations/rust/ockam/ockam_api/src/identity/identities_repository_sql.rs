use core::str::FromStr;

use sqlx::*;

use ockam::identity::Identifier;
use ockam_core::async_trait;
use ockam_core::compat::sync::Arc;
use ockam_core::Result;
use ockam_node::database::{FromSqlxError, SqlxDatabase, ToSqlxType, ToVoid};

use crate::identity::identities_repository::{IdentitiesRepository, NamedIdentity};

/// Implementation of `IdentitiesRepository` trait based on an underlying database
/// using sqlx as its API, and Sqlite as its driver
#[derive(Clone)]
pub struct IdentitiesSqlxDatabase {
    database: Arc<SqlxDatabase>,
}

impl IdentitiesSqlxDatabase {
    /// Create a new database
    pub fn new(database: Arc<SqlxDatabase>) -> Self {
        Self { database }
    }

    /// Create a new in-memory database
    pub fn create() -> Arc<Self> {
        Arc::new(Self::new(Arc::new(SqlxDatabase::in_memory())))
    }
}

#[async_trait]
impl IdentitiesRepository for IdentitiesSqlxDatabase {
    async fn name_identity(&self, identifier: &Identifier, name: &str) -> Result<()> {
        let query = query("INSERT OR REPLACE INTO named_identity values (?, ?, ?)")
            .bind(identifier.to_sql())
            .bind(name.to_sql())
            .bind(false.to_sql());
        query.execute(&self.database.pool).await.void()
    }

    async fn delete_identity_by_name(&self, name: &str) -> Result<Option<Identifier>> {
        let identifier = self.get_identifier_by_name(name).await?;
        let query = query("DELETE FROM named_identity where name=?").bind(name.to_sql());
        query.execute(&self.database.pool).await.void()?;
        Ok(identifier)
    }

    async fn get_identifier_by_name(&self, name: &str) -> Result<Option<Identifier>> {
        let query = query_as("SELECT * FROM named_identity WHERE name=$1").bind(name.to_sql());
        let row: Option<NamedIdentityRow> = query
            .fetch_optional(&self.database.pool)
            .await
            .into_core()?;
        row.map(|r| r.identifier()).transpose()
    }

    async fn get_identity_name_by_identifier(
        &self,
        identifier: &Identifier,
    ) -> Result<Option<String>> {
        let query =
            query_as("SELECT * FROM named_identity WHERE identifier=$1").bind(identifier.to_sql());
        let row: Option<NamedIdentityRow> = query
            .fetch_optional(&self.database.pool)
            .await
            .into_core()?;
        Ok(row.map(|r| r.name()))
    }

    async fn get_named_identities(&self) -> Result<Vec<NamedIdentity>> {
        let query = query_as("SELECT * FROM named_identity");
        let row: Vec<NamedIdentityRow> = query.fetch_all(&self.database.pool).await.into_core()?;
        row.iter().map(|r| r.named_identity()).collect()
    }

    async fn get_named_identity(&self, name: &str) -> Result<Option<NamedIdentity>> {
        let query = query_as("SELECT * FROM named_identity WHERE name=$1").bind(name.to_sql());
        let row: Option<NamedIdentityRow> = query
            .fetch_optional(&self.database.pool)
            .await
            .into_core()?;
        row.map(|r| r.named_identity()).transpose()
    }

    async fn set_as_default(&self, identifier: &Identifier) -> Result<()> {
        let transaction = self.database.pool.acquire().await.into_core()?;
        // set the identifier as the default one
        let query1 = query("UPDATE named_identity SET is_default = ? WHERE identifier = ?")
            .bind(true.to_sql())
            .bind(identifier.to_sql());
        query1.execute(&self.database.pool).await.void()?;

        // set all the others as non-default
        let query2 = query("UPDATE named_identity SET is_default = ? WHERE identifier <> ?")
            .bind(false.to_sql())
            .bind(identifier.to_sql());
        query2.execute(&self.database.pool).await.void()?;
        transaction.close().await.into_core()
    }

    async fn set_as_default_by_name(&self, name: &str) -> Result<()> {
        let query = query("UPDATE named_identity SET is_default = ? WHERE name = ?")
            .bind(true.to_sql())
            .bind(name.to_sql());
        query.execute(&self.database.pool).await.void()
    }

    async fn get_default_identifier(&self) -> Result<Option<Identifier>> {
        let query = query_as("SELECT * FROM named_identity WHERE is_default=?").bind(true.to_sql());
        let row: Option<NamedIdentityRow> = query
            .fetch_optional(&self.database.pool)
            .await
            .into_core()?;
        row.map(|r| r.identifier()).transpose()
    }

    async fn get_default_named_identity(&self) -> Result<Option<NamedIdentity>> {
        let query =
            query_as("SELECT * FROM named_identity WHERE is_default=$1").bind(true.to_sql());
        let row: Option<NamedIdentityRow> = query
            .fetch_optional(&self.database.pool)
            .await
            .into_core()?;
        row.map(|r| r.named_identity()).transpose()
    }

    async fn get_default_identity_name(&self) -> Result<Option<String>> {
        let query =
            query_as("SELECT * FROM named_identity WHERE is_default=$1").bind(true.to_sql());
        let row: Option<NamedIdentityRow> = query
            .fetch_optional(&self.database.pool)
            .await
            .into_core()?;
        Ok(row.map(|r| r.name))
    }

    async fn is_default_identity_by_name(&self, name: &str) -> Result<bool> {
        let query =
            query_as("SELECT is_default FROM named_identity WHERE name=$1").bind(name.to_sql());
        let row: Option<NamedIdentityRow> = query
            .fetch_optional(&self.database.pool)
            .await
            .into_core()?;
        Ok(row.map(|r| r.is_default).unwrap_or(false))
    }
}

#[derive(sqlx::FromRow)]
pub(crate) struct NamedIdentityRow {
    identifier: String,
    name: String,
    is_default: bool,
}

impl NamedIdentityRow {
    pub(crate) fn identifier(&self) -> Result<Identifier> {
        Identifier::from_str(&self.identifier)
    }

    pub(crate) fn name(&self) -> String {
        self.name.clone()
    }

    pub(crate) fn named_identity(&self) -> Result<NamedIdentity> {
        Ok(NamedIdentity::new(
            self.identifier()?,
            self.name.clone(),
            self.is_default,
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use tempfile::NamedTempFile;

    use super::*;

    #[tokio::test]
    async fn test_identities_repository_named_identities() -> Result<()> {
        let identifier1 =
            Identifier::from_str("Ie92f183eb4c324804ef4d62962dea94cf095a265").unwrap();
        let identifier2 =
            Identifier::from_str("I124ed0b2e5a2be82e267ead6b3279f683616b66d").unwrap();
        let db_file = NamedTempFile::new().unwrap();
        let repository = create_repository(db_file.path()).await?;

        // A name can be associated to an identity
        repository.name_identity(&identifier1, "name1").await?;
        repository.name_identity(&identifier2, "name2").await?;

        let result = repository.get_identifier_by_name("name1").await?;
        assert_eq!(result, Some(identifier1.clone()));

        let result = repository
            .get_identity_name_by_identifier(&identifier1)
            .await?;
        assert_eq!(result, Some("name1".into()));

        let result = repository.get_named_identity("name2").await?;
        assert_eq!(result.map(|n| n.identifier()), Some(identifier2.clone()));

        let result = repository.get_named_identities().await?;
        assert_eq!(
            result.iter().map(|n| n.identifier()).collect::<Vec<_>>(),
            vec![identifier1.clone(), identifier2.clone()]
        );

        repository.delete_identity_by_name("name1").await?;
        let result = repository.get_named_identities().await?;
        assert_eq!(
            result.iter().map(|n| n.identifier()).collect::<Vec<_>>(),
            vec![identifier2.clone()]
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_identities_repository_default_identities() -> Result<()> {
        let identifier1 =
            Identifier::from_str("Ie92f183eb4c324804ef4d62962dea94cf095a265").unwrap();
        let identifier2 =
            Identifier::from_str("I124ed0b2e5a2be82e267ead6b3279f683616b66d").unwrap();
        let db_file = NamedTempFile::new().unwrap();
        let repository = create_repository(db_file.path()).await?;

        // A name can be associated to an identity
        repository.name_identity(&identifier1, "name1").await?;
        repository.name_identity(&identifier2, "name2").await?;

        // An identity can be marked as being the default one
        repository.set_as_default(&identifier1).await?;
        let result = repository.get_default_identifier().await?;
        assert_eq!(result, Some(identifier1.clone()));

        // An identity can be marked as being the default one by passing its name
        repository.set_as_default_by_name("name2").await?;
        let result = repository.get_default_identifier().await?;
        assert_eq!(result, Some(identifier2.clone()));

        let result = repository.get_default_named_identity().await?;
        assert_eq!(result.map(|n| n.identifier()), Some(identifier2.clone()));

        let result = repository.get_default_identity_name().await?;
        assert_eq!(result, Some("name2".into()));

        let result = repository.is_default_identity_by_name("name1").await?;
        assert!(!result);

        let result = repository.is_default_identity_by_name("name2").await?;
        assert!(result);
        Ok(())
    }

    /// HELPERS
    async fn create_repository(path: &Path) -> Result<Arc<dyn IdentitiesRepository>> {
        let db = SqlxDatabase::create(path).await?;
        Ok(Arc::new(IdentitiesSqlxDatabase::new(Arc::new(db))))
    }
}
