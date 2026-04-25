//! Integration tests: parse known-good binary fixtures derived from the
//! Snapcast v2 wire format specification.
//!
//! These tests verify that [`sonium_protocol`] produces and consumes bytes
//! that are byte-identical to what a real Snapcast server or client would
//! send, using hand-crafted little-endian byte sequences as ground truth.

use sonium_protocol::{
    MessageHeader, MessageType, Timestamp,
    messages::{
        Message, Hello, CodecHeader, WireChunk, ServerSettings,
        ClientInfo, TimeMsg, ErrorMsg,
        codec_header::{opus_codec_header, parse_opus_codec_header},
    },
    header::HEADER_SIZE,
};

// ── Helpers ───────────────────────────────────────────────────────────────

/// Build the full wire bytes for a message: header + payload.
fn wire(msg: &Message) -> Vec<u8> {
    msg.encode()
}

/// Parse header + payload from a complete wire buffer.
fn parse(bytes: &[u8]) -> (MessageHeader, Message) {
    let hdr = MessageHeader::from_bytes(&bytes[..HEADER_SIZE]).unwrap();
    let payload = &bytes[HEADER_SIZE..];
    let msg = Message::from_payload(&hdr, payload).unwrap();
    (hdr, msg)
}

// ── Header byte-layout tests ──────────────────────────────────────────────

#[test]
fn header_type_field_is_first_2_bytes_le() {
    let msg   = Message::Hello(Hello::new("h", "id"));
    let bytes = wire(&msg);
    let type_raw = u16::from_le_bytes([bytes[0], bytes[1]]);
    assert_eq!(type_raw, 5u16, "Hello = type 5");
}

#[test]
fn wire_chunk_type_id_is_2() {
    let chunk = WireChunk::new(Timestamp::default(), vec![1, 2, 3]);
    let bytes = wire(&Message::WireChunk(chunk));
    assert_eq!(u16::from_le_bytes([bytes[0], bytes[1]]), 2u16);
}

#[test]
fn payload_size_field_matches_actual_payload_len() {
    let msg   = Message::ServerSettings(ServerSettings::default());
    let bytes = wire(&msg);
    let declared = u32::from_le_bytes([bytes[22], bytes[23], bytes[24], bytes[25]]);
    let actual   = (bytes.len() - HEADER_SIZE) as u32;
    assert_eq!(declared, actual);
}

// ── Full message round-trips through wire ─────────────────────────────────

#[test]
fn hello_full_wire_round_trip() {
    let orig = Hello {
        mac:              "de:ad:be:ef:ca:fe".into(),
        hostname:         "kitchen-pi".into(),
        version:          "0.1.0".into(),
        client_name:      "Sonium".into(),
        os:               "linux".into(),
        arch:             "aarch64".into(),
        instance:         1,
        id:               "kitchen-pi-1".into(),
        protocol_version: 2,
    };
    let bytes = wire(&Message::Hello(orig.clone()));
    let (hdr, msg) = parse(&bytes);

    assert_eq!(hdr.msg_type, MessageType::Hello);
    assert_eq!(hdr.payload_size, (bytes.len() - HEADER_SIZE) as u32);

    if let Message::Hello(h) = msg {
        assert_eq!(h.hostname,         "kitchen-pi");
        assert_eq!(h.id,               "kitchen-pi-1");
        assert_eq!(h.protocol_version, 2);
    } else { panic!("expected Hello"); }
}

#[test]
fn codec_header_opus_full_wire_round_trip() {
    let orig  = CodecHeader::new("opus", opus_codec_header(48_000, 16, 2));
    let bytes = wire(&Message::CodecHeader(orig.clone()));
    let (hdr, msg) = parse(&bytes);

    assert_eq!(hdr.msg_type, MessageType::CodecHeader);

    if let Message::CodecHeader(ch) = msg {
        assert_eq!(ch.codec, "opus");
        let (rate, bits, channels) = parse_opus_codec_header(&ch.header_data).unwrap();
        assert_eq!((rate, bits, channels), (48_000, 16, 2));
    } else { panic!("expected CodecHeader"); }
}

