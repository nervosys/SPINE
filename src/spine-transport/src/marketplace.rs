//! # Transport Plugin Marketplace
//!
//! Decentralized registry for discovering, versioning, and managing transport plugins.
//!
//! - **PluginDescriptor**: Name, version (semver), author, capabilities
//! - **PluginRegistry**: Thread-safe registration, search, versioned retrieval
//! - **Compatibility checks**: Minimum API version enforcement

use crate::plugin::TransportPlugin;
use crate::TransportResult;
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, RwLock};

// =============================================================================
// PLUGIN DESCRIPTOR
// =============================================================================

/// Semantic version for plugin compatibility.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SemVer {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl SemVer {
    pub fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() != 3 {
            return None;
        }
        Some(Self {
            major: parts[0].parse().ok()?,
            minor: parts[1].parse().ok()?,
            patch: parts[2].parse().ok()?,
        })
    }

    /// Returns true if `self` is compatible with `required` (same major, >= minor.patch).
    pub fn compatible_with(&self, required: &SemVer) -> bool {
        self.major == required.major
            && (self.minor > required.minor
                || (self.minor == required.minor && self.patch >= required.patch))
    }
}

impl fmt::Display for SemVer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Metadata describing a registered plugin.
#[derive(Debug, Clone)]
pub struct PluginDescriptor {
    pub name: String,
    pub version: SemVer,
    pub author: String,
    pub description: String,
    pub capabilities: Vec<String>,
    /// Minimum transport API version this plugin requires.
    pub min_api_version: SemVer,
}

impl PluginDescriptor {
    pub fn new(name: impl Into<String>, version: SemVer) -> Self {
        Self {
            name: name.into(),
            version,
            author: String::new(),
            description: String::new(),
            capabilities: Vec::new(),
            min_api_version: SemVer::new(1, 0, 0),
        }
    }

    pub fn author(mut self, author: impl Into<String>) -> Self {
        self.author = author.into();
        self
    }

    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    pub fn capability(mut self, cap: impl Into<String>) -> Self {
        self.capabilities.push(cap.into());
        self
    }

    pub fn min_api(mut self, v: SemVer) -> Self {
        self.min_api_version = v;
        self
    }
}

// =============================================================================
// REGISTRY ENTRY
// =============================================================================

struct RegistryEntry {
    descriptor: PluginDescriptor,
    factory: Box<dyn Fn() -> Box<dyn TransportPlugin> + Send + Sync>,
}

impl fmt::Debug for RegistryEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RegistryEntry")
            .field("descriptor", &self.descriptor)
            .finish()
    }
}

// =============================================================================
// PLUGIN REGISTRY
// =============================================================================

/// Current transport plugin API version.
pub const TRANSPORT_API_VERSION: SemVer = SemVer {
    major: 1,
    minor: 0,
    patch: 0,
};

