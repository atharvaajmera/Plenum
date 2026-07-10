//! WebSocket signaling client: connects to the relay server, exchanges
//! `SignalMessage` JSON frames, and drives a single `RTCPeerConnection` through
//! offer/answer/ICE-candidate negotiation up to an open data channel.

use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::MediaEngine;
use webrtc::api::APIBuilder;
use webrtc::data_channel::data_channel_init::RTCDataChannelInit;
use webrtc::data_channel::RTCDataChannel;
use webrtc::ice_transport::ice_candidate::RTCIceCandidateInit;
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::RTCPeerConnection;

use crate::rtc::config::to_rtc_configuration;
use crate::rtc::error::RtcError;
use crate::signaling::{IceServer, SignalMessage};

/// Everything the transport needs once negotiation has produced an open data
/// channel: the peer connection (kept alive for the transport's lifetime, needed
/// for a clean `close()`), the data channel itself, and a receiver fed by the
/// data channel's `on_message` callback.
pub struct ConnectedChannel {
    pub peer_connection: Arc<RTCPeerConnection>,
    pub data_channel: Arc<RTCDataChannel>,
    pub inbound_rx: std::sync::mpsc::Receiver<Vec<u8>>,
}

fn build_api() -> Result<webrtc::api::API, RtcError> {
    let mut media_engine = MediaEngine::default();
    media_engine
        .register_default_codecs()
        .map_err(|error| RtcError::PeerConnection(error.to_string()))?;

    let mut registry = Registry::new();
    registry = register_default_interceptors(registry, &mut media_engine)
        .map_err(|error| RtcError::PeerConnection(error.to_string()))?;

    Ok(APIBuilder::new()
        .with_media_engine(media_engine)
        .with_interceptor_registry(registry)
        .build())
}

/// Wire up the handlers shared by both roles once a data channel (local or
/// remote) exists: `on_open` signals readiness, `on_message` forwards bytes into
/// the std sync channel that `RtcTransport::recv()` polls.
fn wire_data_channel(
    data_channel: &Arc<RTCDataChannel>,
    inbound_tx: std::sync::mpsc::Sender<Vec<u8>>,
    open_tx: mpsc::UnboundedSender<()>,
) {
    let open_tx = open_tx.clone();
    data_channel.on_open(Box::new(move || {
        let _ = open_tx.send(());
        Box::pin(async {})
    }));

    data_channel.on_message(Box::new(move |msg: webrtc::data_channel::data_channel_message::DataChannelMessage| {
        let _ = inbound_tx.send(msg.data.to_vec());
        Box::pin(async {})
    }));
}

async fn send_ws_message(
    ws_tx: &mut futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        Message,
    >,
    message: &SignalMessage,
) -> Result<(), RtcError> {
    let json = message
        .to_json()
        .map_err(|error| RtcError::Signaling(error.to_string()))?;
    ws_tx
        .send(Message::Text(json))
        .await
        .map_err(|error| RtcError::WebSocket(error.to_string()))
}

/// Register the outbound ICE-candidate drain: whenever the peer connection
/// gathers a local candidate, ship it over `outbound_tx` addressed to
/// `remote_peer_id_slot` (filled in once the remote peer id is known). `None`
/// signals end-of-candidates and is not forwarded (nothing to send).
fn wire_ice_candidate_outbound(
    peer_connection: &Arc<RTCPeerConnection>,
    outbound_tx: mpsc::UnboundedSender<SignalMessage>,
    session_id: String,
    my_peer_id: String,
    remote_peer_id: Arc<std::sync::Mutex<Option<String>>>,
) {
    peer_connection.on_ice_candidate(Box::new(move |candidate| {
        let outbound_tx = outbound_tx.clone();
        let session_id = session_id.clone();
        let my_peer_id = my_peer_id.clone();
        let remote_peer_id = Arc::clone(&remote_peer_id);
        Box::pin(async move {
            let Some(candidate) = candidate else {
                return;
            };
            let Some(to_peer_id) = remote_peer_id.lock().unwrap().clone() else {
                // Remote peer id not known yet; drop the candidate. In practice
                // candidates only start gathering after set_local_description,
                // which (for both roles here) happens after the remote peer id
                // is already known.
                return;
            };
            let Ok(init) = candidate.to_json() else {
                return;
            };
            let message = SignalMessage::IceCandidate {
                session_id,
                from_peer_id: my_peer_id,
                to_peer_id,
                candidate: init.candidate,
                sdp_mid: init.sdp_mid,
                sdp_mline_index: init.sdp_mline_index,
            };
            let _ = outbound_tx.send(message);
        })
    }));
}

