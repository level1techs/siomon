//! Embedded lookup databases.
//!
//! Static reference data compiled into the binary: CPU codenames, per-board
//! sensor label/template mappings, MCE error descriptions, and voltage scaling
//! tables. Board templates are organized by vendor under `boards/`.

pub mod boards;
pub mod cpu_codenames;
pub mod mce;
pub mod sensor_labels;
pub mod voltage_scaling;
