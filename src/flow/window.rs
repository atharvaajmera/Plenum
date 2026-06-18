//! Sender and receiver sliding-window state.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::flow::FlowError;
use crate::protocol::{Packet, PacketType, encode_packet};
use crate::transport::Transport;

#[derive(Debug, Clone)]
struct InFlightPacket {
    sent_at: u64,
}

/// Tracks outgoing data packets, acknowledgements, retransmission timeouts, and
/// the configured number of packets allowed in flight.
#[derive(Debug)]
pub struct SenderWindow {
    window_size: usize,
    timeout_ticks: u64,
    packets: BTreeMap<u32, Packet>,
    pending: VecDeque<u32>,
    in_flight: BTreeMap<u32, InFlightPacket>,
    nacked: BTreeSet<u32>,
    acked: BTreeSet<u32>,
}

impl SenderWindow {
    pub fn new(window_size: usize, timeout_ticks: u64) -> Result<Self, FlowError> {
        if window_size == 0 {
            return Err(FlowError::InvalidWindowSize);
        }

        Ok(Self {
            window_size,
            timeout_ticks,
            packets: BTreeMap::new(),
            pending: VecDeque::new(),
            in_flight: BTreeMap::new(),
            nacked: BTreeSet::new(),
            acked: BTreeSet::new(),
        })
    }

    pub fn enqueue(&mut self, packet: Packet) -> Result<(), FlowError> {
        if packet.packet_type != PacketType::Data {
            return Err(FlowError::UnexpectedPacketType {
                actual: packet.packet_type,
            });
        }

        let sequence_no = packet.sequence_no;
        if self.packets.contains_key(&sequence_no)
            || self.acked.contains(&sequence_no)
            || self.in_flight.contains_key(&sequence_no)
        {
            return Err(FlowError::DuplicateSequence { sequence_no });
        }

        self.packets.insert(sequence_no, packet);
        self.pending.push_back(sequence_no);
        Ok(())
    }

    pub fn enqueue_unacked(&mut self, packet: Packet) -> Result<bool, FlowError> {
        if self.acked.contains(&packet.sequence_no) {
            return Ok(false);
        }

        self.enqueue(packet)?;
        Ok(true)
    }

    pub fn window_size(&self) -> usize {
        self.window_size
    }

    pub fn in_flight_len(&self) -> usize {
        self.in_flight.len()
    }

    pub fn acked_len(&self) -> usize {
        self.acked.len()
    }

    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty() && self.in_flight.is_empty()
    }

    pub fn buffered_payload_bytes(&self) -> usize {
        self.packets
            .values()
            .map(|packet| packet.payload.len())
            .sum()
    }

    /// Sends unsent packets while there is window capacity.
    pub fn send_available<T: Transport>(
        &mut self,
        transport: &mut T,
        now: u64,
    ) -> Result<usize, FlowError> {
        let mut sent = 0;

        while self.in_flight.len() < self.window_size {
            let Some(sequence_no) = self.pending.pop_front() else {
                break;
            };

            if self.acked.contains(&sequence_no) || self.in_flight.contains_key(&sequence_no) {
                continue;
            }

            let packet = self
                .packets
                .get(&sequence_no)
                .ok_or(FlowError::UnknownSequence { sequence_no })?
                .clone();
            send_packet(transport, &packet)?;
            self.in_flight
                .insert(sequence_no, InFlightPacket { sent_at: now });
            sent += 1;
        }

        Ok(sent)
    }

    /// Applies ACK/NACK control packets from the receiver.
    pub fn handle_control_packet(&mut self, packet: &Packet) -> Result<(), FlowError> {
        match packet.packet_type {
            PacketType::Ack => {
                self.in_flight.remove(&packet.sequence_no);
                self.nacked.remove(&packet.sequence_no);
                self.acked.insert(packet.sequence_no);
                self.packets.remove(&packet.sequence_no);
                Ok(())
            }
            PacketType::Nack => {
                if self.acked.contains(&packet.sequence_no) {
                    return Ok(());
                }

                if !self.packets.contains_key(&packet.sequence_no) {
                    return Err(FlowError::UnknownSequence {
                        sequence_no: packet.sequence_no,
                    });
                }

                self.nacked.insert(packet.sequence_no);
                Ok(())
            }
            actual => Err(FlowError::UnexpectedPacketType { actual }),
        }
    }

    /// Retransmits packets explicitly NACKed or timed out.
    ///
    /// Retransmissions do not consume additional window capacity because they
    /// replace packets already considered in flight.
    pub fn retransmit_due<T: Transport>(
        &mut self,
        transport: &mut T,
        now: u64,
    ) -> Result<usize, FlowError> {
        let timed_out: Vec<u32> = self
            .in_flight
            .iter()
            .filter_map(|(&sequence_no, in_flight)| {
                let elapsed = now.saturating_sub(in_flight.sent_at);
                (elapsed >= self.timeout_ticks).then_some(sequence_no)
            })
            .collect();

        let mut due: BTreeSet<u32> = timed_out.into_iter().collect();
        due.append(&mut self.nacked);

        let mut sent = 0;
        for sequence_no in due {
            if self.acked.contains(&sequence_no) {
                continue;
            }

            let packet = self
                .packets
                .get(&sequence_no)
                .ok_or(FlowError::UnknownSequence { sequence_no })?
                .clone();

            send_packet(transport, &packet)?;
            self.in_flight
                .insert(sequence_no, InFlightPacket { sent_at: now });
            sent += 1;
        }

        Ok(sent)
    }
}

