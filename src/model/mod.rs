//! Shared data structures for hardware and sensor information.
//!
//! Serde-serializable types used by collectors, sensors, and output formatters.
//! [`system::SystemInfo`] is the top-level container populated during collection.

pub mod audio;
pub mod battery;
pub mod cpu;
pub mod gpu;
pub mod memory;
pub mod motherboard;
pub mod network;
pub mod pci;
pub mod sensor;
pub mod storage;
pub mod system;
pub mod usb;
