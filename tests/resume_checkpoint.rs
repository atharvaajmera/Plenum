use std::time::{SystemTime, UNIX_EPOCH};

use aether::flow::ReceiverWindow;
use aether::protocol::{Packet, PacketType};
use aether::stream::ResumeCheckpoint;

#[test]
fn checkpoint_roundtrip_persists_resume_metadata() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("aether-resume-{unique}.json"));

    let mut checkpoint = ResumeCheckpoint::new("example.bin", 12345, 4096);
    checkpoint.update(7, 28672);
    checkpoint.save(&path).expect("checkpoint should save");

    let loaded = ResumeCheckpoint::load(&path).expect("checkpoint should load");
    assert_eq!(loaded, checkpoint);

    ResumeCheckpoint::clear(&path).expect("checkpoint should clear");
    assert!(!path.exists());
}

#[test]
fn receiver_window_can_resume_from_existing_sequence() {
    let mut receiver = ReceiverWindow::with_next_expected(3);

    let controls = receiver
        .receive_data_packet(Packet::new(PacketType::Data, 1, b"old".to_vec()))
        .expect("duplicate old packet should still ack");
    assert_eq!(controls.len(), 1);
    assert_eq!(controls[0].packet_type, PacketType::Ack);
    assert_eq!(controls[0].sequence_no, 1);
    assert!(receiver.drain_ordered().is_empty());

    let controls = receiver
        .receive_data_packet(Packet::new(PacketType::Data, 3, b"new".to_vec()))
        .expect("current packet should succeed");
    assert_eq!(controls[0].sequence_no, 3);

    let drained = receiver.drain_ordered_packets();
    assert_eq!(drained, vec![(3, b"new".to_vec())]);
    assert_eq!(receiver.next_expected(), 4);
}
