//! The relay's **only** cryptographic operations: verifying the Ed25519
//! challenge-response signature (crypto.md §8) and deriving the `namespace_id`
//! (crypto.md §7). The relay never touches X25519/AEAD — those are end-to-end
//! between agent and client.

use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

/// Challenge-response signing domain (crypto.md §1, `AUTH_DOMAIN`).
pub const AUTH_DOMAIN: &[u8] = b"skald-relay-auth-v1";
/// Namespace derivation domain (crypto.md §1, `NS_DOMAIN`).
pub const NS_DOMAIN: &[u8] = b"skald-namespace-v1";

/// Decode a hex pubkey/signature/id (case-insensitive on input) into `N` bytes.
/// Returns `None` on malformed hex or wrong length.
pub fn decode_hex<const N: usize>(s: &str) -> Option<[u8; N]> {
    let bytes = hex::decode(s).ok()?;
    if bytes.len() != N {
        return None;
    }
    let mut out = [0u8; N];
    out.copy_from_slice(&bytes);
    Some(out)
}

/// `namespace_id = hex(SHA256(NS_DOMAIN ‖ 0x00 ‖ agent_ed25519_pub))` (crypto.md §7).
/// Returns both the raw 32 bytes and the lowercase hex string.
pub fn namespace_id(agent_ed25519_pub: &[u8; 32]) -> ([u8; 32], String) {
    let mut h = Sha256::new();
    h.update(NS_DOMAIN);
    h.update([0u8]);
    h.update(agent_ed25519_pub);
    let raw: [u8; 32] = h.finalize().into();
    let hexed = hex::encode(raw);
    (raw, hexed)
}

/// Verify the Ed25519 challenge signature: `signature` over
/// `AUTH_DOMAIN ‖ 0x00 ‖ challenge_nonce_raw(32B)` under `ed25519_pub`.
///
/// Uses `verify_strict` (rejects malleable signatures / low-order keys).
pub fn verify_challenge(
    ed25519_pub: &[u8; 32],
    challenge_nonce_raw: &[u8; 32],
    signature: &[u8; 64],
) -> bool {
    let Ok(vk) = VerifyingKey::from_bytes(ed25519_pub) else {
        return false;
    };
    let sig = Signature::from_bytes(signature);

    let mut msg = Vec::with_capacity(AUTH_DOMAIN.len() + 1 + 32);
    msg.extend_from_slice(AUTH_DOMAIN);
    msg.push(0x00);
    msg.extend_from_slice(challenge_nonce_raw);

    vk.verify_strict(&msg, &sig).is_ok()
}

/// Constant-time comparison of two tokens/secrets (relay.md §6: pairing_token).
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.ct_eq(b).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    #[test]
    fn decode_hex_rejects_wrong_len() {
        assert!(decode_hex::<32>("00").is_none());
        assert!(decode_hex::<32>("zz".repeat(32).as_str()).is_none());
        assert_eq!(decode_hex::<2>("ABcd"), Some([0xAB, 0xCD])); // accepts uppercase
    }

    #[test]
    fn challenge_roundtrip() {
        let sk = SigningKey::from_bytes(&[7u8; 32]);
        let pk = sk.verifying_key().to_bytes();
        let nonce = [0x42u8; 32];

        let mut msg = Vec::new();
        msg.extend_from_slice(AUTH_DOMAIN);
        msg.push(0x00);
        msg.extend_from_slice(&nonce);
        let sig = sk.sign(&msg).to_bytes();

        assert!(verify_challenge(&pk, &nonce, &sig));

        // Different nonce → invalid signature.
        assert!(!verify_challenge(&pk, &[0x43u8; 32], &sig));
        // Different key → invalid.
        let other = SigningKey::from_bytes(&[8u8; 32])
            .verifying_key()
            .to_bytes();
        assert!(!verify_challenge(&other, &nonce, &sig));
    }

    #[test]
    fn namespace_id_is_hex_of_sha256() {
        let pk = [0u8; 32];
        let (raw, hexed) = namespace_id(&pk);
        assert_eq!(hexed.len(), 64);
        assert_eq!(hex::encode(raw), hexed);
    }

    /// Cross-compat: the iOS client uses `CryptoKit` to sign the auth challenge,
    /// the relay uses `ed25519-dalek` to verify.  The two libraries produce
    /// *different* (but both valid) Ed25519 signatures for the same (key, message)
    /// — see `data/ios-app/test-vectors.md` §4 "Note on V18".  This test pins
    /// the two signatures the relay must accept for the canonical
    /// `SEED_CLIENT = bytes(32..<64)` test vector:
    ///
    ///  * `V18_cryptokit` (the value committed in test-vectors.md §4) — what
    ///    the iOS test suite asserts byte-for-byte.
    ///  * `V18_dalek` (regression value) — what the Rust `gen-vectors` binary
    ///    produces.  The relay MUST keep accepting it as well: any signature
    ///    that was once valid under RFC 8032 strict Ed25519 stays valid
    ///    (the protocol invariant is "the relay verifies the client's
    ///    signature", not "the client and relay produce identical bytes").
    #[test]
    fn challenge_verifies_cryptokit_signature() {
        // V8 client_ed25519_pub from test-vectors.md §4.
        let client_pub: [u8; 32] =
            hex::decode("12355ea750e60d6370ba6776037f25062f6c9450c5009669884895fd5b377a18")
                .unwrap()
                .try_into()
                .unwrap();
        let challenge: [u8; 32] =
            hex::decode("aabbccddeeff00112233445566778899aabbccddeeff00112233445566778899")
                .unwrap()
                .try_into()
                .unwrap();
        // V18 from test-vectors.md §4 (CryptoKit signature).
        let v18_cryptokit: [u8; 64] =
            hex::decode("a2af4518a7001e3269006d30e1175d33d36cc23350c6c4def8347be8ec97e32ce51c4f066ca29cc497690aa241524d20ea20a72d38d9beb6da01e966aada8508")
                .unwrap()
                .try_into()
                .unwrap();
        // The dalek reference value (regression — see "Note on V18" in §4).
        let v18_dalek: [u8; 64] =
            hex::decode("ae38491a1f25bb5fb11f0b17e3d344412bfc927461b6517e9a0ab6a64020054677f59490af026f34c81d9378d4daae4823109ca2d1afbf4ff00230a038270002")
                .unwrap()
                .try_into()
                .unwrap();

        assert!(verify_challenge(&client_pub, &challenge, &v18_cryptokit),
                "relay must accept the CryptoKit signature committed as V18");
        assert!(verify_challenge(&client_pub, &challenge, &v18_dalek),
                "relay must also accept the historical dalek reference signature");

        // Sanity: tampered signature is rejected.
        let mut bad = v18_cryptokit;
        bad[0] ^= 0x01;
        assert!(!verify_challenge(&client_pub, &challenge, &bad));
    }
}
