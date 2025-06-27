use std::sync::Arc;

use webrtc::{
    data_channel::RTCDataChannel, peer_connection::peer_connection_state::RTCPeerConnectionState,
    track::track_remote::TrackRemote,
};

#[derive(Clone)]
pub enum SteckerConnectionEvent {
    ConnectionStateChanged(RTCPeerConnectionState),
    ConnectionClosed,
    NewDataChannel(Arc<RTCDataChannel>),
    NewAudioTrack(Arc<TrackRemote>),
}
