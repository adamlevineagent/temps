//! Worker-side mirror of the CP's internal-zone route table.
//!
//! Holds the current `*.temps.local` host → backends map purely in
//! memory. The agent is intentionally stateless: routes are populated
//! by [`crate::route_sync_client::RouteSyncClient`] on first sync and
//! kept current via long-poll updates from the control plane.
//!
//! The agent does **not** persist routes to disk. A restarted agent
//! starts with an empty store and serves 404 from the internal proxy
//! until the first sync completes (typically <1s after the agent's
//! first request to the CP). This keeps agents disposable — the CP
//! is the only source of truth, and there's no on-disk state that can
//! drift, get corrupted, or have to be migrated when the wire format
//! evolves.
//!
//! ## Atomic snapshots
//!
//! Replace-the-whole-map semantics. Each apply allocates a new
//! `HashMap`, populates it, then swaps it into the `Arc<RwLock<…>>`.
//! Lookups take a tiny read lock and clone the matched entry; they
//! never block writers in practice (apply happens once per CP
//! generation bump, lookups happen per request).

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tracing::info;

/// One backend reachable for a host. `address` is the dial-as-is form
/// produced on the CP — overlay IP for same-node containers, underlay
/// IP + published port for cross-node, etc. The agent does not parse
/// or rewrite it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteBackend {
    pub address: String,
    pub container_id: Option<String>,
    pub container_name: Option<String>,
}

/// One internal-zone route. `host` is the lower-cased FQDN the proxy
/// matches `Host:` against.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteEntry {
    pub host: String,
    pub backends: Vec<RouteBackend>,
    pub deployment_id: Option<i32>,
    pub project_id: Option<i32>,
    pub environment_id: Option<i32>,
}

pub struct RouteStore {
    inner: RwLock<HashMap<String, RouteEntry>>,
    generation: RwLock<u64>,
}

impl Default for RouteStore {
    fn default() -> Self {
        Self::new()
    }
}

impl RouteStore {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(HashMap::new()),
            generation: RwLock::new(0),
        }
    }

    /// Replace the in-memory store with the given snapshot.
    /// Returns the new generation.
    pub fn apply_snapshot(&self, generation: u64, routes: Vec<RouteEntry>) -> u64 {
        let mut map = HashMap::with_capacity(routes.len());
        for r in &routes {
            map.insert(r.host.to_ascii_lowercase(), r.clone());
        }
        *self.inner.write() = map;
        *self.generation.write() = generation;
        info!(
            generation,
            entries = self.inner.read().len(),
            "applied route snapshot"
        );
        generation
    }

    /// Look up a host. Returns the cloned entry on hit. Case-insensitive.
    pub fn lookup(&self, host: &str) -> Option<RouteEntry> {
        let key = host.to_ascii_lowercase();
        self.inner.read().get(&key).cloned()
    }

    pub fn current_generation(&self) -> u64 {
        *self.generation.read()
    }

    pub fn len(&self) -> usize {
        self.inner.read().len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.read().is_empty()
    }
}

pub type SharedRouteStore = Arc<RouteStore>;

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(host: &str, addr: &str) -> RouteEntry {
        RouteEntry {
            host: host.into(),
            backends: vec![RouteBackend {
                address: addr.into(),
                container_id: None,
                container_name: None,
            }],
            deployment_id: Some(1),
            project_id: Some(1),
            environment_id: Some(1),
        }
    }

    #[test]
    fn apply_and_lookup() {
        let store = RouteStore::new();
        store.apply_snapshot(5, vec![entry("PROD.foo.temps.local", "10.0.0.1:80")]);
        assert_eq!(store.current_generation(), 5);
        assert!(store.lookup("prod.foo.temps.local").is_some());
        // Case-insensitive match.
        assert!(store.lookup("PROD.FOO.TEMPS.LOCAL").is_some());
        assert!(store.lookup("missing.temps.local").is_none());
    }

    #[test]
    fn second_apply_replaces_first() {
        let store = RouteStore::new();
        store.apply_snapshot(1, vec![entry("a.temps.local", "10.0.0.1:80")]);
        store.apply_snapshot(2, vec![entry("b.temps.local", "10.0.0.2:80")]);
        assert_eq!(store.current_generation(), 2);
        assert!(store.lookup("a.temps.local").is_none());
        assert!(store.lookup("b.temps.local").is_some());
    }

    #[test]
    fn empty_store_starts_empty() {
        let store = RouteStore::new();
        assert_eq!(store.current_generation(), 0);
        assert!(store.is_empty());
        assert!(store.lookup("anything.temps.local").is_none());
    }
}
