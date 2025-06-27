use std::sync::Arc;

use shared::{
    connections::SteckerWebRTCConnection,
    models::{
        DataRoomInternalType, DataRoomPublicType, SteckerData, SteckerDataChannelEvent,
        SteckerDataChannelType,
    },
};
use tokio::sync::broadcast::Sender;
use tracing::{info, instrument, Instrument, Span};
use uuid::Uuid;

use super::room::{BroadcastRoom, BroadcastRoomMeta, ResponseOffer};

// server state objects
pub struct DataBroadcastRoom {
    pub meta: BroadcastRoomMeta,

    pub events: Sender<SteckerDataChannelEvent>,

    // /// Reply to server (messages not broadcasted)
    // /// potentially not interesting to subscribe to this
    // pub reply: Sender<SteckerData>,
    // /// Subscribe to this to receive messages from room
    // /// potentially not useful to send to this (unless you also become a broadcaster)
    // pub broadcast: Sender<SteckerData>,

    // @todo this data type could be optimized?
    pub room_type: DataRoomPublicType,
}

pub struct BroadcastRoomWithOffer {
    pub broadcast_room: DataBroadcastRoom,
    pub offer: ResponseOffer,
}

impl DataBroadcastRoom {
    #[instrument(skip_all, err)]
    pub async fn create_room(
        name: String,
        offer: String,
        room_type: DataRoomPublicType,
        password: String,
    ) -> anyhow::Result<BroadcastRoomWithOffer> {
        // @todo attach this to current span
        let uuid = Uuid::new_v4();

        let connection = SteckerWebRTCConnection::build_connection()
            .in_current_span()
            .await?;
        let meta_channel = connection.register_channel(&DataRoomInternalType::Meta);
        let data_channel = connection.register_channel(&(room_type.into()));

        let response_offer = connection.respond_to_offer(offer).in_current_span().await?;

        let (num_listeners_sender, _) = tokio::sync::watch::channel(0);

        let data_channel_events = data_channel.events.subscribe();
        let connection_events = connection.event_service.subscribe();

        let mut num_listeners_changed = num_listeners_sender.subscribe();
        let outgoing_message_sender = async move {
            loop {
                num_listeners_changed.changed().await;
                info!("Number of listeners changed for a channel!");
            }
        };

        let broadcast_room = DataBroadcastRoom {
            meta: BroadcastRoomMeta {
                name: name,
                uuid: uuid,
                num_listeners: num_listeners_sender,
                // _num_listeners_receiver: num_listeners_receiver,
                admin_password: password,
            },
            room_type: room_type,
            events: data_channel.events.clone(),
        };

        Ok(BroadcastRoomWithOffer {
            broadcast_room,
            offer: response_offer,
        })
    }

    #[instrument(skip_all, err)]
    pub async fn join_room(&self, offer: &str) -> anyhow::Result<ResponseOffer> {
        let connection = Arc::new(SteckerWebRTCConnection::build_connection().await?);
        let response_offer = connection.respond_to_offer(offer.to_string()).await?;

        let meta_channel = connection.register_channel(&DataRoomInternalType::Meta);
        let stecker_data_channel = connection.register_channel(&self.room_type.into());

        let mut num_listeners_receiver = self.meta.num_listeners.subscribe();
        let mut channel_events = self.events.subscribe();

        let connection2 = connection.clone();
        let outgoing_message_sender = async move {
            let channel = connection2.wait_for_data_channel(&room_type).await.unwrap();
            // channel.send(data);
            loop {
                tokio::select! {
                    Ok(event) = channel_events.recv() => {
                        match event {
                            SteckerDataChannelEvent::InboundMessage(stecker_data) => {
                                // @todo send out to connection
                                channel;
                            },
                            SteckerDataChannelEvent::ConnectionClosed => {
                                break;
                            },
                            _ => {},
                        }
                    },
                }
            }
        };
        Ok(response_offer)
    }
}