/// Tracks incoming data packets, emits ACK/NACK control packets, buffers
/// out-of-order data, and releases ordered payloads to the stream layer.
#[derive(Debug, Default)]
pub struct ReceiverWindow {
    next_expected: u32,
    buffered: BTreeMap<u32, Vec<u8>>,
    delivered: BTreeSet<u32>,
}

impl ReceiverWindow {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_next_expected(next_expected: u32) -> Self {
        Self {
            next_expected,
            ..Self::default()
        }
    }

    pub fn next_expected(&self) -> u32 {
        self.next_expected
    }

    pub fn buffered_len(&self) -> usize {
        self.buffered.len()
    }

    pub fn receive_data_packet(&mut self, packet: Packet) -> Result<Vec<Packet>, FlowError> {
        if packet.packet_type != PacketType::Data {
            return Err(FlowError::UnexpectedPacketType {
                actual: packet.packet_type,
            });
        }

        let sequence_no = packet.sequence_no;
        let mut controls = vec![Packet::new(PacketType::Ack, sequence_no, Vec::new())];

        if sequence_no < self.next_expected {
            return Ok(controls);
        }

        if !self.delivered.contains(&sequence_no) && !self.buffered.contains_key(&sequence_no) {
            self.buffered.insert(sequence_no, packet.payload);
        }

        if sequence_no > self.next_expected && !self.buffered.contains_key(&self.next_expected) {
            controls.push(Packet::new(
                PacketType::Nack,
                self.next_expected,
                Vec::new(),
            ));
        }

        Ok(controls)
    }

    pub fn buffered_payload_bytes(&self) -> usize {
        self.buffered.values().map(Vec::len).sum()
    }

    /// Drains contiguous ordered packets starting at the next expected sequence.
    pub fn drain_ordered_packets(&mut self) -> Vec<(u32, Vec<u8>)> {
        let mut packets = Vec::new();

        while let Some(payload) = self.buffered.remove(&self.next_expected) {
            let sequence_no = self.next_expected;
            self.delivered.insert(sequence_no);
            self.next_expected = self.next_expected.saturating_add(1);
            packets.push((sequence_no, payload));
        }

        packets
    }

    /// Drains contiguous ordered payloads starting at the next expected sequence.
    pub fn drain_ordered(&mut self) -> Vec<Vec<u8>> {
        self.drain_ordered_packets()
            .into_iter()
            .map(|(_, payload)| payload)
            .collect()
    }
}

fn send_packet<T: Transport>(transport: &mut T, packet: &Packet) -> Result<(), FlowError> {
    let bytes = encode_packet(packet)?;
    transport.send(&bytes)?;
    Ok(())
}
