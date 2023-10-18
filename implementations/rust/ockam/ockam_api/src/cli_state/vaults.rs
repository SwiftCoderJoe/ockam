use std::path::PathBuf;

use ockam::identity::Vault;
use ockam_core::errcode::{Kind, Origin};

use crate::cli_state::CliState;
use crate::identity::NamedVault;

use super::Result;

impl CliState {
    /// Create a vault with the given name if it was not created before
    /// If no name is given use "default" as the default vault name
    /// If the vault was not created before return Ok(vault)
    /// Otherwise return Err(name of the vault)
    pub async fn create_named_vault(
        &self,
        vault_name: &Option<String>,
    ) -> Result<std::result::Result<NamedVault, String>> {
        let vault_name = vault_name.clone().unwrap_or("default".to_string());
        if self.get_named_vault(&vault_name).await.is_ok() {
            Ok(Err(vault_name))
        } else {
            self.create_vault(&vault_name, None, false).await?;
            Ok(Ok(self.get_named_vault(&vault_name).await?))
        }
    }

    pub async fn create_vault(
        &self,
        vault_name: &str,
        path: Option<String>,
        is_aws_kms: bool,
    ) -> Result<()> {
        let vaults_repository = self.vaults_repository().await?;
        let is_default = vaults_repository.get_named_vaults().await?.is_empty();

        // if a path is not specified use the database to store secrets
        let path = path.map(PathBuf::from).unwrap_or(self.database_path());
        vaults_repository
            .store_vault(vault_name, path, is_aws_kms)
            .await?;
        if is_default {
            vaults_repository.set_as_default(vault_name).await?;
        }
        Ok(())
    }

    pub async fn is_default_vault(&self, vault_name: &str) -> Result<bool> {
        Ok(self
            .vaults_repository()
            .await?
            .is_default(vault_name)
            .await?)
    }

    pub async fn set_default_vault(&self, vault_name: &str) -> Result<()> {
        Ok(self
            .vaults_repository()
            .await?
            .set_as_default(vault_name)
            .await?)
    }

    pub async fn get_vault_names(&self) -> Result<Vec<String>> {
        let named_vaults = self.vaults_repository().await?.get_named_vaults().await?;
        Ok(named_vaults.iter().map(|v| v.name()).collect())
    }

    pub async fn get_named_vaults(&self) -> Result<Vec<NamedVault>> {
        Ok(self.vaults_repository().await?.get_named_vaults().await?)
    }

    /// Return either the default vault or a vault with the given name
    pub async fn get_vault_or_default(&self, vault_name: &Option<String>) -> Result<Vault> {
        let vault_name = self.get_vault_name_or_default(vault_name).await?;
        Ok(self.get_named_vault(&vault_name).await?.vault().await?)
    }

    /// Return either the default vault or a vault with the given name
    pub async fn get_named_vault_or_default(
        &self,
        vault_name: &Option<String>,
    ) -> Result<NamedVault> {
        let vault_name = self.get_vault_name_or_default(vault_name).await?;
        Ok(self.get_named_vault(&vault_name).await?)
    }

    /// Return the vault with the given name
    pub async fn get_vault_by_name(&self, vault_name: &str) -> Result<Vault> {
        Ok(self.get_named_vault(&vault_name).await?.vault().await?)
    }

    pub async fn get_vault_name_or_default(&self, vault_name: &Option<String>) -> Result<String> {
        match vault_name {
            Some(name) => Ok(name.clone()),
            None => self.get_default_vault_name().await,
        }
    }

    pub async fn get_named_vault(&self, vault_name: &str) -> Result<NamedVault> {
        let result = self
            .vaults_repository()
            .await?
            .get_vault_by_name(vault_name)
            .await?;
        result.ok_or_else(|| {
            ockam_core::Error::new(
                Origin::Api,
                Kind::NotFound,
                format!("no vault found with name {vault_name}"),
            )
            .into()
        })
    }

    pub(crate) async fn get_default_vault(&self) -> Result<NamedVault> {
        let result = self.vaults_repository().await?.get_default_vault().await?;
        result.ok_or_else(|| {
            ockam_core::Error::new(
                Origin::Api,
                Kind::NotFound,
                format!("no default vault found"),
            )
            .into()
        })
    }

    pub(crate) async fn get_default_vault_name(&self) -> Result<String> {
        let result = self
            .vaults_repository()
            .await?
            .get_default_vault_name()
            .await?;
        result.ok_or_else(|| {
            ockam_core::Error::new(
                Origin::Api,
                Kind::NotFound,
                format!("no default vault found"),
            )
            .into()
        })
    }

    /// Return the vault with the given name
    pub async fn delete_vault(&self, vault_name: &str) -> Result<()> {
        Ok(self
            .vaults_repository()
            .await?
            .delete_vault(vault_name)
            .await?)
    }
}
