//! Tauri commands — thin IPC shells over pure `processing/` logic.
//! One module per domain as the backend grows: hash.rs, image.rs, rename.rs, pdf.rs.

pub mod hash;
pub mod image;
pub mod rename;
