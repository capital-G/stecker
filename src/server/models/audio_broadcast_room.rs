use std::sync::Arc;

use shared::{
    connections::SteckerWebRTCConnection,
    event_service::SteckerConnectionEvent,
    models::{DataRoomInternalType, SteckerAudioChannel},
};
use tokio::sync::broadcast::Sender;
use tracing::{error, info, trace, Instrument};
use webrtc::{
    peer_connection::{self, peer_connection_state::RTCPeerConnectionState},
    track::track_local::{track_local_static_rtp::TrackLocalStaticRTP, TrackLocalWriter},
};

use super::room::BroadcastRoomMeta;

pub enum AudioBroadcastRoomEvent {}

#[derive(Debug)]
pub struct AudioBroadcastRoom {
    pub meta: BroadcastRoomMeta,
    pub stecker_audio_channel: SteckerAudioChannel,
}

pub struct AudioBroadcastRoomWithOffer {
    pub audio_broadcast_room: AudioBroadcastRoom,
    pub offer: String,
    // pub disconnected: Sender<()>,
}

impl AudioBroadcastRoom {
    pub async fn create_room(
        name: String,
        offer: String,
        admin_password: String,
    ) -> anyhow::Result<AudioBroadcastRoomWithOffer> {
        let connection = SteckerWebRTCConnection::build_connection()
            .in_current_span()
            .await?;
        let audio_channel = Arc::new(SteckerAudioChannel::new());
        let meta_channel = connection.register_channel(&DataRoomInternalType::Meta);
        let response_offer = connection.respond_to_offer(offer).in_current_span().await?;

        let audio_channel2 = audio_channel.clone();
        let connection2 = connection.clone();
        let consume_audio_track = async move {
            let remote_track = connection2.wait_for_audio_track().await?;
            let connection_events = connection2.event_service.subscribe();

            let local_track = Arc::new(TrackLocalStaticRTP::new(
                remote_track.codec().capability,
                "audio".to_owned(),
                "stecker".to_owned(),
            ));
            audio_channel2.change_sender(local_track);

            loop {
                tokio::select! {
                    Ok((rtp, _)) = remote_track.read_rtp() => {
                        let _ = audio_channel2.sequence_number_sender.send(rtp.header.sequence_number);
                        local_track.write_rtp(&rtp).await;
                    },
                    event = connection_events.recv().await => {
                        trace!("Something happened");
                        break;
                    }
                }
            }
        };
        let _ = tokio::spawn(consume_audio_track).await;

        return Ok(AudioBroadcastRoomWithOffer {
            offer: response_offer,
            // disconnected,
            audio_broadcast_room: Self {
                stecker_audio_channel: audio_channel,
                meta: BroadcastRoomMeta {
                    name: name,
                    uuid: Uuid::new_v4(),
                    // meta_broadcast: meta_channel.inbound.clone(),
                    // meta_reply: meta_channel.outbound.clone(),
                    num_listeners: num_listeners_sender,
                    // _num_listeners_receiver: num_listeners_receiver,
                    admin_password,
                },
            },
        });
    }

    pub async fn join_room(&self, offer: &str) -> anyhow::Result<ResponseOffer> {
        let connection = SteckerWebRTCConnection::build_connection().await?;
        let _meta_channel = connection.register_channel(&DataRoomInternalType::Meta);

        let audio_track_receiver = self
            .stecker_audio_channel
            .audio_channel_tx
            .subscribe()
            .borrow()
            .clone();

        match audio_track_receiver {
            Some(audio_track) => {
                trace!("Found an audio track");
                let _ = connection.add_existing_audio_track(audio_track).await;
                let response_offer = connection.respond_to_offer(offer.to_owned()).await?;
                Ok(response_offer)
            }
            None => Err(anyhow::anyhow!(
                "Have not received an audio track from the sender yet - try later"
            )),
        }
    }

    pub async fn replace_sender(&self, offer: String) -> anyhow::Result<ResponseOffer> {
        info!("Replace audio sender");
        let connection = SteckerWebRTCConnection::build_connection()
            .in_current_span()
            .await?;
        let response_offer = connection.respond_to_offer(offer).in_current_span().await?;
        let mut audio_track_receiver = connection
            .listen_for_remote_audio_track()
            .in_current_span()
            .await;

        let local_track = if let Some(track) = self
            .stecker_audio_channel
            .audio_channel_tx
            .subscribe()
            .borrow()
            .clone()
        {
            track
        } else {
            return Err(anyhow::anyhow!(
                "Room has not been sucessfully set up, can not take it over."
            ));
        };

        let _ = self.stecker_audio_channel.reset_sender.send(());

        let mut stop_consuming = self.stecker_audio_channel.reset_sender.subscribe();
        let seq_number_sender = self.stecker_audio_channel.sequence_number_sender.clone();
        let mut seq_number_receiver = self.stecker_audio_channel.sequence_number_receiver.clone();
        let _ = *seq_number_receiver.borrow_and_update();

        tokio::spawn(
            async move {
                let track = audio_track_receiver.recv().await.unwrap();
                let ssrc = track.ssrc();
                trace!(ssrc, "Start consuming new audio track");

                let mut last_seq: u16 = (*seq_number_receiver.borrow_and_update()).clone();

                loop {
                    tokio::select! {
                        result = track.read_rtp() => {
                            if let Ok((mut rtp, _)) = result {
                                // we need to reorder RTP packages b/c otherwise the client will
                                // think there was a package drop b/c of a gap in the seq order
                                last_seq = last_seq.wrapping_add(1);
                                rtp.header.sequence_number = last_seq;
                                let _ = seq_number_sender.send(last_seq);
                                let _ = local_track.write_rtp(&rtp).await;
                            } else {
                                error!("Failed to read track - stop consuming");
                                break;
                            }
                       },
                       _ = stop_consuming.recv() => {
                            info!("Got signal to terminate consuming the current track");
                            break;
                        }
                    }
                }
            }
            .in_current_span(),
        );

        Ok(response_offer)
    }
}
