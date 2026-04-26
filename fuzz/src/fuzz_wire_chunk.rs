//! Fuzz target: WireChunk payload parser.
//!
//! The most frequently received message — any malformed chunk must be
//! rejected cleanly, never panic.

#![no_main]

use libfuzzer_sys::fuzz_target;
use sonium_protocol::messages::WireChunk;

fuzz_target!(|data: &[u8]| {
    // Parse must not panic on arbitrary input
    if let Ok(chunk) = WireChunk::decode(data) {
        // If parse succeeds, round-trip must be stable
        let encoded = chunk.encode();
        let back    = WireChunk::decode(&encoded).expect("round-trip must parse");
        assert_eq!(back.timestamp.sec,  chunk.timestamp.sec);
        assert_eq!(back.timestamp.usec, chunk.timestamp.usec);
        assert_eq!(back.data,           chunk.data);
    }
});
