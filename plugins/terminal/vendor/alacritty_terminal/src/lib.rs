//! Alacritty terminal emulation modules adapted for a capability-based WASM guest.
//! Native PTY, polling and thread modules are intentionally supplied by the host API.
pub mod event;
pub mod grid;
pub mod index;
pub mod selection;
pub mod term;
pub mod vi_mode;
pub use grid::Grid;
pub use term::Term;
pub use vte;
