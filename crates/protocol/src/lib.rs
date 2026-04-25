//! # sonium-protocol
//!
//! Binary wire protocol compatible with [Snapcast v2].
//!
//! ## Message framing
//!
//! Every message on the wire starts with a fixed 26-byte **header** followed
//! by a variable-length **payload**:
//!
//! ```text
//! ┌────────────────────────────────────────────── 26 bytes ──┐
//! │ type(u16) │ id(u16) │ refers_to(u16)                     │
//! │ sent_sec(i32) │ sent_usec(i32)                           │
//! │ recv_sec(i32) │ recv_usec(i32)                           │
//! │ payload_size(u32)                                        │
//! └──────────────────────────────────────────────────────────┘
//! │ payload[payload_size] …                                  │
//! ```
//!
//! All integer fields are **little-endian**.  The header is defined in
//! [`header::MessageHeader`].
//!
//! ## Message types
//!
//! | ID | Type | Direction | Description |
//! |----|------|-----------|-------------|
//! | 1  | [`CodecHeader`] | S→C | Codec init data sent once at stream start |
//! | 2  | [`WireChunk`]   | S→C | One encoded audio frame + playout timestamp |
//! | 3  | [`ServerSettings`] | S→C | Volume, mute, buffer config |
//! | 4  | [`TimeMsg`]     | C↔S | NTP-like clock sync |
//! | 5  | [`Hello`]       | C→S | Client introduction on connect |
//! | 7  | [`ClientInfo`]  | C→S | Volume / mute update from client |
//! | 8  | [`ErrorMsg`]    | S→C | Error notification |
//!
//! [`CodecHeader`]:    messages::CodecHeader
//! [`WireChunk`]:      messages::WireChunk
//! [`ServerSettings`]: messages::ServerSettings
//! [`TimeMsg`]:        messages::TimeMsg
//! [`Hello`]:          messages::Hello
//! [`ClientInfo`]:     messages::ClientInfo
//! [`ErrorMsg`]:       messages::ErrorMsg
//! [Snapcast v2]: https://github.com/badaix/snapcast/blob/master/doc/binary_protocol.md

pub mod header;
pub mod wire;
pub mod messages;

pub use header::{MessageHeader, MessageType, Timestamp};
pub use wire::{WireRead, WireWrite};
pub use messages::Message;
