//! Low-level helpers for reading/writing little-endian values from byte slices.
//! Used by all message (de)serializers.

use sonium_common::SoniumError;

pub type Result<T> = sonium_common::error::Result<T>;

pub struct WireRead<'a> {
    data: &'a [u8],
    pos:  usize,
}

impl<'a> WireRead<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    pub fn read_u16(&mut self) -> Result<u16> {
        self.read_bytes::<2>().map(u16::from_le_bytes)
    }

    pub fn read_u32(&mut self) -> Result<u32> {
        self.read_bytes::<4>().map(u32::from_le_bytes)
    }

    pub fn read_i32(&mut self) -> Result<i32> {
        self.read_bytes::<4>().map(i32::from_le_bytes)
    }

    pub fn read_str(&mut self) -> Result<String> {
        let len = self.read_u32()? as usize;
        let bytes = self.read_slice(len)?;
        String::from_utf8(bytes.to_vec())
            .map_err(|e| SoniumError::Protocol(format!("invalid UTF-8: {e}")))
    }

    pub fn read_blob(&mut self) -> Result<Vec<u8>> {
        let len = self.read_u32()? as usize;
        Ok(self.read_slice(len)?.to_vec())
    }

    pub fn read_slice(&mut self, len: usize) -> Result<&'a [u8]> {
        if self.pos + len > self.data.len() {
            return Err(SoniumError::Protocol(format!(
                "read overflow: need {len}, have {}",
                self.remaining()
            )));
        }
        let s = &self.data[self.pos..self.pos + len];
        self.pos += len;
        Ok(s)
    }

    fn read_bytes<const N: usize>(&mut self) -> Result<[u8; N]> {
        let s = self.read_slice(N)?;
        Ok(s.try_into().unwrap())
    }
}

pub struct WireWrite {
    buf: Vec<u8>,
}

impl WireWrite {
    pub fn new() -> Self {
        Self { buf: Vec::new() }
    }

    pub fn with_capacity(cap: usize) -> Self {
        Self { buf: Vec::with_capacity(cap) }
    }

    pub fn write_u16(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn write_u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn write_i32(&mut self, v: i32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn write_str(&mut self, s: &str) {
        self.write_u32(s.len() as u32);
        self.buf.extend_from_slice(s.as_bytes());
    }

    pub fn write_blob(&mut self, b: &[u8]) {
        self.write_u32(b.len() as u32);
        self.buf.extend_from_slice(b);
    }

    pub fn write_raw(&mut self, b: &[u8]) {
        self.buf.extend_from_slice(b);
    }

    pub fn finish(self) -> Vec<u8> {
        self.buf
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }
}

impl Default for WireWrite {
    fn default() -> Self {
        Self::new()
    }
}
