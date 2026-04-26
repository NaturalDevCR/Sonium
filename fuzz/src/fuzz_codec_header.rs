//! Fuzz target: CodecHeader + Opus header parser.

#![no_main]

use libfuzzer_sys::fuzz_target;
use sonium_protocol::messages::{CodecHeader, codec_header::parse_opus_codec_header};

fuzz_target!(|data: &[u8]| {
    // CodecHeader outer envelope
    if let Ok(hdr) = CodecHeader::decode(data) {
        let encoded = hdr.encode();
        let back    = CodecHeader::decode(&encoded).expect("round-trip");
        assert_eq!(back.codec, hdr.codec);

        // Inner Opus header — must not panic
        let _ = parse_opus_codec_header(&hdr.header_data);
    }

    // Also fuzz the inner Opus header parser directly
    let _ = parse_opus_codec_header(data);
});
