//! portwho: a live "who owns this port?" table for macOS dev machines.
//!
//! The binary is a thin shell over these modules; they're public so the
//! integration tests in `tests/` can drive them against real processes.

pub mod app;
pub mod identity;
pub mod kill;
pub mod mem;
pub mod ports;
pub mod ui;
pub mod view;
