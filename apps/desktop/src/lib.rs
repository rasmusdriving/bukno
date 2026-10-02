//! The Bukno desktop app. The binary in `main.rs` is the composition root;
//! the library exists so UI checks can drive the same app.

pub mod app;
pub mod components;
pub mod evidence;
pub mod navigation;
pub mod screens;
pub mod theme;
pub mod transcript;

pub use app::BuknoApp;
