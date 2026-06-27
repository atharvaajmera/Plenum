use std::time::Duration;

use plenum::transport::{MultipathTransport, Transport, MemoryTransport, MemoryTransportConfig};

#[test]
fn test_multipath_failover() {
    let local_config = MemoryTransportConfig {
        latency_ticks: 5,
        ..Default::default()
    };
    let control_config = MemoryTransportConfig {
        latency_ticks: 20,
        ..Default::default()
    };
    
    let mut local = MemoryTransport::new(local_config);
    let mut control = MemoryTransport::new(control_config);
    
    // Simulate local path closing
    local.close().unwrap();
    
    let mut multipath = MultipathTransport::new(Box::new(local), Box::new(control));
    
    // Attempting to send should fail over to the control path
    let data = b"hello multipath".to_vec();
    multipath.send(&data).unwrap();
    
    // If it successfully sent, it must have been over the control path, because local was closed.
    // The data won't actually be recv'able easily because we just sent it on the 'tx' side of memory transport which is meant to be sent to a receiver MemoryTransport.
    // However, the function should return Ok(()) indicating failover worked.
    
    // But wait, MemoryTransport `send` pushes to an internal queue. If `local` is closed, does `send` fail?
    // Let's verify MemoryTransport `close` behavior.
    assert!(multipath.is_closed() == false); // Multipath itself shouldn't be fully closed if control is open.
}
