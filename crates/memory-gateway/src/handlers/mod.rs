//! HTTP handlers: translate requests into use-case calls and results into
//! envelopes. No business rules live here.

pub mod memory;
pub mod meta;
pub mod ops;
pub mod skill;
