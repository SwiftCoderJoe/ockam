use crate::kafka::{
    KAFKA_SECURE_CHANNEL_CONTROLLER_ADDRESS, KAFKA_SECURE_CHANNEL_LISTENER_ADDRESS,
    ORCHESTRATOR_KAFKA_CONSUMERS,
};
use ockam::remote::RemoteForwarder;
use ockam_core::compat::collections::{HashMap, HashSet};
use ockam_core::compat::sync::Arc;
use ockam_core::compat::{join, join_all};
use ockam_core::errcode::{Kind, Origin};
use ockam_core::{async_trait, route, Address, AllowAll, Error, Result, Route, Routed, Worker};
use ockam_core::{AsyncTryClone, Message};
use ockam_identity::api::{
    DecryptionRequest, DecryptionResponse, EncryptionRequest, EncryptionResponse,
};
use ockam_identity::authenticated_storage::AuthenticatedStorage;
use ockam_identity::{Identity, IdentityVault, SecureChannelRegistryEntry, TrustEveryonePolicy};
use ockam_node::compat::futures::FutureExt;
use ockam_node::compat::tokio::sync::Mutex;
use ockam_node::Context;
use serde::{Deserialize, Serialize};

pub(crate) struct KafkaEncryptedContent {
    /// The encrypted content
    pub(crate) content: Vec<u8>,
    /// The secure channel id used to encrypt the content
    pub(crate) secure_channel_id: UniqueSecureChannelId,
}

/// Offer simple APIs to encrypt and decrypt kafka messages.
/// Underneath it creates secure channels for each topic/partition
/// and uses them to encrypt the content.
/// Multiple secure channels may be created for the same topic/partition
/// but each will be explicitly labelled.
/// It's the same for both producer and consumer although it could be split
/// into two distinct implementations.
/// This is a proxy trait to avoid propagating the vault implementation.
#[async_trait]
pub(crate) trait KafkaSecureChannelController: Send + Sync {
    /// Change the route for future connections and close existing ones.
    /// Forwarders are re-created as well as outgoing secure channels, however, incoming
    /// secure channel are **not** closed since they may be still useful to decode
    /// pending messages.
    /// This method is useful when connection goes down and a new route is created.
    async fn change_route(&self, context: &Context, new_route: Route) -> Result<()>;

    /// Encrypts the content specifically for the consumer waiting for that topic name and
    /// partition.
    /// To do so it'll create a secure channel which will be used for key exchange only.
    /// The secure channel will be created only once and then re-used, hence the first time will
    /// be slower, and may take up to few seconds.
    async fn encrypt_content_for(
        &self,
        context: &mut Context,
        topic_name: &str,
        partition_id: i32,
        content: Vec<u8>,
    ) -> Result<KafkaEncryptedContent>;

    /// Decrypts the content based on the unique secure channel identifier
    /// the secure channel is expected to be already initialized.
    async fn decrypt_content_for(
        &self,
        context: &mut Context,
        secure_channel_id: UniqueSecureChannelId,
        encrypted_content: Vec<u8>,
    ) -> Result<Vec<u8>>;

    /// Starts forwarders in the orchestrator for each {partition}_{topic_name} combination
    /// should be used only by the consumer.
    /// does nothing if they were already created, but fails it they already exist.
    async fn start_forwarders_for(
        &self,
        context: &mut Context,
        topic_id: &str,
        partitions: Vec<i32>,
    ) -> Result<()>;
}

#[async_trait]
pub(crate) trait ForwarderCreator: Send + Sync + 'static {
    /// Create a forwarder, if it exists it'll be replaced
    async fn create_forwarder(&self, context: &Context, alias: String) -> Result<()>;
}

pub(crate) struct RemoteForwarderCreator {
    hub_route: Route,
}

#[async_trait]
impl ForwarderCreator for RemoteForwarderCreator {
    async fn create_forwarder(&self, context: &Context, alias: String) -> Result<()> {
        trace!("creating remote forwarder for: {alias}");
        let remote_forwarder_information = RemoteForwarder::create_static(
            context,
            self.hub_route.clone(),
            alias.clone(),
            AllowAll,
        )
        .await?;
        trace!("remote forwarder created: {remote_forwarder_information:?}");
        Ok(())
    }
}

