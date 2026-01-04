//! 插件系统模块 - 部分功能尚在开发中

pub mod manager;
pub mod package;
pub mod registry;
pub mod runtime;
pub mod types;
pub mod verifier;

pub use manager::{PluginManager, PluginOutputWithSource};
pub use package::PluginPackage;
pub use registry::PluginRegistry;
pub use types::*;
pub use verifier::PluginVerifier;