/// Thread-safe plugin registry with versioned retrieval and search.
#[derive(Debug)]
pub struct PluginRegistry {
    /// name → (version → entry)
    entries: RwLock<HashMap<String, Vec<RegistryEntry>>>,
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
        }
    }

    /// Register a plugin with its descriptor and factory function.
    pub fn register<F>(&self, descriptor: PluginDescriptor, factory: F) -> TransportResult<()>
    where
        F: Fn() -> Box<dyn TransportPlugin> + Send + Sync + 'static,
    {
        // Check API compatibility
        if !TRANSPORT_API_VERSION.compatible_with(&descriptor.min_api_version) {
            return Err(crate::TransportError::Protocol(format!(
                "Plugin '{}' requires API {}, current is {}",
                descriptor.name, descriptor.min_api_version, TRANSPORT_API_VERSION
            )));
        }

        let name = descriptor.name.clone();
        let entry = RegistryEntry {
            descriptor,
            factory: Box::new(factory),
        };

        let mut entries = self.entries.write().unwrap();
        let versions = entries.entry(name).or_default();

        // Avoid duplicate versions
        if versions
            .iter()
            .any(|e| e.descriptor.version == entry.descriptor.version)
        {
            return Err(crate::TransportError::Protocol(
                "Plugin version already registered".to_string(),
            ));
        }

        versions.push(entry);
        // Sort by version descending (latest first)
        versions.sort_by(|a, b| b.descriptor.version.cmp(&a.descriptor.version));
        Ok(())
    }

    /// Get the latest version of a plugin.
    pub fn get_latest(&self, name: &str) -> Option<Box<dyn TransportPlugin>> {
        let entries = self.entries.read().unwrap();
        entries
            .get(name)
            .and_then(|versions| versions.first())
            .map(|entry| (entry.factory)())
    }

    /// Get a specific version of a plugin.
    pub fn get_version(
        &self,
        name: &str,
        version: &SemVer,
    ) -> Option<Box<dyn TransportPlugin>> {
        let entries = self.entries.read().unwrap();
        entries.get(name).and_then(|versions| {
            versions
                .iter()
                .find(|e| &e.descriptor.version == version)
                .map(|e| (e.factory)())
        })
    }

    /// List all registered plugins and their versions.
    pub fn list(&self) -> Vec<PluginDescriptor> {
        let entries = self.entries.read().unwrap();
        entries
            .values()
            .flat_map(|versions| versions.iter().map(|e| e.descriptor.clone()))
            .collect()
    }

    /// Search for plugins by capability.
    pub fn search_by_capability(&self, capability: &str) -> Vec<PluginDescriptor> {
        let entries = self.entries.read().unwrap();
        entries
            .values()
            .flat_map(|versions| versions.iter())
            .filter(|e| e.descriptor.capabilities.iter().any(|c| c == capability))
            .map(|e| e.descriptor.clone())
            .collect()
    }

    /// Search for plugins by name substring (case-insensitive).
    pub fn search_by_name(&self, query: &str) -> Vec<PluginDescriptor> {
        let query_lower = query.to_lowercase();
        let entries = self.entries.read().unwrap();
        entries
            .values()
            .flat_map(|versions| versions.iter())
            .filter(|e| e.descriptor.name.to_lowercase().contains(&query_lower))
            .map(|e| e.descriptor.clone())
            .collect()
    }

    /// Number of distinct plugin names registered.
    pub fn plugin_count(&self) -> usize {
        self.entries.read().unwrap().len()
    }

    /// Total number of plugin versions across all names.
    pub fn version_count(&self) -> usize {
        self.entries
            .read()
            .unwrap()
            .values()
            .map(|v| v.len())
            .sum()
    }

    /// Remove a plugin by name (all versions).
    pub fn unregister(&self, name: &str) -> bool {
        self.entries.write().unwrap().remove(name).is_some()
    }
}

