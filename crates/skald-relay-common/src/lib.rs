//! Shared building blocks for the Skald Remote Control relay and the
//! mobile-connector plugin (see data/ios-app/plugin.md §1.1).
//!
//! - [`frames`]: serde control-frame types (the JSON wire protocol).
//! - [`crypto`]: domain constants, namespace derivation, challenge sign/verify,
//!   X25519 ECDH, HKDF, AES-256-GCM seal/open, and nonce/AAD construction.
//!
//! This crate has **no** dependency on Skald, axum or tokio: both the relay and
//! the plugin link it so they can never diverge from the protocol or from the
//! interop vectors in test-vectors.md.

pub mod crypto;
pub mod frames;
