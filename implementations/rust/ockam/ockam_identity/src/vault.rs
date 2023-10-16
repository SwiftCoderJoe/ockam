use ockam_core::compat::sync::Arc;
use ockam_node::database::SqlxDatabase;
use ockam_vault::storage::{SecretsRepository, SecretsSqlxDatabase};
use ockam_vault::{
    SoftwareVaultForSecureChannels, SoftwareVaultForSigning, SoftwareVaultForVerifyingSignatures,
    VaultForSecureChannels, VaultForSigning, VaultForVerifyingSignatures,
};

/// Vault
#[derive(Clone)]
pub struct Vault {
    /// Vault used for Identity Keys
    pub identity_vault: Arc<dyn VaultForSigning>,
    /// Vault used for Secure Channels
    pub secure_channel_vault: Arc<dyn VaultForSecureChannels>,
    /// Vault used for signing Credentials
    pub credential_vault: Arc<dyn VaultForSigning>,
    /// Vault used for verifying signature and sha256
    pub verifying_vault: Arc<dyn VaultForVerifyingSignatures>,
}

impl Vault {
    /// Constructor
    pub fn new(
        identity_vault: Arc<dyn VaultForSigning>,
        secure_channel_vault: Arc<dyn VaultForSecureChannels>,
        credential_vault: Arc<dyn VaultForSigning>,
        verifying_vault: Arc<dyn VaultForVerifyingSignatures>,
    ) -> Self {
        Self {
            identity_vault,
            secure_channel_vault,
            credential_vault,
            verifying_vault,
        }
    }

    /// Create Software implementation Vault with an in-memory storage
    pub fn create() -> Self {
        Self::new(
            Self::create_identity_vault(),
            Self::create_secure_channel_vault(),
            Self::create_credential_vault(),
            Self::create_verifying_vault(),
        )
    }

    /// Create [`SoftwareVaultForSigning`] with an in-memory storage
    pub fn create_identity_vault() -> Arc<dyn VaultForSigning> {
        Arc::new(SoftwareVaultForSigning::new(SecretsSqlxDatabase::create()))
    }

    /// Create [`SoftwareSecureChannelVault`] with an in-memory storage
    pub fn create_secure_channel_vault() -> Arc<dyn VaultForSecureChannels> {
        Arc::new(SoftwareVaultForSecureChannels::new(
            SecretsSqlxDatabase::create(),
        ))
    }

    /// Create [`SoftwareVaultForSigning`] with an in-memory storage
    pub fn create_credential_vault() -> Arc<dyn VaultForSigning> {
        Arc::new(SoftwareVaultForSigning::new(SecretsSqlxDatabase::create()))
    }

    /// Create [`SoftwareVaultForVerifyingSignatures`]
    pub fn create_verifying_vault() -> Arc<dyn VaultForVerifyingSignatures> {
        Arc::new(SoftwareVaultForVerifyingSignatures {})
    }
}

impl Vault {
    /// Create Software Vaults and persist them to a given path
    #[cfg(feature = "std")]
    pub async fn create_with_persistent_storage_path(
        path: &std::path::Path,
    ) -> ockam_core::Result<Vault> {
        let database = Arc::new(SqlxDatabase::create(path).await?);
        Ok(Self::create_with_secrets_repository(Arc::new(
            SecretsSqlxDatabase::new(database),
        )))
    }

    /// Create Software Vaults with a given secrets repository
    pub fn create_with_secrets_repository(repository: Arc<dyn SecretsRepository>) -> Vault {
        Self::new(
            Arc::new(SoftwareVaultForSigning::new(repository.clone())),
            Arc::new(SoftwareVaultForSecureChannels::new(repository.clone())),
            Arc::new(SoftwareVaultForSigning::new(repository.clone())),
            Arc::new(SoftwareVaultForVerifyingSignatures {}),
        )
    }
}
