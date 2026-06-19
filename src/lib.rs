//! lessmd — a less-like terminal pager that renders markdown and mermaid.
//!
//! All pager logic lives in pure modules here (no terminal I/O); `main.rs`
//! is the only place that drives crossterm/ratatui.

pub mod cli;
pub mod document;
pub mod help;
pub mod input;
pub mod pager;
pub mod render;
pub mod search;
pub mod source;
