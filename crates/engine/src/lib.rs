//! Sans-IO engine for Spool.
//!
//! The engine never touches sockets or files. Callers move ciphertext between
//! a transport and [`conn::Conn`], and stream file bytes through the PAR2
//! verifier and repairer. The same code runs natively in tests and in the
//! browser as WebAssembly.

pub mod conn;
pub mod gf16;
pub mod nntp;
pub mod nzb;
pub mod par2;
pub mod tls;
pub mod yenc;

#[cfg(target_arch = "wasm32")]
mod wasm;

pub use conn::{Conn, ConnError};
pub use nntp::{Event, Segment};
