//! `RtcTransport`: a `crate::transport::Transport` implementation backed by a
//! WebRTC data channel, negotiated over a relay/signaling WebSocket.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc as std_mpsc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use tokio::sync::{mpsc, oneshot};

use crate::rtc::error::RtcError;
use crate::rtc::runtime::BackgroundRuntime;
use crate::rtc::signaling_client::{run_answerer, run_offerer, ConnectedChannel};
use crate::signaling::IceServer;
use crate::transport::{Transport, TransportError, TransportResult};

/// Max packets queued between `Transport::send` and the background send loop.
/// Bounded so a retransmission storm can't pile megabytes of stale duplicates
/// into the queue (which then all have to drain over SCTP before `close()`
/// returns — the "stuck at 100%" failure). When full, `send` blocks the engine
/// thread briefly, which is exactly the backpressure the window loop needs.
const OUTBOUND_QUEUE_PACKETS: usize = 64;

/// SCTP buffered-amount watermarks. The background loop stops handing packets
/// to the data channel once `buffered_amount` exceeds the high watermark and
/// resumes below the low watermark, keeping end-to-end queueing (and thus the
/// window loop's view of "sent") close to what's actually on the wire.
const BUFFERED_HIGH_WATERMARK: usize = 1024 * 1024;
const BUFFERED_LOW_WATERMARK: usize = 256 * 1024;

/// A `Transport` implementation over a WebRTC data channel.
///
/// Owns one dedicated OS thread running a `current_thread` tokio runtime, which
/// holds the `Arc<RTCPeerConnection>` and `Arc<RTCDataChannel>` for the lifetime
/// of this transport. `send`/`recv`/`close` bridge to that thread via channels,
/// matching `TcpTransport`'s non-blocking polling contract.
pub struct RtcTransport {
    inbound_rx: std_mpsc::Receiver<Vec<u8>>,
    outbound_tx: mpsc::Sender<Vec<u8>>,
    shutdown_tx: Option<oneshot::Sender<()>>,
    diag_rx: std_mpsc::Receiver<String>,
    runtime: Option<BackgroundRuntime>,
    closed: bool,
}

/// Outcome of the background thread's connect-and-negotiate phase, sent back to
/// the constructing thread over a std (non-tokio) channel.
enum ConnectOutcome {
    Connected {
        outbound_tx: mpsc::Sender<Vec<u8>>,
        shutdown_tx: oneshot::Sender<()>,
        inbound_rx: std_mpsc::Receiver<Vec<u8>>,
        diag_rx: std_mpsc::Receiver<String>,
    },
    Failed(RtcError),
}

impl RtcTransport {
    pub fn connect_as_offerer(
        relay_url: &str,
        session_id: &str,
        my_peer_id: &str,
        ice_servers: Vec<IceServer>,
        connect_timeout: Duration,
    ) -> Result<Self, RtcError> {
        Self::connect(
            relay_url,
            session_id,
            my_peer_id,
            ice_servers,
            connect_timeout,
            true,
            None,
        )
    }

    pub fn connect_as_answerer(
        relay_url: &str,
        session_id: &str,
        my_peer_id: &str,
        ice_servers: Vec<IceServer>,
        connect_timeout: Duration,
    ) -> Result<Self, RtcError> {
        Self::connect(
            relay_url,
            session_id,
            my_peer_id,
            ice_servers,
            connect_timeout,
            false,
            None,
        )
    }

    /// Like [`Self::connect_as_offerer`], but polls `cancel` while waiting for
    /// negotiation and aborts with [`RtcError::Cancelled`] once it is set.
    /// Matters most for the answerer, whose connect timeout can be many
    /// minutes (it covers a human relaying the room code).
    pub fn connect_as_offerer_cancellable(
        relay_url: &str,
        session_id: &str,
        my_peer_id: &str,
        ice_servers: Vec<IceServer>,
        connect_timeout: Duration,
        cancel: Arc<AtomicBool>,
    ) -> Result<Self, RtcError> {
        Self::connect(
            relay_url,
            session_id,
            my_peer_id,
            ice_servers,
            connect_timeout,
            true,
            Some(cancel),
        )
    }

