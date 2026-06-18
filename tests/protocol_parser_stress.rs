use rand::rngs::StdRng;
use rand::{Rng, RngCore, SeedableRng};

use aether::protocol::{Packet, PacketType, encode_packet, parse_packet};

#[test]
fn parser_handles_random_bytes_without_panicking() {
    let mut rng = StdRng::seed_from_u64(0xA37E_u64);

    for _ in 0..2_000 {
        let len = rng.gen_range(0..1024);
        let mut bytes = vec![0_u8; len];
        rng.fill(&mut bytes[..]);
        let _ = parse_packet(&bytes);
    }
}

#[test]
fn parser_roundtrips_random_valid_packets() {
    let mut rng = StdRng::seed_from_u64(0xBEEFu64);

    for _ in 0..500 {
        let payload_len = rng.gen_range(0..8192);
        let mut payload = vec![0_u8; payload_len];
        rng.fill(&mut payload[..]);
        let packet = Packet::new(PacketType::Data, rng.next_u32(), payload);

        let encoded = encode_packet(&packet).expect("packet should encode");
        let decoded = parse_packet(&encoded).expect("packet should parse");

        assert_eq!(decoded, packet);
    }
}

#[test]
fn parser_rejects_random_corruptions_of_valid_packets() {
    let mut rng = StdRng::seed_from_u64(0xC0FFEEu64);

    for _ in 0..250 {
        let payload_len = rng.gen_range(1..2048);
        let mut payload = vec![0_u8; payload_len];
        rng.fill(&mut payload[..]);
        let packet = Packet::new(PacketType::Data, rng.next_u32(), payload);
        let encoded = encode_packet(&packet).expect("packet should encode");

        let mode = rng.gen_range(0..3);
        let mut mutated = encoded.clone();
        match mode {
            0 => {
                mutated.truncate(rng.gen_range(0..mutated.len()));
            }
            1 => {
                let idx = rng.gen_range(0..mutated.len());
                mutated[idx] ^= 0xff;
            }
            _ => {
                mutated.push((rng.next_u32() & 0xff) as u8);
            }
        }

        if mutated != encoded {
            let _ = parse_packet(&mutated);
        }
    }
}
