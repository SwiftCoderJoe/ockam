use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::sync::Arc;

use ockam::identity::Vault;
use ockam_core::async_trait;
use ockam_core::Result;
use ockam_vault_aws::AwsSigningVault;

#[async_trait]
pub trait VaultsRepository: Send + Sync + 'static {
    async fn store_vault(&self, name: &str, path: PathBuf, is_aws_kms: bool) -> Result<()>;
    async fn delete_vault(&self, name: &str) -> Result<()>;
    async fn set_as_default(&self, name: &str) -> Result<()>;
    async fn is_default(&self, name: &str) -> Result<bool>;
    async fn get_named_vaults(&self) -> Result<Vec<NamedVault>>;
    async fn get_vault_by_name(&self, name: &str) -> Result<Option<NamedVault>>;
    async fn get_default_vault(&self) -> Result<Option<NamedVault>>;
    async fn get_default_vault_name(&self) -> Result<Option<String>>;
}

#[derive(Debug, PartialEq, Eq, Clone, serde::Serialize, serde::Deserialize)]
pub struct NamedVault {
    name: String,
    path: PathBuf,
    is_aws_kms: bool,
    is_default: bool,
}

impl NamedVault {
    pub fn new(name: String, path: PathBuf, is_default: bool, is_aws_kms: bool) -> Self {
        Self {
            name,
            path,
            is_default,
            is_aws_kms,
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

    pub fn is_aws_kms(&self) -> bool {
        self.is_aws_kms
    }

    pub async fn vault(&self) -> Result<Vault> {
        if self.is_aws_kms {
            let mut vault = Vault::create();
            let aws_vault = Arc::new(AwsSigningVault::create().await?);
            vault.identity_vault = aws_vault.clone();
            vault.credential_vault = aws_vault;
            Ok(vault)
        } else {
            Ok(Vault::create_with_persistent_storage_path(self.path.as_path()).await?)
        }
    }
}

impl Display for NamedVault {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Name: {}", self.name)?;
        writeln!(
            f,
            "Type: {}",
            match self.is_aws_kms {
                true => "AWS KMS",
                false => "OCKAM",
            }
        )?;
        Ok(())
    }
}
