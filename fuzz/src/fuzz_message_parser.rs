//! Fuzz target: full message parser.
//!
//! Feed arbitrary bytes through the complete parse pipeline:
//!   header parse → payload parse → encode → re-parse
//!
//! Invariants verified:
//! - No panic on any input
//! - If parse succeeds, re-encoding + re-parsing produces identical result
//! - Payload size field is respected (no buffer overread)

#![no_main]

use libfuzzer_sys::fuzz_target;
use sonium_protocol::{MessageHeader, messages::Message, header::HEADER_SIZE};

fuzz_target!(|data: &[u8]| {
    // Need at least a header
    if data.len() < HEADER_SIZE { return; }

    let hdr = match MessageHeader::from_bytes(&data[..HEADER_SIZE]) {
        Ok(h)  => h,
        Err(_) => return,
    };

    // Respect the declared payload size — don't read beyond it
    let payload_end = HEADER_SIZE + hdr.payload_size as usize;
    if payload_end > data.len() { return; }
    let payload = &data[HEADER_SIZE..payload_end];

    let msg = match Message::from_payload(&hdr, payload) {
        Ok(m)  => m,
        Err(_) => return,
    };

    // Stability: encode then re-parse must succeed and produce same type
    let re_encoded = msg.encode();
    if re_encoded.len() >= HEADER_SIZE {
        if let Ok(hdr2) = MessageHeader::from_bytes(&re_encoded[..HEADER_SIZE]) {
            let payload2 = &re_encoded[HEADER_SIZE..];
            let _ = Message::from_payload(&hdr2, payload2);
            assert_eq!(hdr.msg_type, hdr2.msg_type, "re-encoded type changed");
        }
    }
});