    /// See [`Self::connect_as_offerer_cancellable`].
    pub fn connect_as_answerer_cancellable(
        relay_url: &str,
        session_id: &str,
        my_peer_id: &str,
        ice_servers: Vec<IceServer>,
        connect_timeout: Duration,
        cancel: Arc<AtomicBool>,
    ) -> Result<Self, RtcError> {
        Self::connect(
            relay_url,
            session_id,
            my_peer_id,
            ice_servers,
            connect_timeout,
            false,
            Some(cancel),
        )
    }

    fn connect(
        relay_url: &str,
        session_id: &str,
        my_peer_id: &str,
        ice_servers: Vec<IceServer>,
        connect_timeout: Duration,
        is_offerer: bool,
        cancel: Option<Arc<AtomicBool>>,
    ) -> Result<Self, RtcError> {
        let relay_url = relay_url.to_string();
        let session_id = session_id.to_string();
        let my_peer_id = my_peer_id.to_string();

        let (outcome_tx, outcome_rx) = std_mpsc::channel::<ConnectOutcome>();

        let runtime = BackgroundRuntime::spawn("plenum-rtc", move || async move {
            let connected = if is_offerer {
                run_offerer(&relay_url, &session_id, &my_peer_id, ice_servers).await
            } else {
                run_answerer(&relay_url, &session_id, &my_peer_id, ice_servers).await
            };

            let ConnectedChannel {
                peer_connection,
                data_channel,
                inbound_rx,
                diag_tx,
                diag_rx,
            } = match connected {
                Ok(connected) => connected,
                Err(error) => {
                    let _ = outcome_tx.send(ConnectOutcome::Failed(error));
                    return;
                }
            };

            let (outbound_tx, mut outbound_rx) = mpsc::channel::<Vec<u8>>(OUTBOUND_QUEUE_PACKETS);
            let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();

            if outcome_tx
                .send(ConnectOutcome::Connected {
                    outbound_tx,
                    shutdown_tx,
                    inbound_rx,
                    diag_rx,
                })
                .is_err()
            {
                // Constructing thread gave up (e.g. timed out); close down.
                let _ = peer_connection.close().await;
                return;
            }

            // Drive outbound sends and shutdown on this same background thread
            // for the remaining lifetime of the transport.
            //
            // `biased` makes select poll the outbound queue *before* the
            // shutdown signal on every iteration. Without it, when the engine
            // calls `send(Finish)` immediately followed by `close()`, both
            // branches are ready and a random pick can observe shutdown first,
            // dropping the final `Finish` packet — leaving the receiver stuck at
            // 100% having never seen end-of-transfer.
            let mut shutdown_requested = false;
            loop {
                tokio::select! {
                    biased;
                    maybe_bytes = outbound_rx.recv() => {
                        match maybe_bytes {
                            Some(bytes) => {
                                // Watermark pacing: don't stack packets onto an
                                // already-deep SCTP send buffer; wait for it to
                                // drain below the low watermark first. Keeps
                                // wire latency honest so the retransmit timer
                                // doesn't fire on packets that are merely
                                // queued, not lost.
                                if data_channel.buffered_amount().await >= BUFFERED_HIGH_WATERMARK {
                                    while !shutdown_requested
                                        && data_channel.buffered_amount().await > BUFFERED_LOW_WATERMARK
                                    {
                                        tokio::select! {
                                            _ = tokio::time::sleep(Duration::from_millis(5)) => {}
                                            _ = &mut shutdown_rx => {
                                                shutdown_requested = true;
                                            }
                                        }
                                    }
                                }
                                if let Err(error) = data_channel.send(&Bytes::from(bytes)).await {
                                    let _ = diag_tx.send(format!(
                                        "DIAG transport: data_channel.send failed mid-transfer: {error}"
                                    ));
                                    break;
                                }
                                if shutdown_requested {
                                    break;
                                }
                            }
                            None => break,
                        }
                    }
                    _ = &mut shutdown_rx, if !shutdown_requested => {
                        break;
                    }
                }
            }

            // Graceful close: flush anything still queued (bounded — the
            // outbound channel holds at most OUTBOUND_QUEUE_PACKETS packets),
            // then wait for the SCTP send buffer to drain so the peer actually
            // receives the final bytes (e.g. the `Finish` packet) before we
            // tear the connection down. webrtc-rs's `close()` is abrupt and
            // discards buffered data.
            while let Ok(bytes) = outbound_rx.try_recv() {
                if let Err(error) = data_channel.send(&Bytes::from(bytes)).await {
                    let _ = diag_tx.send(format!(
                        "DIAG transport: data_channel.send failed during close-flush: {error}"
                    ));
                    break;
                }
            }
            // Bounded wait (~5s) for the send buffer to empty. Ordered/reliable
            // SCTP guarantees delivery once buffered_amount reaches zero.
            for _ in 0..500 {
                if data_channel.buffered_amount().await == 0 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }

            let _ = peer_connection.close().await;
        });

        // Wait for the background thread's connect outcome in short slices so
        // a local cancel (user backed out / switched modes) takes effect
        // promptly instead of after the full connect timeout.
        let deadline = Instant::now() + connect_timeout;
        let outcome = loop {
            match outcome_rx.recv_timeout(Duration::from_millis(100)) {
                Ok(outcome) => break outcome,
                Err(std_mpsc::RecvTimeoutError::Timeout) => {
                    let cancelled = cancel
                        .as_ref()
                        .is_some_and(|flag| flag.load(Ordering::Relaxed));
                    if cancelled || Instant::now() >= deadline {
                        // The background thread is still mid-negotiation and
                        // only exits once its outcome send fails or its own
                        // network timeouts fire. Joining it here could block
                        // the caller (potentially for minutes), so hand the
                        // join to a detached reaper thread instead.
                        drop(outcome_rx);
                        std::thread::spawn(move || runtime.join());
                        return Err(if cancelled {
                            RtcError::Cancelled
                        } else {
                            RtcError::Timeout
                        });
                    }
                }
                Err(std_mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(RtcError::PeerConnection(
                        "rtc background thread exited before producing a connection".into(),
                    ));
                }
            }
        };

        match outcome {
            ConnectOutcome::Connected {
                outbound_tx,
                shutdown_tx,
                inbound_rx,
                diag_rx,
            } => Ok(Self {
                inbound_rx,
                outbound_tx,
                shutdown_tx: Some(shutdown_tx),
                diag_rx,
                runtime: Some(runtime),
                closed: false,
            }),
            ConnectOutcome::Failed(error) => {
                runtime.join();
                Err(error)
            }
        }
    }
}

