//! Control-frame serde types. These now live in the shared `skald-relay-common`
//! crate so the relay and the mobile-connector plugin stay byte-identical on the
//! wire (see plugin.md §1.1). Re-exported here so existing relay paths
//! (`crate::types::…`) keep working unchanged.

pub use skald_relay_common::frames::*;