/// Connect to the relay as the **offerer** (sender role): join the session, wait
/// for `PeerJoined` (either a genuine new peer or the relay's synthesized
/// already-present-peer notification) to learn the answerer's peer id, create the
/// data channel + offer, negotiate, and wait for the data channel to open.
pub async fn run_offerer(
    relay_url: &str,
    session_id: &str,
    my_peer_id: &str,
    ice_servers: Vec<IceServer>,
) -> Result<ConnectedChannel, RtcError> {
    let (ws_stream, _response) = tokio_tungstenite::connect_async(relay_url)
        .await
        .map_err(|error| RtcError::WebSocket(error.to_string()))?;
    let (mut ws_tx, mut ws_rx) = ws_stream.split();

    send_ws_message(
        &mut ws_tx,
        &SignalMessage::JoinSession {
            peer_id: my_peer_id.to_string(),
            session_id: session_id.to_string(),
        },
    )
    .await?;

    let api = build_api()?;
    let configuration: RTCConfiguration = to_rtc_configuration(&ice_servers);
    let peer_connection = Arc::new(
        api.new_peer_connection(configuration)
            .await
            .map_err(|error| RtcError::PeerConnection(error.to_string()))?,
    );

    let (inbound_tx, inbound_rx) = std::sync::mpsc::channel::<Vec<u8>>();
    let (open_tx, mut open_rx) = mpsc::unbounded_channel::<()>();
    let (outbound_tx, mut outbound_rx) = mpsc::unbounded_channel::<SignalMessage>();
    let remote_peer_id: Arc<std::sync::Mutex<Option<String>>> = Arc::new(std::sync::Mutex::new(None));

    wire_ice_candidate_outbound(
        &peer_connection,
        outbound_tx.clone(),
        session_id.to_string(),
        my_peer_id.to_string(),
        Arc::clone(&remote_peer_id),
    );

    // Spawn the outbound WS drain task: forwards locally-generated SignalMessages
    // (currently just ICE candidates; offer/answer are sent inline below) to the
    // relay as JSON text frames.
    let outbound_task = tokio::spawn(async move {
        while let Some(message) = outbound_rx.recv().await {
            if send_ws_message(&mut ws_tx, &message).await.is_err() {
                break;
            }
        }
    });

    let mut data_channel_created = false;
    let mut pending_data_channel: Option<Arc<RTCDataChannel>> = None;

    loop {
        tokio::select! {
            _ = open_rx.recv() => {
                break;
            }
            frame = ws_rx.next() => {
                let Some(frame) = frame else {
                    return Err(RtcError::WebSocket("relay connection closed before negotiation completed".into()));
                };
                let frame = frame.map_err(|error| RtcError::WebSocket(error.to_string()))?;
                let Message::Text(text) = frame else {
                    continue;
                };
                let signal = SignalMessage::from_json(&text)
                    .map_err(|error| RtcError::Signaling(error.to_string()))?;

                match signal {
                    SignalMessage::PeerJoined { peer_id, .. } if !data_channel_created => {
                        *remote_peer_id.lock().unwrap() = Some(peer_id.clone());
                        data_channel_created = true;

                        let dc_init = RTCDataChannelInit {
                            ordered: Some(true),
                            ..Default::default()
                        };
                        let data_channel = peer_connection
                            .create_data_channel("plenum", Some(dc_init))
                            .await
                            .map_err(|error| RtcError::PeerConnection(error.to_string()))?;
                        wire_data_channel(&data_channel, inbound_tx.clone(), open_tx.clone());

                        let offer = peer_connection
                            .create_offer(None)
                            .await
                            .map_err(|error| RtcError::PeerConnection(error.to_string()))?;
                        let sdp = offer.sdp.clone();
                        peer_connection
                            .set_local_description(offer)
                            .await
                            .map_err(|error| RtcError::PeerConnection(error.to_string()))?;

                        let _ = outbound_tx.send(SignalMessage::Offer {
                            session_id: session_id.to_string(),
                            from_peer_id: my_peer_id.to_string(),
                            to_peer_id: peer_id,
                            sdp,
                            nat: None,
                        });

                        // Stash the data channel handle for the caller once open.
                        pending_data_channel = Some(data_channel);
                    }
                    SignalMessage::PeerJoined { .. } => {
                        // Already negotiating/negotiated; ignore further notifications.
                    }
                    SignalMessage::Answer { sdp, .. } => {
                        let answer = RTCSessionDescription::answer(sdp)
                            .map_err(|error| RtcError::PeerConnection(error.to_string()))?;
                        peer_connection
                            .set_remote_description(answer)
                            .await
                            .map_err(|error| RtcError::PeerConnection(error.to_string()))?;
                    }
                    SignalMessage::IceCandidate { candidate, sdp_mid, sdp_mline_index, .. } => {
                        let init = RTCIceCandidateInit {
                            candidate,
                            sdp_mid,
                            sdp_mline_index,
                            ..Default::default()
                        };
                        peer_connection
                            .add_ice_candidate(init)
                            .await
                            .map_err(|error| RtcError::PeerConnection(error.to_string()))?;
                    }
                    SignalMessage::Error { message } => {
                        return Err(RtcError::Signaling(message));
                    }
                    _ => {}
                }
            }
        }
    }

    outbound_task.abort();

    let data_channel = pending_data_channel
        .ok_or_else(|| RtcError::PeerConnection("data channel was never created".into()))?;

    Ok(ConnectedChannel {
        peer_connection,
        data_channel,
        inbound_rx,
    })
}

