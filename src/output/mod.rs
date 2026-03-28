//! Output formatters and the TUI dashboard.
//!
//! Each submodule renders [`SystemInfo`](crate::model::system::SystemInfo) in a
//! different format. Most are feature-gated; `text` is always available.

#[cfg(feature = "csv")]
pub mod csv;
#[cfg(feature = "html")]
pub mod html;
#[cfg(feature = "json")]
pub mod json;
pub mod text;
#[cfg(feature = "tui")]
pub mod tui;
#[cfg(feature = "xml")]
pub mod xml;
