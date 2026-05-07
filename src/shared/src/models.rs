use std::{
    collections::HashMap,
    fmt::Display,
    sync::{Arc, Mutex},
    time::Duration,
};

use bytes::{Buf, BufMut, Bytes, BytesMut};
use tokio::{
    sync::{
        broadcast::{self, Sender},
        watch,
    },
    time::sleep,
};
use tracing::{debug, info, instrument, trace, Instrument, Span};
use webrtc::{
    data_channel::{data_channel_message::DataChannelMessage, RTCDataChannel},
    peer_connection::{self, RTCPeerConnection},
    track::track_local::track_local_static_rtp::TrackLocalStaticRTP,
};

use crate::connections::SteckerWebRTCConnection;

// @todo use cargo.toml version
pub static API_VERSION: &'static str = "0.1.0";

/// the possible kinds of data rooms used
#[derive(Debug, Clone, Copy)]
pub struct RoomFloatData;
#[derive(Debug, Clone, Copy)]
pub struct RoomStringData;

pub trait SteckerData {
    type Payload: Clone + Send + 'static;

    /// each data channel has a label - this is used to identify
    /// what kind of channel we are publishing or receiving.
    fn label() -> String;

    fn encode(value: Self::Payload) -> anyhow::Result<Bytes>;
    fn decode(message: DataChannelMessage) -> anyhow::Result<Self::Payload>;
    fn matches_data_channel(data_channel: &Arc<RTCDataChannel>) -> bool {
        data_channel.label() == Self::label()
    }
}

impl SteckerData for RoomFloatData {
    type Payload = f32;

    fn encode(value: Self::Payload) -> anyhow::Result<Bytes> {
        let mut b = BytesMut::with_capacity(4);
        b.put_f32(value);
        Ok(b.freeze())
    }

    fn decode(message: DataChannelMessage) -> anyhow::Result<Self::Payload> {
        let mut b = message.data.clone();
        Ok(Bytes::get_f32(&mut b))
    }

    fn label() -> String {
        "FLOAT".to_string()
    }
}
impl SteckerData for RoomStringData {
    type Payload = String;

    fn encode(value: Self::Payload) -> anyhow::Result<Bytes> {
        Ok(value.clone().into())
    }

    fn decode(message: DataChannelMessage) -> anyhow::Result<Self::Payload> {
        Ok(String::from_utf8(message.data.to_vec())?)
    }

    fn label() -> String {
        "STRING".to_string()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RoomAudioData;

impl SteckerData for RoomAudioData {
    type Payload = ();

    fn label() -> String {
        "AUDIO".to_string()
    }

    fn encode(_value: Self::Payload) -> anyhow::Result<Bytes> {
        anyhow::bail!("Audio rooms do not use data channel encoding")
    }

    fn decode(_message: DataChannelMessage) -> anyhow::Result<Self::Payload> {
        anyhow::bail!("Audio rooms do not use data channel decoding")
    }
}

#[derive(Clone)]
pub enum DataChannelEvent {
    OpenedConnection,
    ClosedConnection,
}

#[derive(Clone, Debug)]
pub struct SteckerDataChannel<T: SteckerData> {
    /// messages received from data channel are inbound,
    pub inbound: Sender<T::Payload>,
    /// messages send to data channel are outbound
    pub outbound: Sender<T::Payload>,
    pub events: Sender<DataChannelEvent>,
}

pub trait SteckerDataChanelTrait {
    fn create_channels() -> Self;
    async fn connect(&self, data_channel: &Arc<RTCDataChannel>);
}

impl<T> SteckerDataChanelTrait for SteckerDataChannel<T>
where
    T: SteckerData,
{
    fn create_channels() -> Self {
        let capacity: usize = 512;

        let (inbound, _) = broadcast::channel::<T::Payload>(capacity);
        let (outbound, _) = broadcast::channel::<T::Payload>(capacity);
        let (events, _) = broadcast::channel::<DataChannelEvent>(4);

        SteckerDataChannel {
            inbound,
            outbound,
            events,
        }
    }

    /// wires up the data stecker tokio channels to the callbacks
    /// from the given RTCDataChannel
    #[instrument(skip_all)]
    async fn connect(&self, data_channel: &Arc<RTCDataChannel>) {
        let sender = self.events.clone();
        let mut outbound = self.outbound.subscribe();
        let channel = data_channel.clone();
        let span = Span::current();
        data_channel.on_open(Box::new(move|| {
            let mut receiver = sender.subscribe();
            let future = async move {
                trace!("New data channel opened");
                let _ = sender.send(DataChannelEvent::OpenedConnection);
                loop {
                    tokio::select! {
                        Ok(outbound_msg) = outbound.recv() => {
                            let _ = channel.send(&T::encode(outbound_msg).unwrap()).await;
                        },
                        Ok(DataChannelEvent::ClosedConnection) = receiver.recv() => {
                            trace!("Received closing trigger for sending out data channel messages");
                            break;
                        }
                        else => break,
                    }
                }
            };
            Box::pin(future.instrument(span))
        }));

        let sender = self.events.clone();
        data_channel.on_close(Box::new(move || {
            let _ = sender.send(DataChannelEvent::ClosedConnection);
            Box::pin(async {})
        }));

        let inbound = self.inbound.clone();
        data_channel.on_message(Box::new(move |message| {
            let value = T::decode(message).unwrap();
            let _ = inbound.send(value);
            Box::pin(async {})
        }));
    }
}

#[derive(Clone, Debug)]
pub struct SteckerAudioChannel {
    // channel which we use to receive a pushed audio channel
    pub audio_channel_rx: tokio::sync::watch::Receiver<Option<Arc<TrackLocalStaticRTP>>>,
    // channel which we use to push an audio channel to our consumers
    pub audio_channel_tx: tokio::sync::watch::Sender<Option<Arc<TrackLocalStaticRTP>>>,
    // sends a signal if the connection was closed by our peer
    pub close: Sender<()>,
    // drops the current source WebRTC connection so it can be replaced by a new one
    pub reset_sender: Sender<()>,
    // if we want to replace a running sender, we also need to continue the sequence_number
    // of the RTP packages
    pub sequence_number: watch::Sender<u16>,
}

impl SteckerAudioChannel {
    pub fn create_channels() -> Self {
        let (close, _) = broadcast::channel::<()>(1);
        let (audio_channel_tx, audio_channel_rx) = tokio::sync::watch::channel(None);
        let (reset_sender, _) = broadcast::channel::<()>(1);
        let (sequence_number, _) = watch::channel::<u16>(0);
        SteckerAudioChannel {
            audio_channel_tx,
            audio_channel_rx,
            close,
            reset_sender,
            sequence_number,
        }
    }
}

pub trait SteckerChannel: Send + Sync {
    fn close(&self);
}
