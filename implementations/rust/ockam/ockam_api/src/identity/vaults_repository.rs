use ockam::identity::Vault;
use ockam_core::async_trait;
use ockam_core::Result;
use std::path::PathBuf;

#[async_trait]
pub trait VaultsRepository: Send + Sync + 'static {
    async fn name_vault(&self, name: &str, path: PathBuf) -> Result<()>;
    async fn get_vault_by_name(&self, name: &str) -> Result<Option<NamedVault>>;
    async fn get_default_vault(&self) -> Result<Option<NamedVault>>;
    async fn get_default_vault_name(&self) -> Result<Option<String>>;
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