/// Connect to the relay as the **answerer** (receiver role): join the session,
/// register `on_data_channel` before any remote description is set (so it fires
/// reliably when the offerer's channel arrives), wait for an `Offer`, answer it,
/// and wait for the resulting data channel to open.
pub async fn run_answerer(
    relay_url: &str,
    session_id: &str,
    my_peer_id: &str,
    ice_servers: Vec<IceServer>,
) -> Result<ConnectedChannel, RtcError> {
    let (ws_stream, _response) = tokio_tungstenite::connect_async(relay_url)
        .await
        .map_err(|error| RtcError::WebSocket(error.to_string()))?;
    let (mut ws_tx, mut ws_rx) = ws_stream.split();

    send_ws_message(
        &mut ws_tx,
        &SignalMessage::JoinSession {
            peer_id: my_peer_id.to_string(),
            session_id: session_id.to_string(),
        },
    )
    .await?;

    let api = build_api()?;
    // Default configuration for now; if the incoming Offer carries a `nat` payload
    // with ICE servers, we rebuild the peer connection's effective config isn't
    // possible post-construction for ice_servers, so we honor the caller-supplied
    // `ice_servers` here and layer in offer-provided ones (if any) at that point is
    // not supported by webrtc-rs (ice_servers is fixed at construction). We
    // therefore construct using the caller-supplied ice_servers; the offer's `nat`
    // is used only if the peer connection has not yet been created (see below).
    let configuration: RTCConfiguration = to_rtc_configuration(&ice_servers);
    let peer_connection = Arc::new(
        api.new_peer_connection(configuration)
            .await
            .map_err(|error| RtcError::PeerConnection(error.to_string()))?,
    );

    let (inbound_tx, inbound_rx) = std::sync::mpsc::channel::<Vec<u8>>();
    let (open_tx, mut open_rx) = mpsc::unbounded_channel::<()>();
    let (outbound_tx, mut outbound_rx) = mpsc::unbounded_channel::<SignalMessage>();
    let remote_peer_id: Arc<std::sync::Mutex<Option<String>>> = Arc::new(std::sync::Mutex::new(None));

    wire_ice_candidate_outbound(
        &peer_connection,
        outbound_tx.clone(),
        session_id.to_string(),
        my_peer_id.to_string(),
        Arc::clone(&remote_peer_id),
    );

    // Register on_data_channel BEFORE set_remote_description, so it reliably
    // fires when the offerer's channel arrives during negotiation.
    let inbound_tx_for_channel = inbound_tx.clone();
    let open_tx_for_channel = open_tx.clone();
    let pending_data_channel: Arc<std::sync::Mutex<Option<Arc<RTCDataChannel>>>> =
        Arc::new(std::sync::Mutex::new(None));
    let pending_data_channel_setter = Arc::clone(&pending_data_channel);
    peer_connection.on_data_channel(Box::new(move |data_channel: Arc<RTCDataChannel>| {
        wire_data_channel(&data_channel, inbound_tx_for_channel.clone(), open_tx_for_channel.clone());
        *pending_data_channel_setter.lock().unwrap() = Some(data_channel);
        Box::pin(async {})
    }));

    let outbound_task = tokio::spawn(async move {
        while let Some(message) = outbound_rx.recv().await {
            if send_ws_message(&mut ws_tx, &message).await.is_err() {
                break;
            }
        }
    });

    let mut answered = false;

    loop {
        tokio::select! {
            _ = open_rx.recv() => {
                break;
            }
            frame = ws_rx.next() => {
                let Some(frame) = frame else {
                    return Err(RtcError::WebSocket("relay connection closed before negotiation completed".into()));
                };
                let frame = frame.map_err(|error| RtcError::WebSocket(error.to_string()))?;
                let Message::Text(text) = frame else {
                    continue;
                };
                let signal = SignalMessage::from_json(&text)
                    .map_err(|error| RtcError::Signaling(error.to_string()))?;

                match signal {
                    SignalMessage::Offer { from_peer_id, sdp, .. } if !answered => {
                        *remote_peer_id.lock().unwrap() = Some(from_peer_id.clone());
                        answered = true;

                        let offer = RTCSessionDescription::offer(sdp)
                            .map_err(|error| RtcError::PeerConnection(error.to_string()))?;
                        peer_connection
                            .set_remote_description(offer)
                            .await
                            .map_err(|error| RtcError::PeerConnection(error.to_string()))?;

                        let answer = peer_connection
                            .create_answer(None)
                            .await
                            .map_err(|error| RtcError::PeerConnection(error.to_string()))?;
                        let answer_sdp = answer.sdp.clone();
                        peer_connection
                            .set_local_description(answer)
                            .await
                            .map_err(|error| RtcError::PeerConnection(error.to_string()))?;

                        let _ = outbound_tx.send(SignalMessage::Answer {
                            session_id: session_id.to_string(),
                            from_peer_id: my_peer_id.to_string(),
                            to_peer_id: from_peer_id,
                            sdp: answer_sdp,
                        });
                    }
                    SignalMessage::Offer { .. } => {
                        // Already negotiating/negotiated; ignore further offers.
                    }
                    SignalMessage::IceCandidate { candidate, sdp_mid, sdp_mline_index, .. } => {
                        let init = RTCIceCandidateInit {
                            candidate,
                            sdp_mid,
                            sdp_mline_index,
                            ..Default::default()
                        };
                        peer_connection
                            .add_ice_candidate(init)
                            .await
                            .map_err(|error| RtcError::PeerConnection(error.to_string()))?;
                    }
                    SignalMessage::Error { message } => {
                        return Err(RtcError::Signaling(message));
                    }
                    _ => {}
                }
            }
        }
    }

    outbound_task.abort();

    let data_channel = pending_data_channel
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| RtcError::PeerConnection("data channel was never received".into()))?;

    Ok(ConnectedChannel {
        peer_connection,
        data_channel,
        inbound_rx,
    })
}
