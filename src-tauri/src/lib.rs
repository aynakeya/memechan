pub mod app;
#[cfg(feature = "desktop")]
mod clipboard;
#[cfg(any(feature = "desktop", test))]
mod copy_cache;
#[cfg(feature = "desktop")]
pub mod desktop;
mod import;
pub mod intelligence;
pub mod library;
pub mod media;
pub mod vision;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[cfg(test)]
mod test_support;
