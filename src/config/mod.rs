//! Configuration split by responsibility: `schema` owns what the settings
//! ARE (fields, defaults, validation, per-device policy); `store` owns where
//! they LIVE (path resolution, TOML I/O, atomic saves). The re-exports keep
//! `crate::config::AppConfig` / `crate::config::ConfigStore` as the public
//! paths so callers don't care about the internal split.

mod schema;
mod store;

pub use schema::{AppConfig, CONFIG_VERSION, DeviceRule};
pub use store::ConfigStore;
