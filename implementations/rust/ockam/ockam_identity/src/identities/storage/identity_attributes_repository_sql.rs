use core::str::FromStr;
use std::collections::BTreeMap;

use sqlx::*;

use ockam_core::async_trait;
use ockam_core::compat::sync::Arc;
use ockam_core::Result;
use ockam_node::database::{FromSqlxError, SqlxDatabase, ToSqlxType, ToVoid};

use crate::models::Identifier;
use crate::utils::now;
use crate::{AttributesEntry, IdentityAttributesRepository, TimestampInSeconds};

/// Implementation of `IdentitiesRepository` trait based on an underlying database
/// using sqlx as its API, and Sqlite as its driver
#[derive(Clone)]
pub struct IdentityAttributesSqlxDatabase {
    database: Arc<SqlxDatabase>,
}

impl IdentityAttributesSqlxDatabase {
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
impl IdentityAttributesRepository for IdentityAttributesSqlxDatabase {
    async fn get_attributes(&self, identity: &Identifier) -> Result<Option<AttributesEntry>> {
        let query = query_as("SELECT * FROM identity_attributes WHERE identifier=$1")
            .bind(identity.to_sql());
        let identity_attributes: Option<IdentityAttributesRow> = query
            .fetch_optional(&self.database.pool)
            .await
            .into_core()?;
        Ok(identity_attributes.map(|r| r.attributes()).transpose()?)
    }

    async fn list(&self) -> Result<Vec<(Identifier, AttributesEntry)>> {
        let query = query_as("SELECT * FROM identity_attributes");
        let result: Vec<IdentityAttributesRow> =
            query.fetch_all(&self.database.pool).await.into_core()?;
        result
            .into_iter()
            .map(|r| r.identifier().and_then(|i| r.attributes().map(|a| (i, a))))
            .collect::<Result<Vec<_>>>()
    }

    async fn put_attributes(&self, sender: &Identifier, entry: AttributesEntry) -> Result<()> {
        let query = query("INSERT OR REPLACE INTO identity_attributes VALUES (?, ?, ?, ?, ?)")
            .bind(sender.to_sql())
            .bind(minicbor::to_vec(entry.attrs())?.to_sql())
            .bind(entry.added().to_sql())
            .bind(entry.expires().map(|e| e.to_sql()))
            .bind(entry.attested_by().map(|e| e.to_sql()));
        query.execute(&self.database.pool).await.void()
    }

    /// Store an attribute name/value pair for a given identity
    async fn put_attribute_value(
        &self,
        subject: &Identifier,
        attribute_name: Vec<u8>,
        attribute_value: Vec<u8>,
    ) -> Result<()> {
        let transaction: Transaction<'static, Sqlite> =
            self.database.pool.begin().await.into_core()?;

        let mut attributes = match self.get_attributes(subject).await? {
            Some(entry) => (*entry.attrs()).clone(),
            None => BTreeMap::new(),
        };
        attributes.insert(attribute_name, attribute_value);
        let entry = AttributesEntry::new(attributes, now()?, None, Some(subject.clone()));
        self.put_attributes(subject, entry).await?;

        transaction.commit().await.into_core()
    }

    async fn delete(&self, identity: &Identifier) -> Result<()> {
        let query =
            query("DELETE FROM identity_attributes WHERE identifier = ?").bind(identity.to_sql());
        query.execute(&self.database.pool).await.void()
    }
}

#[derive(FromRow)]
struct IdentityAttributesRow {
    identifier: String,
    attributes: Vec<u8>,
    added: i64,
    expires: Option<i64>,
    attested_by: Option<String>,
}

impl IdentityAttributesRow {
    fn identifier(&self) -> Result<Identifier> {
        Identifier::from_str(&self.identifier)
    }

    fn attributes(&self) -> Result<AttributesEntry> {
        let attributes =
            minicbor::decode(self.attributes.as_slice()).map_err(SqlxDatabase::map_decode_err)?;
        let added = TimestampInSeconds(self.added as u64);
        let expires = self.expires.map(|v| TimestampInSeconds(v as u64));
        let attested_by = self
            .attested_by
            .clone()
            .map(|v| Identifier::from_str(&v))
            .transpose()?;

        Ok(AttributesEntry::new(
            attributes,
            added,
            expires,
            attested_by,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::time::Duration;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_identities_attributes_repository() -> Result<()> {
        let identifier1 =
            Identifier::from_str("Ie92f183eb4c324804ef4d62962dea94cf095a265").unwrap();

        let attributes = create_attributes_entry().await?;
        let db_file = NamedTempFile::new().unwrap();
        let repository = create_repository(db_file.path()).await?;

        // store and retrieve attributes by identity
        repository
            .put_attributes(&identifier1, attributes.clone())
            .await?;

        let result = repository.list().await?;
        assert_eq!(result, vec![(identifier1.clone(), attributes.clone())]);

        let result = repository.get_attributes(&identifier1).await?;
        assert_eq!(result, Some(attributes));

        // delete attributes
        let _ = repository.delete(&identifier1).await?;
        let result = repository.get_attributes(&identifier1).await?;
        assert_eq!(result, None);

        // store just one attribute name / value
        let before_adding = now()?;
        repository
            .put_attribute_value(
                &identifier1,
                "name".as_bytes().to_vec(),
                "value".as_bytes().to_vec(),
            )
            .await?;

        let result = repository.get_attributes(&identifier1).await?.unwrap();
        // the name/value pair is present
        assert_eq!(
            result.attrs().get("name".as_bytes()),
            Some(&"value".as_bytes().to_vec())
        );
        // there is a timestamp showing when the attributes have been added
        assert!(result.added() >= before_adding);

        // the attributes are self-attested
        assert_eq!(result.attested_by(), Some(identifier1.clone()));

        // store one more attribute name / value
        // Let time pass for bit to observe a timestamp update
        // We need to wait at least one second since this is the granularity of the
        // timestamp for tracking attributes
        tokio::time::sleep(Duration::from_millis(1100)).await;
        repository
            .put_attribute_value(
                &identifier1,
                "name2".as_bytes().to_vec(),
                "value2".as_bytes().to_vec(),
            )
            .await?;

        let result2 = repository.get_attributes(&identifier1).await?.unwrap();

        // both the new and the old name/value pairs are present
        assert_eq!(
            result2.attrs().get("name".as_bytes()),
            Some(&"value".as_bytes().to_vec())
        );
        assert_eq!(
            result2.attrs().get("name2".as_bytes()),
            Some(&"value2".as_bytes().to_vec())
        );
        // The original timestamp has been updated
        assert!(result2.added() > result.added());

        // the attributes are still self-attested
        assert_eq!(result2.attested_by(), Some(identifier1.clone()));
        Ok(())
    }

    /// HELPERS
    async fn create_attributes_entry() -> Result<AttributesEntry> {
        let identifier1 =
            Identifier::from_str("Ie92f183eb4c324804ef4d62962dea94cf095a265").unwrap();
        Ok(AttributesEntry::new(
            BTreeMap::from([
                ("name".as_bytes().to_vec(), "alice".as_bytes().to_vec()),
                ("age".as_bytes().to_vec(), "20".as_bytes().to_vec()),
            ]),
            TimestampInSeconds(1000),
            Some(TimestampInSeconds(2000)),
            Some(identifier1.clone()),
        ))
    }

    async fn create_repository(path: &Path) -> Result<Arc<dyn IdentityAttributesRepository>> {
        let db = SqlxDatabase::create(path).await?;
        Ok(Arc::new(IdentityAttributesSqlxDatabase::new(Arc::new(db))))
    }
}