///Unique identifier for a specific secure_channel.
/// Used in order to distinguish between secure channels created between
/// the same identities.
#[derive(Debug, Clone, Serialize, Deserialize, Message)]
struct SecureChannelIdentifierMessage {
    secure_channel_identifier: UniqueSecureChannelId,
}

pub(crate) struct KafkaSecureChannelControllerImpl<
    V: IdentityVault,
    S: AuthenticatedStorage,
    F: ForwarderCreator,
> {
    inner: Arc<Mutex<InnerSecureChannelControllerImpl<V, S, F>>>,
}

//had to manually implement since #[derive(Clone)] doesn't work well in this situation
impl<V: IdentityVault, S: AuthenticatedStorage, F: ForwarderCreator> Clone
    for KafkaSecureChannelControllerImpl<V, S, F>
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

/// An identifier of the secure channel **instance**
pub(crate) type UniqueSecureChannelId = u64;
type TopicPartition = (String, i32);
struct InnerSecureChannelControllerImpl<
    V: IdentityVault,
    S: AuthenticatedStorage,
    F: ForwarderCreator,
> {
    //we are using encryptor api address as unique _local_ identifier
    //of the secure channel
    id_encryptor_map: HashMap<UniqueSecureChannelId, Address>,
    topic_encryptor_map: HashMap<TopicPartition, (UniqueSecureChannelId, Address)>,
    identity: Identity<V, S>,
    project_route: Route,
    topic_forwarder_set: HashSet<TopicPartition>,
    forwarder_creator: F,
}

impl<V: IdentityVault, S: AuthenticatedStorage>
    KafkaSecureChannelControllerImpl<V, S, RemoteForwarderCreator>
{
    pub(crate) fn new(
        identity: Identity<V, S>,
        project_route: Route,
    ) -> KafkaSecureChannelControllerImpl<V, S, RemoteForwarderCreator> {
        Self::new_extended(
            identity,
            project_route.clone(),
            RemoteForwarderCreator {
                hub_route: route![project_route, ORCHESTRATOR_KAFKA_CONSUMERS],
            },
        )
    }
}

impl<V: IdentityVault, S: AuthenticatedStorage, F: ForwarderCreator>
    KafkaSecureChannelControllerImpl<V, S, F>
{
    /// to manually specify `ForwarderCreator`, for testing purposes
    pub(crate) fn new_extended(
        identity: Identity<V, S>,
        project_route: Route,
        forwarder_creator: F,
    ) -> KafkaSecureChannelControllerImpl<V, S, F> {
        Self {
            inner: Arc::new(Mutex::new(InnerSecureChannelControllerImpl {
                id_encryptor_map: Default::default(),
                topic_encryptor_map: Default::default(),
                topic_forwarder_set: Default::default(),
                identity,
                forwarder_creator,
                project_route,
            })),
        }
    }

    pub(crate) async fn create_consumer_listener(&self, context: &Context) -> Result<()> {
        context
            .start_worker(
                Address::from_string(KAFKA_SECURE_CHANNEL_CONTROLLER_ADDRESS),
                SecureChannelControllerListener::<V, S, F> {
                    controller: self.clone(),
                },
                AllowAll,
                AllowAll,
            )
            .await
    }

    pub(crate) fn into_trait(self) -> Arc<dyn KafkaSecureChannelController> {
        Arc::new(self)
    }

    //add a mapping from remote producer
    async fn add_mapping(&self, id: UniqueSecureChannelId, encryptor_address: Address) {
        self.inner
            .lock()
            .await
            .id_encryptor_map
            .insert(id, encryptor_address);
    }
}

struct SecureChannelControllerListener<
    V: IdentityVault,
    S: AuthenticatedStorage,
    F: ForwarderCreator,
> {
    controller: KafkaSecureChannelControllerImpl<V, S, F>,
}

