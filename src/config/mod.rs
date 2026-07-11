//! Configuration split by responsibility: `schema` owns what settings exist,
//! `device_rules` owns physical-device matching/mutation, and `store` owns
//! where settings live (path resolution, TOML I/O, atomic saves). Re-exports keep
//! `crate::config::AppConfig` / `crate::config::ConfigStore` as the public
//! paths so callers don't care about the internal split.

mod device_rules;
mod schema;
mod store;

pub use device_rules::{matching_device_rule, preferred_device_rule, with_device_rule_selection};
pub use schema::{AppConfig, CONFIG_VERSION, DeviceRule};
pub use store::{ConfigRevision, ConfigSnapshot, ConfigStore};
