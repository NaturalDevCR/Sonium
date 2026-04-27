pub mod hello;
pub mod server_settings;
pub mod client_info;
pub mod codec_header;
pub mod wire_chunk;
pub mod time;
pub mod error;

pub use hello::Hello;
pub use server_settings::{EqBand, ServerSettings};
pub use client_info::ClientInfo;
pub use codec_header::CodecHeader;
pub use wire_chunk::WireChunk;
pub use time::TimeMsg;
pub use error::ErrorMsg;

use crate::header::{MessageHeader, MessageType, HEADER_SIZE};
use sonium_common::SoniumError;

/// A fully parsed message including its decoded payload.
#[derive(Debug, Clone)]
pub enum Message {
    Hello(Hello),
    ServerSettings(ServerSettings),
    ClientInfo(ClientInfo),
    CodecHeader(CodecHeader),
    WireChunk(WireChunk),
    Time(TimeMsg),
    Error(ErrorMsg),
}

impl Message {
    pub fn message_type(&self) -> MessageType {
        match self {
            Self::Hello(_)          => MessageType::Hello,
            Self::ServerSettings(_) => MessageType::ServerSettings,
            Self::ClientInfo(_)     => MessageType::ClientInfo,
            Self::CodecHeader(_)    => MessageType::CodecHeader,
            Self::WireChunk(_)      => MessageType::WireChunk,
            Self::Time(_)           => MessageType::Time,
            Self::Error(_)          => MessageType::ErrorMsg,
        }
    }

    /// Deserialize a message given a parsed header and its raw payload bytes.
    pub fn from_payload(hdr: &MessageHeader, payload: &[u8]) -> sonium_common::error::Result<Self> {
        match hdr.msg_type {
            MessageType::Hello          => Ok(Self::Hello(Hello::decode(payload)?)),
            MessageType::ServerSettings => Ok(Self::ServerSettings(ServerSettings::decode(payload)?)),
            MessageType::ClientInfo     => Ok(Self::ClientInfo(ClientInfo::decode(payload)?)),
            MessageType::CodecHeader    => Ok(Self::CodecHeader(CodecHeader::decode(payload)?)),
            MessageType::WireChunk      => Ok(Self::WireChunk(WireChunk::decode(payload)?)),
            MessageType::Time           => Ok(Self::Time(TimeMsg::decode(payload)?)),
            MessageType::ErrorMsg          => Ok(Self::Error(ErrorMsg::decode(payload)?)),
            MessageType::Base           => Err(SoniumError::Protocol("base message unsupported".into())),
        }
    }

    /// Serialize to wire bytes: header + payload.
    pub fn encode(&self) -> Vec<u8> {
        let payload = self.encode_payload();
        let hdr = MessageHeader::new(self.message_type(), payload.len() as u32);
        let mut out = Vec::with_capacity(HEADER_SIZE + payload.len());
        out.extend_from_slice(&hdr.to_bytes());
        out.extend_from_slice(&payload);
        out
    }

    /// Serialize to wire bytes with explicit timestamp and id fields.
    pub fn encode_with_header(&self, mut hdr: MessageHeader) -> Vec<u8> {
        let payload = self.encode_payload();
        hdr.payload_size = payload.len() as u32;
        let mut out = Vec::with_capacity(HEADER_SIZE + payload.len());
        out.extend_from_slice(&hdr.to_bytes());
        out.extend_from_slice(&payload);
        out
    }

    fn encode_payload(&self) -> Vec<u8> {
        match self {
            Self::Hello(m)          => m.encode(),
            Self::ServerSettings(m) => m.encode(),
            Self::ClientInfo(m)     => m.encode(),
            Self::CodecHeader(m)    => m.encode(),
            Self::WireChunk(m)      => m.encode(),
            Self::Time(m)           => m.encode(),
            Self::Error(m)          => m.encode(),
        }
    }
}