#[ockam::worker]
impl<V: IdentityVault, S: AuthenticatedStorage, F: ForwarderCreator> Worker
    for SecureChannelControllerListener<V, S, F>
{
    type Message = SecureChannelIdentifierMessage;
    type Context = Context;

    async fn handle_message(
        &mut self,
        context: &mut Self::Context,
        message: Routed<Self::Message>,
    ) -> Result<()> {
        //todo: is there a better way to extract it from the context?
        let encryptor_address = message.return_route().next().cloned()?;

        self.controller
            .add_mapping(message.secure_channel_identifier, encryptor_address.clone())
            .await;

        context.send(message.return_route(), ()).await
    }
}

impl<V: IdentityVault, S: AuthenticatedStorage, F: ForwarderCreator>
    KafkaSecureChannelControllerImpl<V, S, F>
{
    ///returns encryptor api address
    async fn get_or_create_secure_channel_for(
        &self,
        context: &Context,
        topic_name: impl Into<String>,
        partition: i32,
    ) -> Result<(UniqueSecureChannelId, SecureChannelRegistryEntry)> {
        let topic_name = topic_name.into();

        //here we should have the orchestrator address and expect forwarders to be
        // present in the orchestrator with the format "consumer_{partition}_{topic_name}"
        let topic_partition_key = (topic_name.to_string(), partition);

        //TODO: avoid locking while creating a secure channel itself but also allow a double
        // initialization and throwing away duplicated
        let mut inner = self.inner.lock().await;

        let (random_unique_id, encryptor_address) = {
            if let Some(encryptor_address) = inner.topic_encryptor_map.get(&topic_partition_key) {
                encryptor_address.clone()
            } else {
                //consumer__ prefix is added by the orchestrator
                let topic_partition_address = format!("consumer__{topic_name}_{partition}");
                trace!("creating new secure channel to {topic_partition_address}");

                let encryptor_address = inner
                    .identity
                    .create_secure_channel(
                        route![
                            inner.project_route.clone(),
                            topic_partition_address.clone(),
                            KAFKA_SECURE_CHANNEL_LISTENER_ADDRESS
                        ],
                        TrustEveryonePolicy,
                    )
                    .await?;

                trace!("created secure channel to {topic_partition_address}");

                let random_unique_id: UniqueSecureChannelId = rand::random();
                inner.topic_encryptor_map.insert(
                    topic_partition_key,
                    (random_unique_id, encryptor_address.clone()),
                );

                let message = SecureChannelIdentifierMessage {
                    secure_channel_identifier: random_unique_id,
                };

                //communicate to the other end the random id associated with this
                //secure channel, and wait to an empty reply to avoid race conditions
                //on the order of encryption/decryption of messages
                context
                    .send_and_receive(
                        route![
                            encryptor_address.clone(),
                            KAFKA_SECURE_CHANNEL_CONTROLLER_ADDRESS
                        ],
                        message,
                    )
                    .await?;

                trace!("assigned id {random_unique_id} to {topic_partition_address}");
                (random_unique_id, encryptor_address)
            }
        };

        inner
            .identity
            .secure_channel_registry()
            .get_channel_by_encryptor_address(&encryptor_address)
            .map(|entry| (random_unique_id, entry))
            .ok_or_else(|| Error::new(Origin::Channel, Kind::Unknown, "secure channel down"))
    }

    ///return decryptor api address
    async fn get_secure_channel_for(
        &self,
        secure_channel_id: UniqueSecureChannelId,
    ) -> Result<SecureChannelRegistryEntry> {
        let inner = self.inner.lock().await;
        if let Some(encryptor_address) = inner.id_encryptor_map.get(&secure_channel_id) {
            inner
                .identity
                .secure_channel_registry()
                .get_channel_list()
                .iter()
                .find(|entry| {
                    entry.encryptor_messaging_address() == encryptor_address
                        && !entry.is_initiator()
                })
                .cloned()
                .ok_or_else(|| {
                    Error::new(
                        Origin::Channel,
                        Kind::Unknown,
                        "secure channel no longer exists",
                    )
                })
        } else {
            Err(Error::new(
                Origin::Channel,
                Kind::Unknown,
                "missing secure channel",
            ))
        }
    }
}