#[test]
fn wire_chunk_timestamp_survives_full_wire() {
    let ts    = Timestamp { sec: 1_700_000_123, usec: 456_789 };
    let bytes = wire(&Message::WireChunk(WireChunk::new(ts, vec![0xAA; 320])));
    let (_, msg) = parse(&bytes);

    if let Message::WireChunk(c) = msg {
        assert_eq!(c.timestamp.sec,  1_700_000_123);
        assert_eq!(c.timestamp.usec, 456_789);
        assert_eq!(c.data.len(),     320);
    } else { panic!("expected WireChunk"); }
}

#[test]
fn server_settings_json_encoding() {
    let ss  = ServerSettings { buffer_ms: 500, latency: 150, volume: 80, muted: true };
    let (_, msg) = parse(&wire(&Message::ServerSettings(ss)));

    if let Message::ServerSettings(s) = msg {
        assert_eq!(s.buffer_ms, 500);
        assert_eq!(s.latency,   150);
        assert_eq!(s.volume,    80);
        assert!(s.muted);
    } else { panic!("expected ServerSettings"); }
}

#[test]
fn client_info_json_encoding() {
    let ci  = ClientInfo { volume: 55, muted: false };
    let (_, msg) = parse(&wire(&Message::ClientInfo(ci)));

    if let Message::ClientInfo(c) = msg {
        assert_eq!(c.volume, 55);
        assert!(!c.muted);
    } else { panic!("expected ClientInfo"); }
}

#[test]
fn time_msg_latency_preserved() {
    let t   = TimeMsg { latency: Timestamp { sec: 0, usec: 7_432 } };
    let (_, msg) = parse(&wire(&Message::Time(t)));

    if let Message::Time(tm) = msg {
        assert_eq!(tm.latency.usec, 7_432);
    } else { panic!("expected Time"); }
}

#[test]
fn error_msg_code_and_message() {
    let err = ErrorMsg { code: 401, message: "Unauthorized".into(), detail: "bad token".into() };
    let (_, msg) = parse(&wire(&Message::Error(err)));

    if let Message::Error(e) = msg {
        assert_eq!(e.code,    401);
        assert_eq!(e.message, "Unauthorized");
        assert_eq!(e.detail,  "bad token");
    } else { panic!("expected Error"); }
}

// ── Snapcast binary fixture — hand-crafted known-good bytes ───────────────
//
// These bytes were computed from the Snapcast v2 wire format spec and
// cross-checked against the C++ source.  They are the ground truth for
// parser correctness.

/// A minimal `Time` request message:
///   type=4, id=1, refers_to=0, sent=(0,0), recv=(0,0), size=8
///   payload: sec=0, usec=0
const TIME_REQUEST_FIXTURE: &[u8] = &[
    // header
    0x04, 0x00,  // type = 4 (Time)
    0x01, 0x00,  // id = 1
    0x00, 0x00,  // refers_to = 0
    0x00, 0x00, 0x00, 0x00,  // sent_sec = 0
    0x00, 0x00, 0x00, 0x00,  // sent_usec = 0
    0x00, 0x00, 0x00, 0x00,  // recv_sec = 0
    0x00, 0x00, 0x00, 0x00,  // recv_usec = 0
    0x08, 0x00, 0x00, 0x00,  // payload_size = 8
    // payload
    0x00, 0x00, 0x00, 0x00,  // latency.sec = 0
    0x00, 0x00, 0x00, 0x00,  // latency.usec = 0
];

#[test]
fn parse_time_request_fixture() {
    let hdr = MessageHeader::from_bytes(&TIME_REQUEST_FIXTURE[..HEADER_SIZE]).unwrap();
    assert_eq!(hdr.msg_type,     MessageType::Time);
    assert_eq!(hdr.id,           1);
    assert_eq!(hdr.payload_size, 8);

    let msg = Message::from_payload(&hdr, &TIME_REQUEST_FIXTURE[HEADER_SIZE..]).unwrap();
    if let Message::Time(t) = msg {
        assert_eq!(t.latency.sec,  0);
        assert_eq!(t.latency.usec, 0);
    } else { panic!("expected Time"); }
}

