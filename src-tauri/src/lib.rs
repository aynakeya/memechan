#[cfg(feature = "desktop")]
pub mod desktop;
pub mod intelligence;
pub mod library;
pub mod media;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
