//! Core library for Fotogramma: no UI and no Qt dependency.
//!
//! The rules mirror the Python application exactly:
//! only the first video stream (`v:0`) of `.mp4`, `.mkv` and `.webm` is read,
//! `hevc` and `av1` are compatible, anything else is convertible and read
//! failures are errors. Originals are replaced only after codec, duration,
//! stream, full-decode and backup validation.

pub mod conversion;
pub mod formatting;
pub mod i18n;
pub mod media_tools;
pub mod scanner;
pub mod update;
pub mod verification;

pub use scanner::{Category, ProbeError, Summary, VideoInfo};
