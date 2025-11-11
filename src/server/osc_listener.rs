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
use tracing::{debug, error, instrument, Instrument};

use crate::state::AppState;

#[instrument(skip(socket, state))]
pub async fn handle_osc_client(socket: TcpStream, addr: SocketAddr, state: Arc<AppState>) {
    let (reader, writer) = socket.into_split();
    let mut framed_reader = FramedRead::new(reader, OscDecoder);
    let mut framed_writer = FramedWrite::new(writer, OscEncoder);

    let (tx_outgoing_osc, mut rx_outgoing_osc) = mpsc::channel::<OscPacket>(16);
    let tx_outgoing_ping_osc = tx_outgoing_osc.clone();
    let mut room_events_rx = state.room_events.clone().subscribe();

    let (connection_closed_sender, _) = broadcast::channel::<()>(1);

    let reader_task =
        {
            let connection_closed = connection_closed_sender.clone();
            let mut service = OscService { client_addr: addr };

            tokio::spawn(async move {
            while let Some(result) = framed_reader.next().await {
                match result {
                    Ok(msg) => {
                        if let Some(response) = service.call(msg).await.unwrap() {
                            if tx_outgoing_osc.send(response).await.is_err() {
                                debug!("Writing task seems to been dropped - stop reading task");
                                break;
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
        if src.is_empty() {
            // No data left to decode: likely EOF
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