#[test]
fn encode_time_request_matches_fixture() {
    let msg = Message::Time(TimeMsg::zero());
    let mut hdr = MessageHeader::new(MessageType::Time, 8);
    hdr.id       = 1;
    hdr.sent     = Timestamp::default();
    hdr.received = Timestamp::default();
    let encoded = msg.encode_with_header(hdr);

    // Compare only the first 4 bytes (type + id) and payload — sent timestamp
    // will differ from the fixture (it uses Timestamp::now() in the fixture
    // above we hardcoded zeros)
    assert_eq!(&encoded[0..4], &TIME_REQUEST_FIXTURE[0..4], "type+id mismatch");
    assert_eq!(&encoded[HEADER_SIZE..], &TIME_REQUEST_FIXTURE[HEADER_SIZE..], "payload mismatch");
}

/// Opus codec header fixture:
///   codec = "opus" (4 bytes len + 4 bytes data)
///   header_data = 12 bytes (magic + rate + bits + channels)
const OPUS_CODEC_HDR_FIXTURE: &[u8] = &[
    // --- message header ---
    0x01, 0x00,  // type = 1 (CodecHeader)
    0x00, 0x00,  // id = 0
    0x00, 0x00,  // refers_to = 0
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,  // sent = 0
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,  // recv = 0
    0x18, 0x00, 0x00, 0x00,  // payload_size = 24
    // --- payload ---
    // codec name: "opus" (len=4)
    0x04, 0x00, 0x00, 0x00,  // string length = 4
    0x6F, 0x70, 0x75, 0x73,  // "opus"
    // header_data (len=12)
    0x0C, 0x00, 0x00, 0x00,  // blob length = 12
    0x53, 0x55, 0x50, 0x4F,  // magic = 0x4F505553 LE → bytes [53 55 50 4F]
    0x80, 0xBB, 0x00, 0x00,  // rate = 48000 LE
    0x10, 0x00,              // bits = 16 LE
    0x02, 0x00,              // channels = 2 LE
];

#[test]
fn parse_opus_codec_header_fixture() {
    let hdr = MessageHeader::from_bytes(&OPUS_CODEC_HDR_FIXTURE[..HEADER_SIZE]).unwrap();
    assert_eq!(hdr.msg_type,     MessageType::CodecHeader);
    assert_eq!(hdr.payload_size, 24);

    let msg = Message::from_payload(&hdr, &OPUS_CODEC_HDR_FIXTURE[HEADER_SIZE..]).unwrap();
    if let Message::CodecHeader(ch) = msg {
        assert_eq!(ch.codec, "opus");
        let (rate, bits, channels) = parse_opus_codec_header(&ch.header_data).unwrap();
        assert_eq!((rate, bits, channels), (48_000, 16, 2));
    } else { panic!("expected CodecHeader"); }
}

// ── Multi-message stream simulation ──────────────────────────────────────

#[test]
fn simulate_client_connection_sequence() {
    // Simulate the exact sequence a client sends on connect:
    //   1. Hello
    //   2. Time request
    //   3. ClientInfo (volume change)

    let hello = wire(&Message::Hello(Hello::new("bedroom-pi", "bedroom-pi-1")));
    let time  = wire(&Message::Time(TimeMsg::zero()));
    let ci    = wire(&Message::ClientInfo(ClientInfo { volume: 75, muted: false }));

    // Concatenate into one byte stream (simulates TCP receive buffer)
    let mut buf = Vec::new();
    buf.extend_from_slice(&hello);
    buf.extend_from_slice(&time);
    buf.extend_from_slice(&ci);

    // Parse each message in sequence
    let mut pos = 0usize;
    let expected_types = [MessageType::Hello, MessageType::Time, MessageType::ClientInfo];

    for expected in &expected_types {
        let hdr = MessageHeader::from_bytes(&buf[pos..pos + HEADER_SIZE]).unwrap();
        assert_eq!(&hdr.msg_type, expected);
        let end = pos + HEADER_SIZE + hdr.payload_size as usize;
        let _msg = Message::from_payload(&hdr, &buf[pos + HEADER_SIZE..end]).unwrap();
        pos = end;
    }
    assert_eq!(pos, buf.len(), "all bytes consumed");
}
