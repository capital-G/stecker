use std::sync::Arc;
use std::{io, net::SocketAddr, time::Duration};

use axum::async_trait;
use bytes::Buf;
use bytes::BytesMut;
use futures::{SinkExt, StreamExt};
use rosc::{decoder::decode_tcp, encoder::encode_tcp, OscMessage, OscPacket};
use tokio::{
    net::TcpStream,
    sync::{broadcast, mpsc},
};
use tokio_util::codec::{Decoder, Encoder, FramedRead, FramedWrite};
use tower::Service;
use tracing::{debug, error, instrument, trace, Instrument};

use crate::models::{DispatcherType, RoomDispatcherInput, RoomType};
use crate::state::AppState;

impl TryFrom<OscMessage> for RoomDispatcherInput {
    type Error = ();

    #[instrument(skip_all)]
    fn try_from(message: OscMessage) -> Result<RoomDispatcherInput, Self::Error> {
        match message.args.len() {
            5 => Ok(RoomDispatcherInput {
                name: message.args[0].clone().string().ok_or(())?,
                admin_password: message.args[1].clone().string(),
                rule: message.args[2].clone().string().ok_or(())?,
                room_type: RoomType::Audio,
                dispatcher_type: DispatcherType::Random,
                timeout: message.args[3].clone().int().ok_or(())?,
                return_room_prefix: message.args[4].clone().string(),
            }),
            _ => {
                trace!("Invaild length of room dispatch OSC message");
                Err(())
            }
        }
    }
}

struct OscProcessor {
    state: Arc<AppState>,
}

impl OscProcessor {
    #[instrument(skip_all)]
    pub async fn process(&self, osc_packet: OscPacket) -> Option<OscPacket> {
        match osc_packet {
            OscPacket::Message(osc_message) => match osc_message.addr.as_str() {
                "/createDispatcher" => match RoomDispatcherInput::try_from(osc_message) {
                    Ok(dispatcher_input) => {
                        match self.state.create_dispatcher(dispatcher_input).await {
                            Ok(_) => Some(Self::send_reply("Created dispatcher")),
                            Err(_) => Some(Self::send_error("Error at creating dispatcher")),
                        }
                    }
                    Err(_) => Some(Self::send_error("Invalid create dispatcher message")),
                },
                _ => None,
            },
            OscPacket::Bundle(_) => None,
        }
    }

    fn send_reply(message: &str) -> OscPacket {
        OscPacket::Message(OscMessage {
            addr: "/reply".to_string(),
            args: vec![rosc::OscType::String(message.to_string())],
        })
    }

    pub fn send_error(message: &str) -> OscPacket {
        OscPacket::Message(OscMessage {
            addr: "/error".to_string(),
            args: vec![rosc::OscType::String(message.to_string())],
        })
    }
}

