//! WebSocket signaling client: connects to the relay server, exchanges
//! `SignalMessage` JSON frames, and drives a single `RTCPeerConnection` through
//! offer/answer/ICE-candidate negotiation up to an open data channel.

use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::MediaEngine;
use webrtc::api::APIBuilder;
use webrtc::data_channel::data_channel_init::RTCDataChannelInit;
use webrtc::data_channel::data_channel_state::RTCDataChannelState;
use webrtc::data_channel::RTCDataChannel;
use webrtc::ice_transport::ice_candidate::RTCIceCandidateInit;
use webrtc::ice_transport::ice_connection_state::RTCIceConnectionState;
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::stats::StatsReportType;

use crate::rtc::config::to_rtc_configuration;
use crate::rtc::error::RtcError;
use crate::signaling::{IceServer, SignalMessage};

/// How long to keep the signaling WebSocket alive after the local data channel
/// opens. The local side opening does NOT mean the remote side has finished
/// ICE: trickle candidates can still be in flight in both directions, and
/// tearing the socket down here loses them — leaving the remote peer
/// permanently half-open (it never completes a usable pair, never opens its
/// data channel, and the transfer sits at 0%).
const SIGNALING_LINGER: Duration = Duration::from_secs(15);

/// Everything the transport needs once negotiation has produced an open data
/// channel: the peer connection (kept alive for the transport's lifetime, needed
/// for a clean `close()`), the data channel itself, a receiver fed by the data
/// channel's `on_message` callback, and a diagnostics channel pair (`diag_tx`
/// still usable by the caller for its own post-connect logging, e.g. data
/// channel send failures; `diag_rx` drained by `RtcTransport::poll_diagnostics`).
pub struct ConnectedChannel {
    pub peer_connection: Arc<RTCPeerConnection>,
    pub data_channel: Arc<RTCDataChannel>,
    pub inbound_rx: std::sync::mpsc::Receiver<Vec<u8>>,
    pub diag_tx: std::sync::mpsc::Sender<String>,
    pub diag_rx: std::sync::mpsc::Receiver<String>,
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

/// Flush ICE candidates that were received and buffered before the remote
/// description was set, applying them now that add_ice_candidate will accept
/// them. Drains the buffer.
async fn flush_pending_candidates(
    peer_connection: &Arc<RTCPeerConnection>,
    pending: &mut Vec<RTCIceCandidateInit>,
) -> Result<(), RtcError> {
    for init in pending.drain(..) {
        peer_connection
            .add_ice_candidate(init)
            .await
            .map_err(|error| RtcError::PeerConnection(error.to_string()))?;
    }
    Ok(())
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

/// Spawn the outbound WS drain task: forwards locally-generated
/// `SignalMessage`s (ICE candidates; offer/answer are queued the same way) to
/// the relay as JSON text frames. Also listens on `ws_close_rx` so the socket
/// can be shut down with a proper close handshake once signaling is finished —
/// the receiver also resolves if the sender is dropped on an error path, so
/// the socket is closed cleanly in every case.
fn spawn_outbound_task(
    mut ws_tx: futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        Message,
    >,
    mut outbound_rx: mpsc::UnboundedReceiver<SignalMessage>,
    mut ws_close_rx: oneshot::Receiver<()>,
) {
    tokio::spawn(async move {
        loop {
            tokio::select! {
                maybe_message = outbound_rx.recv() => {
                    match maybe_message {
                        Some(message) => {
                            if send_ws_message(&mut ws_tx, &message).await.is_err() {
                                break;
                            }
                        }
                        None => break,
                    }
                }
                _ = &mut ws_close_rx => {
                    let _ = ws_tx.send(Message::Close(None)).await;
                    break;
                }
            }
        }
    });
}

/// Keep the signaling socket alive for a grace period after the local data
/// channel opens: keep reading inbound frames and applying late remote trickle
/// ICE candidates (outbound candidates keep flowing through the outbound task
/// the whole time), then signal the outbound task to close the socket with a
/// proper close handshake. See `SIGNALING_LINGER` for why this matters.
fn spawn_signaling_linger(
    mut ws_rx: futures_util::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    >,
    peer_connection: Arc<RTCPeerConnection>,
    diag_tx: std::sync::mpsc::Sender<String>,
    role: &'static str,
    ws_close_tx: oneshot::Sender<()>,
) {
    tokio::spawn(async move {
        let _ = diag_tx.send(format!(
            "DIAG {role}: data channel open; keeping signaling alive {}s for late ICE",
            SIGNALING_LINGER.as_secs()
        ));
        let deadline = tokio::time::Instant::now() + SIGNALING_LINGER;
        loop {
            tokio::select! {
                _ = tokio::time::sleep_until(deadline) => break,
                frame = ws_rx.next() => {
                    match frame {
                        Some(Ok(Message::Text(text))) => {
                            let Ok(signal) = SignalMessage::from_json(&text) else {
                                continue;
                            };
                            if let SignalMessage::IceCandidate {
                                candidate,
                                sdp_mid,
                                sdp_mline_index,
                                ..
                            } = signal
                            {
                                let init = RTCIceCandidateInit {
                                    candidate,
                                    sdp_mid,
                                    sdp_mline_index,
                                    ..Default::default()
                                };
                                if let Err(error) = peer_connection.add_ice_candidate(init).await {
                                    let _ = diag_tx.send(format!(
                                        "DIAG {role}: linger add_ice_candidate failed: {error}"
                                    ));
                                }
                            }
                        }
                        Some(Ok(_)) => {}
                        Some(Err(_)) | None => break,
                    }
                }
            }
        }
        let _ = ws_close_tx.send(());
    });
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

/// TEMP DIAG: log ICE and peer-connection state transitions through `diag_tx`
/// (drained by `RtcTransport::poll_diagnostics` and forwarded to the app's
/// event sink) so they're visible on-device via `adb logcat`, not just
/// desktop stderr.
fn wire_state_logging(
    peer_connection: &Arc<RTCPeerConnection>,
    role: &'static str,
    diag_tx: std::sync::mpsc::Sender<String>,
) {
    let ice_diag_tx = diag_tx.clone();
    peer_connection.on_ice_connection_state_change(Box::new(move |state: RTCIceConnectionState| {
        let _ = ice_diag_tx.send(format!("DIAG {role}: ice_connection_state -> {state:?}"));
        Box::pin(async {})
    }));
    peer_connection.on_peer_connection_state_change(Box::new(
        move |state: RTCPeerConnectionState| {
            let _ = diag_tx.send(format!("DIAG {role}: peer_connection_state -> {state:?}"));
            Box::pin(async {})
        },
    ));
}

/// TEMP DIAG: periodically poll `get_stats()` for the *nominated* (i.e.
/// actually selected, not just offered) ICE candidate pair, resolve its
/// local/remote candidate types (host/srflx/relay) and addresses, and report
/// byte counters + RTT. This is the only way to know whether a transfer is
/// genuinely peer-to-peer or relayed, and whether bytes are moving at the
/// network layer at all -- signaling logs only show candidates that were
/// *offered*, never which pair actually won.
fn spawn_stats_poller(
    peer_connection: Arc<RTCPeerConnection>,
    diag_tx: std::sync::mpsc::Sender<String>,
    role: &'static str,
) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        loop {
            interval.tick().await;

            let report = peer_connection.get_stats().await;
            let nominated = report.reports.values().find_map(|entry| match entry {
                StatsReportType::CandidatePair(pair) if pair.nominated => Some(pair),
                _ => None,
            });

            let Some(pair) = nominated else {
                let _ = diag_tx.send(format!("DIAG {role}: stats -> no nominated candidate pair yet"));
                continue;
            };

            let describe_candidate = |id: &str| -> String {
                report
                    .reports
                    .values()
                    .find_map(|entry| match entry {
                        StatsReportType::LocalCandidate(candidate) if candidate.id == id => {
                            Some(format!("{:?}({})", candidate.candidate_type, candidate.ip))
                        }
                        StatsReportType::RemoteCandidate(candidate) if candidate.id == id => {
                            Some(format!("{:?}({})", candidate.candidate_type, candidate.ip))
                        }
                        _ => None,
                    })
                    .unwrap_or_else(|| "unknown".to_string())
            };

            let local = describe_candidate(&pair.local_candidate_id);
            let remote = describe_candidate(&pair.remote_candidate_id);

            let message = format!(
                "DIAG {role}: stats -> pair={local}<->{remote} state={:?} bytes_sent={} bytes_received={} rtt={:.3}s",
                pair.state, pair.bytes_sent, pair.bytes_received, pair.current_round_trip_time
            );

            if diag_tx.send(message).is_err() {
                // Receiver dropped (RtcTransport closed/dropped) -- stop polling.
                break;
            }
        }
    });
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
    let (diag_tx, diag_rx) = std::sync::mpsc::channel::<String>();
    let (open_tx, mut open_rx) = mpsc::unbounded_channel::<()>();
    let (outbound_tx, outbound_rx) = mpsc::unbounded_channel::<SignalMessage>();
    let remote_peer_id: Arc<std::sync::Mutex<Option<String>>> = Arc::new(std::sync::Mutex::new(None));

