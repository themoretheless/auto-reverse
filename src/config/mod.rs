//! Configuration split by responsibility: `schema` owns what settings exist,
//! `device_rules` owns physical-device matching/mutation, and `store` owns
//! where settings live (path resolution, TOML I/O, atomic saves). Re-exports keep
//! `crate::config::AppConfig` / `crate::config::ConfigStore` as the public
//! paths so callers don't care about the internal split.

mod device_rules;
mod profiles;
mod schema;
mod store;
mod transfer;

pub use device_rules::{
    matching_device_rule, preferred_device_rule, with_device_alias, with_device_rule_selection,
};
pub use profiles::{ProfileSource, ResolvedDeviceProfile, ResolvedProfileValue};
pub use schema::{AppConfig, CONFIG_VERSION, DeviceRule};
pub use store::{ConfigRevision, ConfigSnapshot, ConfigStore};
pub use transfer::{
    ConfigImportPreview, ConfigSection, MAX_IMPORT_BYTES, MigrationReport, SectionChange,
    TransferError, export_document, preview_import_document, preview_import_file,
};