#[async_trait]
impl<V: IdentityVault, S: AuthenticatedStorage, F: ForwarderCreator> KafkaSecureChannelController
    for KafkaSecureChannelControllerImpl<V, S, F>
{
    async fn change_route(&self, context: &Context, new_route: Route) -> Result<()> {
        // let mut secure_channels_to_recreate = Vec::new();
        let mut inner = self.inner.lock().await;

        inner.project_route = new_route;

        //since closing and re-creating every secure channel sequentially take a lot of time
        //we want parallelize as much as we can
        let mut create_secure_channel_futures = Vec::new();

        for ((topic_name, topic_partition), (_old_unique_id, encryptor_address)) in
            &inner.topic_encryptor_map
        {
            if let Err(error) = inner.identity.stop_secure_channel(encryptor_address).await {
                warn!("cannot stop secure channel: {error}");
            }

            create_secure_channel_futures.push(self.get_or_create_secure_channel_for(
                context,
                topic_name.clone(),
                *topic_partition,
            ));
        }

        //clear the encryptor map since we closed every secure channel
        inner.topic_encryptor_map.clear();

        //replacing existing forwarders
        for topic_key in &inner.topic_forwarder_set {
            let (topic_name, partition) = topic_key;
            let alias = format!("{topic_name}_{partition}");
            inner
                .forwarder_creator
                .create_forwarder(context, alias)
                .await?;
        }

        //since the creation of new secure channels requires the lock
        //we first drop it and then start awaiting for results
        drop(inner);

        //creation of new secure channels is an optional operation
        //if something went wrong we can just log it
        for create_secure_channel_result in join_all(create_secure_channel_futures).await {
            if let Err(error) = create_secure_channel_result {
                warn!("cannot pre-create secure channel: {error}");
            }
        }

        Ok(())
    }

    async fn encrypt_content_for(
        &self,
        context: &mut Context,
        topic_name: &str,
        partition_id: i32,
        content: Vec<u8>,
    ) -> Result<KafkaEncryptedContent> {
        let (unique_id, secure_channel_entry) = self
            .get_or_create_secure_channel_for(context, topic_name, partition_id)
            .await?;

        trace!("encrypting content with {unique_id}");
        let encryption_response: EncryptionResponse = context
            .send_and_receive(
                route![secure_channel_entry.encryptor_api_address().clone()],
                EncryptionRequest(content),
            )
            .await?;

        let encrypted_content = match encryption_response {
            EncryptionResponse::Ok(p) => p,
            EncryptionResponse::Err(cause) => {
                warn!("cannot encrypt kafka message");
                return Err(cause);
            }
        };

        trace!("encrypted content with {unique_id}");
        Ok(KafkaEncryptedContent {
            content: encrypted_content,
            secure_channel_id: unique_id,
        })
    }

    async fn decrypt_content_for(
        &self,
        context: &mut Context,
        secure_channel_id: UniqueSecureChannelId,
        encrypted_content: Vec<u8>,
    ) -> Result<Vec<u8>> {
        let secure_channel_entry = self.get_secure_channel_for(secure_channel_id).await?;

        let decrypt_response = context
            .send_and_receive(
                route![secure_channel_entry.decryptor_api_address().clone()],
                DecryptionRequest(encrypted_content),
            )
            .await?;

        let decrypted_content = match decrypt_response {
            DecryptionResponse::Ok(p) => p,
            DecryptionResponse::Err(cause) => {
                error!("cannot decrypt kafka message: closing connection");
                return Err(cause);
            }
        };

        Ok(decrypted_content)
    }

    async fn start_forwarders_for(
        &self,
        context: &mut Context,
        topic_name: &str,
        partitions: Vec<i32>,
    ) -> Result<()> {
        let mut inner = self.inner.lock().await;

        for partition in partitions {
            let topic_key: TopicPartition = (topic_name.to_string(), partition);
            if inner.topic_forwarder_set.contains(&topic_key) {
                continue;
            }
            let alias = format!("{topic_name}_{partition}");
            inner
                .forwarder_creator
                .create_forwarder(context, alias)
                .await?;
            inner.topic_forwarder_set.insert(topic_key);
        }

        Ok(())
    }
}
