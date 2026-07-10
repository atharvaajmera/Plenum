//! Dedicated single-thread tokio runtime helper.
//!
//! `RtcTransport` presents a synchronous, blocking/non-blocking-poll API (mirroring
//! `TcpTransport`) on top of webrtc-rs, which is fundamentally async. Each transport
//! owns exactly one OS thread running a `current_thread` tokio runtime; all
//! `RTCPeerConnection`/`RTCDataChannel`/signaling-websocket state lives on that
//! thread for the lifetime of the transport.

use std::thread::JoinHandle;

/// A background OS thread driving a dedicated `current_thread` tokio runtime.
///
/// Dropping this handle does not stop the thread — callers are expected to signal
/// shutdown themselves (e.g. via a oneshot channel) and then call [`Self::join`].
pub struct BackgroundRuntime {
    handle: Option<JoinHandle<()>>,
}

impl BackgroundRuntime {
    /// Spawn a new OS thread, build a `current_thread` tokio runtime on it, and run
    /// `future` to completion via `block_on`. `future` is produced lazily by
    /// `future_factory` *on the background thread*, so it (and anything it captures)
    /// never needs to be `Send`.
    pub fn spawn<F, Fut>(thread_name: &str, future_factory: F) -> Self
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + 'static,
    {
        let handle = std::thread::Builder::new()
            .name(thread_name.to_string())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        eprintln!("plenum rtc: failed to build tokio runtime: {error}");
                        return;
                    }
                };

                runtime.block_on(future_factory());
            })
            .expect("failed to spawn rtc background thread");

        Self {
            handle: Some(handle),
        }
    }

    /// Block the calling thread until the background thread exits.
    pub fn join(mut self) {
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for BackgroundRuntime {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}
