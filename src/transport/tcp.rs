//! TCP transport implementation.
//!
//! `TcpTransport` preserves the byte-frame boundary expected by the `Transport`
//! trait by prefixing every frame with a four-byte big-endian length.

use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::transport::{Transport, TransportError, TransportResult};

const LENGTH_PREFIX_LEN: usize = 4;
const DEFAULT_MAX_FRAME_LEN: usize = 64 * 1024 * 1024;
const DEFAULT_READ_TIMEOUT: Duration = Duration::from_millis(5);

#[derive(Debug)]
pub struct TcpTransport {
    stream: TcpStream,
    max_frame_len: usize,
    closed: bool,
}

impl TcpTransport {
    pub fn connect(addr: impl ToSocketAddrs) -> TransportResult<Self> {
        let stream = TcpStream::connect(addr)?;
        Self::from_stream(stream)
    }

    pub fn accept(listener: &TcpListener) -> TransportResult<Self> {
        let (stream, _) = listener.accept()?;
        Self::from_stream(stream)
    }

    pub fn from_stream(stream: TcpStream) -> TransportResult<Self> {
        stream.set_nodelay(true)?;
        stream.set_read_timeout(Some(DEFAULT_READ_TIMEOUT))?;
        Ok(Self {
            stream,
            max_frame_len: DEFAULT_MAX_FRAME_LEN,
            closed: false,
        })
    }

    pub fn peer_addr(&self) -> TransportResult<SocketAddr> {
        Ok(self.stream.peer_addr()?)
    }

    pub fn max_frame_len(&self) -> usize {
        self.max_frame_len
    }

    pub fn set_max_frame_len(&mut self, max_frame_len: usize) {
        self.max_frame_len = max_frame_len;
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }

    fn read_exact_or_none(&mut self, bytes: &mut [u8]) -> TransportResult<Option<()>> {
        match self.stream.read_exact(bytes) {
            Ok(()) => Ok(Some(())),
            Err(error)
                if matches!(
                    error.kind(),
                    ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
                ) =>
            {
                Ok(None)
            }
            Err(error) if error.kind() == ErrorKind::UnexpectedEof => {
                self.closed = true;
                Err(TransportError::Closed)
            }
            Err(error) => Err(error.into()),
        }
    }
}

impl Transport for TcpTransport {
    fn send(&mut self, bytes: &[u8]) -> TransportResult<()> {
        if self.closed {
            return Err(TransportError::Closed);
        }

        if bytes.len() > self.max_frame_len || bytes.len() > u32::MAX as usize {
            return Err(TransportError::FrameTooLarge {
                len: bytes.len(),
                max: self.max_frame_len.min(u32::MAX as usize),
            });
        }

        let len = bytes.len() as u32;
        self.stream.write_all(&len.to_be_bytes())?;
        self.stream.write_all(bytes)?;
        self.stream.flush()?;
        Ok(())
    }

    fn recv(&mut self) -> TransportResult<Option<Vec<u8>>> {
        if self.closed {
            return Err(TransportError::Closed);
        }

        let mut length_prefix = [0_u8; LENGTH_PREFIX_LEN];
        let Some(()) = self.read_exact_or_none(&mut length_prefix)? else {
            return Ok(None);
        };

        let frame_len = u32::from_be_bytes(length_prefix) as usize;
        if frame_len > self.max_frame_len {
            return Err(TransportError::FrameTooLarge {
                len: frame_len,
                max: self.max_frame_len,
            });
        }

        let mut frame = vec![0_u8; frame_len];
        if frame_len > 0 {
            let Some(()) = self.read_exact_or_none(&mut frame)? else {
                return Ok(None);
            };
        }

        Ok(Some(frame))
    }

    fn close(&mut self) -> TransportResult<()> {
        self.closed = true;
        self.stream.shutdown(std::net::Shutdown::Both)?;
        Ok(())
    }

    fn is_closed(&self) -> bool {
        self.closed
    }
}
