use ockam_core::compat::sync::Arc;
use ockam_core::{Address, AllowAll, Any, Route, Routed, Worker};
use ockam_node::Context;
use ockam_transport_tcp::TcpTransport;
use tracing::trace;

use crate::kafka::inlet_map::KafkaInletMap;
use crate::kafka::portal_worker::KafkaPortalWorker;
use crate::kafka::protocol_aware::TopicUuidMap;
use crate::kafka::secure_channel_map::KafkaSecureChannelController;
use crate::port_range::PortRange;

///First point of ingress of kafka connections, at the first message it spawns new stateful workers
/// to take care of the connection.
pub(crate) struct KafkaPortalListener {
    inlet_map: KafkaInletMap,
    secure_channel_controller: Arc<dyn KafkaSecureChannelController>,
    uuid_to_name: TopicUuidMap,
}

#[ockam::worker]
impl Worker for KafkaPortalListener {
    type Message = Any;
    type Context = Context;

    async fn handle_message(
        &mut self,
        context: &mut Self::Context,
        message: Routed<Self::Message>,
    ) -> ockam::Result<()> {
        trace!("received first message!");
        let worker_address = KafkaPortalWorker::start_kafka_portal(
            context,
            self.secure_channel_controller.clone(),
            self.uuid_to_name.clone(),
            self.inlet_map.clone(),
        )
        .await?;

        //forward to the worker and place its address in the return route
        let mut message = message.into_local_message();

        message
            .transport_mut()
            .onward_route
            .modify()
            .replace(worker_address.clone());

        trace!(
            "forwarding message: onward={:?}; return={:?}; worker={:?}",
            &message.transport().onward_route,
            &message.transport().return_route,
            worker_address
        );

        context.forward(message).await?;

        Ok(())
    }
}

impl KafkaPortalListener {
    pub(crate) async fn create(
        context: &Context,
        secure_channel_controller: Arc<dyn KafkaSecureChannelController>,
        listener_address: Address,
        inlet_map: KafkaInletMap,
    ) -> ockam_core::Result<()> {
        context
            .start_worker(
                listener_address,
                Self {
                    inlet_map,
                    secure_channel_controller,
                    uuid_to_name: Default::default(),
                },
                AllowAll,
                AllowAll,
            )
            .await
    }
}
