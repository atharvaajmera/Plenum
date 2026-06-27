use std::time::{Duration, Instant};

use crate::transport::{Transport, TransportResult, TransportError};

pub struct MultipathTransport {
    local_path: Box<dyn Transport + Send + Sync>,
    control_path: Box<dyn Transport + Send + Sync>,
    
    local_active: bool,
    
    // Weighted scheduling (e.g., 95% local, 5% relay)
    local_weight: f64,
    control_weight: f64,
    
    chunks_sent_local: u64,
    chunks_sent_control: u64,
    
    // Active latency and congestion monitoring
    last_local_recv: Option<Instant>,
    last_control_recv: Option<Instant>,
    
    pub local_inter_arrival_avg: Duration,
    pub control_inter_arrival_avg: Duration,
    
    closed: bool,
}

impl MultipathTransport {
    pub fn new(local_path: Box<dyn Transport + Send + Sync>, control_path: Box<dyn Transport + Send + Sync>) -> Self {
        Self {
            local_path,
            control_path,
            local_active: true,
            local_weight: 0.95,
            control_weight: 0.05,
            chunks_sent_local: 0,
            chunks_sent_control: 0,
            last_local_recv: None,
            last_control_recv: None,
            local_inter_arrival_avg: Duration::from_millis(0),
            control_inter_arrival_avg: Duration::from_millis(0),
            closed: false,
        }
    }
    
    fn update_metrics(&mut self, is_local: bool) {
        let now = Instant::now();
        if is_local {
            if let Some(last) = self.last_local_recv {
                let diff = now.duration_since(last);
                if self.local_inter_arrival_avg.as_millis() == 0 {
                    self.local_inter_arrival_avg = diff;
                } else {
                    // Moving average
                    self.local_inter_arrival_avg = (self.local_inter_arrival_avg * 9 + diff) / 10;
                }
            }
            self.last_local_recv = Some(now);
        } else {
            if let Some(last) = self.last_control_recv {
                let diff = now.duration_since(last);
                if self.control_inter_arrival_avg.as_millis() == 0 {
                    self.control_inter_arrival_avg = diff;
                } else {
                    self.control_inter_arrival_avg = (self.control_inter_arrival_avg * 9 + diff) / 10;
                }
            }
            self.last_control_recv = Some(now);
        }
    }
    
    pub fn failover_to_control(&mut self) {
        self.local_active = false;
        self.local_weight = 0.0;
        self.control_weight = 1.0;
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }
}

impl Transport for MultipathTransport {
    fn send(&mut self, bytes: &[u8]) -> TransportResult<()> {
        let total_sent = self.chunks_sent_local + self.chunks_sent_control;
        let expected_local = (total_sent as f64 * self.local_weight) as u64;
        
        let target_local = self.local_active && self.chunks_sent_local <= expected_local;
        
        if target_local {
            match self.local_path.send(bytes) {
                Ok(_) => {
                    self.chunks_sent_local += 1;
                    Ok(())
                },
                Err(_) => {
                    // Millisecond-level fallback mechanism
                    self.failover_to_control();
                    self.control_path.send(bytes).map(|_| {
                        self.chunks_sent_control += 1;
                    })
                }
            }
        } else {
            match self.control_path.send(bytes) {
                Ok(_) => {
                    self.chunks_sent_control += 1;
                    Ok(())
                },
                Err(e) => Err(e),
            }
        }
    }
    
    fn recv(&mut self) -> TransportResult<Option<Vec<u8>>> {
        if self.local_active {
            match self.local_path.recv() {
                Ok(Some(data)) => {
                    self.update_metrics(true);
                    return Ok(Some(data));
                },
                Ok(None) => {}, // Try control path
                Err(_) => {
                    self.failover_to_control();
                }
            }
        }
        
        match self.control_path.recv() {
            Ok(Some(data)) => {
                self.update_metrics(false);
                Ok(Some(data))
            },
            Ok(None) => Ok(None),
            Err(e) => Err(e),
        }
    }
    
    fn close(&mut self) -> TransportResult<()> {
        self.closed = true;
        let mut err = None;
        if let Err(e) = self.local_path.close() {
            err = Some(e);
        }
        if let Err(e) = self.control_path.close() {
            err = Some(e);
        }
        if let Some(e) = err {
            return Err(e);
        }
        Ok(())
    }
}
