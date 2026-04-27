//! # sonium-common
//!
//! Shared types used across all Sonium crates.
//!
//! - [`SampleFormat`] — describes a PCM stream (sample rate, bit depth, channel count).
//! - [`SoniumError`] / [`error::Result`] — unified error type.
//! - [`config`] — zero-configuration server and client config structs backed by TOML.

pub mod config;
pub mod error;
pub mod sample_format;

pub use error::SoniumError;
pub use sample_format::SampleFormat;