    wire_ice_candidate_outbound(
        &peer_connection,
        outbound_tx.clone(),
        session_id.to_string(),
        my_peer_id.to_string(),
        Arc::clone(&remote_peer_id),
    );
    wire_state_logging(&peer_connection, "offerer", diag_tx.clone());

    // Spawn the outbound WS drain task: forwards locally-generated SignalMessages
    // (currently just ICE candidates; offer/answer are sent inline below) to the
    // relay as JSON text frames.
    let (ws_close_tx, ws_close_rx) = oneshot::channel::<()>();
    spawn_outbound_task(ws_tx, outbound_rx, ws_close_rx);

    let mut data_channel_created = false;
    let mut pending_data_channel: Option<Arc<RTCDataChannel>> = None;
    // Remote ICE candidates can arrive before the answer sets our remote
    // description (candidates gather the moment we set_local_description, so the
    // peer may trickle some before its Answer reaches us). webrtc-rs rejects
    // add_ice_candidate with ErrNoRemoteDescription in that window, so buffer
    // early candidates and flush them once the remote description is set.
    let mut remote_description_set = false;
    let mut pending_remote_candidates: Vec<RTCIceCandidateInit> = Vec::new();

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
                        remote_description_set = true;
                        flush_pending_candidates(&peer_connection, &mut pending_remote_candidates).await?;
                    }
                    SignalMessage::IceCandidate { candidate, sdp_mid, sdp_mline_index, .. } => {
                        let init = RTCIceCandidateInit {
                            candidate,
                            sdp_mid,
                            sdp_mline_index,
                            ..Default::default()
                        };
                        if remote_description_set {
                            peer_connection
                                .add_ice_candidate(init)
                                .await
                                .map_err(|error| RtcError::PeerConnection(error.to_string()))?;
                        } else {
                            pending_remote_candidates.push(init);
                        }
                    }
                    SignalMessage::Error { message } => {
                        return Err(RtcError::Signaling(message));
                    }
                    _ => {}
                }
            }
        }
    }

    // Local data channel is open, but the remote peer may still be finishing
    // ICE — keep signaling alive so late trickle candidates aren't lost.
    spawn_signaling_linger(
        ws_rx,
        Arc::clone(&peer_connection),
        diag_tx.clone(),
        "offerer",
        ws_close_tx,
    );

    let data_channel = pending_data_channel
        .ok_or_else(|| RtcError::PeerConnection("data channel was never created".into()))?;

    spawn_stats_poller(Arc::clone(&peer_connection), diag_tx.clone(), "offerer");

    Ok(ConnectedChannel {
        peer_connection,
        data_channel,
        inbound_rx,
        diag_tx,
        diag_rx,
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
    let (diag_tx, diag_rx) = std::sync::mpsc::channel::<String>();
    let (open_tx, mut open_rx) = mpsc::unbounded_channel::<()>();
    let (outbound_tx, outbound_rx) = mpsc::unbounded_channel::<SignalMessage>();
    let remote_peer_id: Arc<std::sync::Mutex<Option<String>>> = Arc::new(std::sync::Mutex::new(None));

    wire_ice_candidate_outbound(
        &peer_connection,
        outbound_tx.clone(),
        session_id.to_string(),
        my_peer_id.to_string(),
        Arc::clone(&remote_peer_id),
    );
    wire_state_logging(&peer_connection, "answerer", diag_tx.clone());

    // Register on_data_channel BEFORE set_remote_description, so it reliably
    // fires when the offerer's channel arrives during negotiation.
    let inbound_tx_for_channel = inbound_tx.clone();
    let open_tx_for_channel = open_tx.clone();
    let pending_data_channel: Arc<std::sync::Mutex<Option<Arc<RTCDataChannel>>>> =
        Arc::new(std::sync::Mutex::new(None));
    let pending_data_channel_setter = Arc::clone(&pending_data_channel);
    peer_connection.on_data_channel(Box::new(move |data_channel: Arc<RTCDataChannel>| {
        wire_data_channel(&data_channel, inbound_tx_for_channel.clone(), open_tx_for_channel.clone());
        // Race: the channel may already be Open by the time we register
        // `on_open` (it fired before registration and will never fire again).
        // Check the state directly and signal readiness ourselves — otherwise
        // the answerer waits forever while the offerer happily transmits.
        if data_channel.ready_state() == RTCDataChannelState::Open {
            let _ = open_tx_for_channel.send(());
        }
        *pending_data_channel_setter.lock().unwrap() = Some(data_channel);
        Box::pin(async {})
    }));

    let (ws_close_tx, ws_close_rx) = oneshot::channel::<()>();
    spawn_outbound_task(ws_tx, outbound_rx, ws_close_rx);

    let mut answered = false;
    // See the offerer's comment: buffer remote ICE candidates that arrive before
    // we've set the remote description (the offer), then flush once it's set.
    let mut remote_description_set = false;
    let mut pending_remote_candidates: Vec<RTCIceCandidateInit> = Vec::new();
    // Fallback for the on_open registration race (see on_data_channel above):
    // poll the pending channel's state so a missed open callback can never
    // wedge the answerer.
    let mut open_poll = tokio::time::interval(Duration::from_millis(200));

    loop {
        tokio::select! {
            _ = open_rx.recv() => {
                break;
            }
            _ = open_poll.tick() => {
                let ready = pending_data_channel
                    .lock()
                    .unwrap()
                    .as_ref()
                    .map(|data_channel| data_channel.ready_state());
                if ready == Some(RTCDataChannelState::Open) {
                    break;
                }
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
                        remote_description_set = true;
                        flush_pending_candidates(&peer_connection, &mut pending_remote_candidates).await?;

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
                        if remote_description_set {
                            peer_connection
                                .add_ice_candidate(init)
                                .await
                                .map_err(|error| RtcError::PeerConnection(error.to_string()))?;
                        } else {
                            pending_remote_candidates.push(init);
                        }
                    }
                    SignalMessage::Error { message } => {
                        return Err(RtcError::Signaling(message));
                    }
                    _ => {}
                }
            }
        }
    }

    // Local data channel is open, but the remote peer may still be finishing
    // ICE — keep signaling alive so late trickle candidates aren't lost.
    spawn_signaling_linger(
        ws_rx,
        Arc::clone(&peer_connection),
        diag_tx.clone(),
        "answerer",
        ws_close_tx,
    );

    let data_channel = pending_data_channel
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| RtcError::PeerConnection("data channel was never received".into()))?;

    spawn_stats_poller(Arc::clone(&peer_connection), diag_tx.clone(), "answerer");

    Ok(ConnectedChannel {
        peer_connection,
        data_channel,
        inbound_rx,
        diag_tx,
        diag_rx,
    })
}
