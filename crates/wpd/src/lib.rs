//! Windows Portable Devices (WPD) access to the iPhone's DCIM folder. Windows only.
//!
//! Kept out of `photoxfer-core` so it type-checks for Windows without a C cross-compiler
//! (core bundles SQLite).
#[cfg(windows)]
pub mod wpd;