impl Transport for RtcTransport {
    fn send(&mut self, bytes: &[u8]) -> TransportResult<()> {
        if self.closed {
            return Err(TransportError::Closed);
        }
        // Bounded queue: blocks the (synchronous) engine thread when the
        // background loop is paced on SCTP buffered_amount — this is the
        // backpressure that stops retransmission storms from snowballing.
        self.outbound_tx
            .blocking_send(bytes.to_vec())
            .map_err(|_| TransportError::Closed)
    }

    fn recv(&mut self) -> TransportResult<Option<Vec<u8>>> {
        if self.closed {
            return Err(TransportError::Closed);
        }
        match self.inbound_rx.try_recv() {
            Ok(bytes) => Ok(Some(bytes)),
            Err(std_mpsc::TryRecvError::Empty) => Ok(None),
            Err(std_mpsc::TryRecvError::Disconnected) => {
                self.closed = true;
                Err(TransportError::Closed)
            }
        }
    }

    fn close(&mut self) -> TransportResult<()> {
        self.closed = true;
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }
        if let Some(runtime) = self.runtime.take() {
            runtime.join();
        }
        Ok(())
    }

    fn is_closed(&self) -> bool {
        self.closed
    }

    fn poll_diagnostics(&mut self) -> Vec<String> {
        let mut diagnostics = Vec::new();
        while let Ok(message) = self.diag_rx.try_recv() {
            diagnostics.push(message);
        }
        diagnostics
    }
}

impl Drop for RtcTransport {
    fn drop(&mut self) {
        if !self.closed {
            let _ = self.close();
        }
    }
}