#[instrument(skip(socket, state))]
pub async fn handle_osc_client(socket: TcpStream, addr: SocketAddr, state: Arc<AppState>) {
    let (reader, writer) = socket.into_split();
    let mut framed_reader = FramedRead::new(reader, OscDecoder);
    let mut framed_writer = FramedWrite::new(writer, OscEncoder);

    let (tx_outgoing_osc, mut rx_outgoing_osc) = mpsc::channel::<OscPacket>(16);
    let tx_outgoing_ping_osc = tx_outgoing_osc.clone();
    let mut room_events_rx = state.room_events.clone().subscribe();

    let (connection_closed_sender, _) = broadcast::channel::<()>(1);

    let osc_processor = OscProcessor { state };

    let reader_task = {
        let connection_closed = connection_closed_sender.clone();
        let mut service = OscService { client_addr: addr };

        tokio::spawn(async move {
            while let Some(result) = framed_reader.next().await {
                match result {
                    Ok(msg) => {
                        if let Some(osc_packet) = service.call(msg).await.unwrap() {
                            trace!("Received OSC packet");
                            match osc_processor.process(osc_packet).await {
                                Some(reply) => {
                                    if tx_outgoing_osc.send(reply).await.is_err() {
                                        debug!("Writing task seems to been dropped - stop reading task");
                                        break;
                                    }
                                },
                                None => {},
                            }
                        }
                    },
                    Err(e) => {
                        error!("Failed to read OSC packet: {:?}", e);
                        break;
                    }
                }
            }
            let _ = connection_closed.send(());
            debug!("Reader task finished");
        }.in_current_span())
    };

    let writer_task = {
        let mut connection_closed_receiver = connection_closed_sender.subscribe();

        tokio::spawn(
            async move {
                loop {
                    tokio::select! {
                        _ = connection_closed_receiver.recv() => {
                            debug!("Connection closed signal received in writer task");
                            break;
                        }
                        Some(osc_packet) = rx_outgoing_osc.recv() => {
                            if let Err(e) = framed_writer.send(osc_packet).await {
                                error!("Error sending OSC packet: {:?}", e);
                                break;
                            }
                        }
                    }
                }
                debug!("Writer task finished");
            }
            .in_current_span(),
        )
    };

    let ping_task = {
        let mut connection_closed_receiver = connection_closed_sender.subscribe();

        tokio::spawn(async move {
            loop {
                tokio::select! {

                    _ = connection_closed_receiver.recv() => {
                        debug!("Connection closed signal received in ping task");
                        break;
                    },
                    Ok(room_event) = room_events_rx.recv() => {
                        if tx_outgoing_ping_osc.send(room_event.into_osc_packet().await).await.is_err() {
                            break;
                        }
                    },
                    _ = tokio::time::sleep(Duration::from_secs(10)) => {
                        let message = OscPacket::Message(OscMessage { addr: "/ping".to_string(), args: vec![] });
                        if tx_outgoing_ping_osc.send(message).await.is_err() {
                            debug!("Writer task stopped, stop pinging");
                            break;
                        }
                    }
                }
            }
        }.in_current_span())
    };

    // stop and terminate if any of these tasks fail
    tokio::select! {
        _ = reader_task => {},
        _ = writer_task => {},
        _ = ping_task => {},
    };
    debug!("Connection closed");
}

struct OscDecoder;

impl Decoder for OscDecoder {
    type Item = OscPacket;
    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        // OSC TCP needs to start w/ 4 big endian bytes indicating its length
        if src.len() < 4 {
            return Ok(None);
        }

        let mut length_buf = &src[..4];
        let len = length_buf.get_u32() as usize;

        // waiting for full frame
        if src.len() < 4 + len {
            return Ok(None);
        }

        match decode_tcp(src) {
            Ok((remaining, Some(packet))) => {
                let consumed = src.len() - remaining.len();
                src.advance(consumed);
                Ok(Some(packet))
            }
            Ok((_remaining, None)) => {
                // Not enough data to decode yet
                Ok(None)
            }
            Err(e) => {
                error!("OSC receiving error: {:?}", e);
                Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Failed to decode OSC packet",
                ))
            }
        }
    }
}

struct OscEncoder;

impl Encoder<OscPacket> for OscEncoder {
    type Error = std::io::Error;

    fn encode(&mut self, item: OscPacket, dst: &mut BytesMut) -> Result<(), Self::Error> {
        // todo use anyhow
        let buf = encode_tcp(&item).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "Failed to encode")
        })?;
        dst.extend_from_slice(&buf);
        Ok(())
    }
}

struct OscService {
    client_addr: SocketAddr,
}

#[async_trait]
impl Service<rosc::OscPacket> for OscService {
    type Response = Option<rosc::OscPacket>;
    type Error = anyhow::Error;
    type Future = futures::future::Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(
        &mut self,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn call(&mut self, packet: rosc::OscPacket) -> Self::Future {
        futures::future::ok(Some(packet))
    }
}
