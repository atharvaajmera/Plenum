//! `RtcTransport`: a `crate::transport::Transport` implementation backed by a
//! WebRTC data channel, negotiated over a relay/signaling WebSocket.

use std::sync::mpsc as std_mpsc;
use std::time::Duration;

use bytes::Bytes;
use tokio::sync::{mpsc, oneshot};

use crate::rtc::error::RtcError;
use crate::rtc::runtime::BackgroundRuntime;
use crate::rtc::signaling_client::{run_answerer, run_offerer, ConnectedChannel};
use crate::signaling::IceServer;
use crate::transport::{Transport, TransportError, TransportResult};

/// A `Transport` implementation over a WebRTC data channel.
///
/// Owns one dedicated OS thread running a `current_thread` tokio runtime, which
/// holds the `Arc<RTCPeerConnection>` and `Arc<RTCDataChannel>` for the lifetime
/// of this transport. `send`/`recv`/`close` bridge to that thread via channels,
/// matching `TcpTransport`'s non-blocking polling contract.
pub struct RtcTransport {
    inbound_rx: std_mpsc::Receiver<Vec<u8>>,
    outbound_tx: mpsc::UnboundedSender<Vec<u8>>,
    shutdown_tx: Option<oneshot::Sender<()>>,
    runtime: Option<BackgroundRuntime>,
    closed: bool,
}

/// Outcome of the background thread's connect-and-negotiate phase, sent back to
/// the constructing thread over a std (non-tokio) channel.
enum ConnectOutcome {
    Connected {
        outbound_tx: mpsc::UnboundedSender<Vec<u8>>,
        shutdown_tx: oneshot::Sender<()>,
        inbound_rx: std_mpsc::Receiver<Vec<u8>>,
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
        )
    }

    fn connect(
        relay_url: &str,
        session_id: &str,
        my_peer_id: &str,
        ice_servers: Vec<IceServer>,
        connect_timeout: Duration,
        is_offerer: bool,
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
            } = match connected {
                Ok(connected) => connected,
                Err(error) => {
                    let _ = outcome_tx.send(ConnectOutcome::Failed(error));
                    return;
                }
            };

            let (outbound_tx, mut outbound_rx) = mpsc::unbounded_channel::<Vec<u8>>();
            let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();

            if outcome_tx
                .send(ConnectOutcome::Connected {
                    outbound_tx,
                    shutdown_tx,
                    inbound_rx,
                })
                .is_err()
            {
                // Constructing thread gave up (e.g. timed out); close down.
                let _ = peer_connection.close().await;
                return;
            }

            // Drive outbound sends and shutdown on this same background thread
            // for the remaining lifetime of the transport.
            loop {
                tokio::select! {
                    _ = &mut shutdown_rx => {
                        break;
                    }
                    maybe_bytes = outbound_rx.recv() => {
                        match maybe_bytes {
                            Some(bytes) => {
                                if data_channel.send(&Bytes::from(bytes)).await.is_err() {
                                    break;
                                }
                            }
                            None => break,
                        }
                    }
                }
            }

            let _ = peer_connection.close().await;
        });

        let outcome = outcome_rx
            .recv_timeout(connect_timeout)
            .map_err(|_| RtcError::Timeout)?;

        match outcome {
            ConnectOutcome::Connected {
                outbound_tx,
                shutdown_tx,
                inbound_rx,
            } => Ok(Self {
                inbound_rx,
                outbound_tx,
                shutdown_tx: Some(shutdown_tx),
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
        self.outbound_tx
            .send(bytes.to_vec())
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
}

impl Drop for RtcTransport {
    fn drop(&mut self) {
        if !self.closed {
            let _ = self.close();
        }
    }
}