/// Create a shared registry wrapped in Arc for multi-threaded use.
pub fn shared_registry() -> Arc<PluginRegistry> {
    Arc::new(PluginRegistry::new())
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::{LoggingPlugin, MetricsPlugin, RateLimiterPlugin};

    #[test]
    fn test_semver_parse() {
        let v = SemVer::parse("1.2.3").unwrap();
        assert_eq!(v, SemVer::new(1, 2, 3));
        assert!(SemVer::parse("bad").is_none());
        assert!(SemVer::parse("1.2").is_none());
    }

    #[test]
    fn test_semver_compatible() {
        let v120 = SemVer::new(1, 2, 0);
        let v100 = SemVer::new(1, 0, 0);
        let v200 = SemVer::new(2, 0, 0);
        assert!(v120.compatible_with(&v100));
        assert!(!v100.compatible_with(&v120));
        assert!(!v200.compatible_with(&v100));
    }

    #[test]
    fn test_semver_display() {
        assert_eq!(SemVer::new(1, 2, 3).to_string(), "1.2.3");
    }

    #[test]
    fn test_register_and_get_latest() {
        let reg = PluginRegistry::new();
        let desc = PluginDescriptor::new("metrics", SemVer::new(1, 0, 0))
            .author("SPINE Team")
            .description("Frame-level metrics")
            .capability("metrics");

        reg.register(desc, || Box::new(MetricsPlugin::new()))
            .unwrap();

        let plugin = reg.get_latest("metrics");
        assert!(plugin.is_some());
        assert_eq!(plugin.unwrap().name(), "metrics");
    }

    #[test]
    fn test_register_multiple_versions() {
        let reg = PluginRegistry::new();

        reg.register(
            PluginDescriptor::new("logger", SemVer::new(1, 0, 0)),
            || Box::new(LoggingPlugin::summary()),
        )
        .unwrap();

        reg.register(
            PluginDescriptor::new("logger", SemVer::new(1, 1, 0)),
            || Box::new(LoggingPlugin::summary()),
        )
        .unwrap();

        assert_eq!(reg.version_count(), 2);
        assert_eq!(reg.plugin_count(), 1);

        // Latest should be 1.1.0
        let latest = reg.get_latest("logger").unwrap();
        assert_eq!(latest.name(), "logging");
    }

    #[test]
    fn test_get_specific_version() {
        let reg = PluginRegistry::new();
        reg.register(
            PluginDescriptor::new("limiter", SemVer::new(1, 0, 0)),
            || Box::new(RateLimiterPlugin::new(100.0, 100)),
        )
        .unwrap();

        let v = SemVer::new(1, 0, 0);
        assert!(reg.get_version("limiter", &v).is_some());

        let v2 = SemVer::new(2, 0, 0);
        assert!(reg.get_version("limiter", &v2).is_none());
    }

    #[test]
    fn test_duplicate_version_rejected() {
        let reg = PluginRegistry::new();
        reg.register(
            PluginDescriptor::new("x", SemVer::new(1, 0, 0)),
            || Box::new(MetricsPlugin::new()),
        )
        .unwrap();

        let result = reg.register(
            PluginDescriptor::new("x", SemVer::new(1, 0, 0)),
            || Box::new(MetricsPlugin::new()),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_search_by_capability() {
        let reg = PluginRegistry::new();
        reg.register(
            PluginDescriptor::new("metrics", SemVer::new(1, 0, 0))
                .capability("observability"),
            || Box::new(MetricsPlugin::new()),
        )
        .unwrap();
        reg.register(
            PluginDescriptor::new("logger", SemVer::new(1, 0, 0))
                .capability("observability")
                .capability("debugging"),
            || Box::new(LoggingPlugin::summary()),
        )
        .unwrap();

        let obs = reg.search_by_capability("observability");
        assert_eq!(obs.len(), 2);

        let dbg = reg.search_by_capability("debugging");
        assert_eq!(dbg.len(), 1);
        assert_eq!(dbg[0].name, "logger");
    }

    #[test]
    fn test_search_by_name() {
        let reg = PluginRegistry::new();
        reg.register(
            PluginDescriptor::new("http-compressor", SemVer::new(1, 0, 0)),
            || Box::new(MetricsPlugin::new()),
        )
        .unwrap();
        reg.register(
            PluginDescriptor::new("tcp-logger", SemVer::new(1, 0, 0)),
            || Box::new(LoggingPlugin::summary()),
        )
        .unwrap();

        let results = reg.search_by_name("compress");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "http-compressor");
    }

    #[test]
    fn test_list_all() {
        let reg = PluginRegistry::new();
        reg.register(
            PluginDescriptor::new("a", SemVer::new(1, 0, 0)),
            || Box::new(MetricsPlugin::new()),
        )
        .unwrap();
        reg.register(
            PluginDescriptor::new("b", SemVer::new(1, 0, 0)),
            || Box::new(LoggingPlugin::summary()),
        )
        .unwrap();

        let all = reg.list();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_unregister() {
        let reg = PluginRegistry::new();
        reg.register(
            PluginDescriptor::new("temp", SemVer::new(1, 0, 0)),
            || Box::new(MetricsPlugin::new()),
        )
        .unwrap();
        assert_eq!(reg.plugin_count(), 1);
        assert!(reg.unregister("temp"));
        assert_eq!(reg.plugin_count(), 0);
        assert!(!reg.unregister("temp"));
    }

    #[test]
    fn test_shared_registry() {
        let reg = shared_registry();
        reg.register(
            PluginDescriptor::new("shared", SemVer::new(1, 0, 0)),
            || Box::new(MetricsPlugin::new()),
        )
        .unwrap();
        let reg2 = reg.clone();
        assert_eq!(reg2.plugin_count(), 1);
    }

    #[test]
    fn test_not_found_returns_none() {
        let reg = PluginRegistry::new();
        assert!(reg.get_latest("nonexistent").is_none());
    }

    #[test]
    fn test_descriptor_builder() {
        let desc = PluginDescriptor::new("test", SemVer::new(1, 0, 0))
            .author("Test Author")
            .description("A test plugin")
            .capability("cap1")
            .capability("cap2")
            .min_api(SemVer::new(1, 0, 0));

        assert_eq!(desc.name, "test");
        assert_eq!(desc.author, "Test Author");
        assert_eq!(desc.capabilities.len(), 2);
    }
}
