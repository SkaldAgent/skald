// Build script for skald-relay-common.
//
// Compiles the v2 relay protocol schema (proto/skald/relay/v2/relay_frame.proto)
// into Rust types using prost-build. The generated module is named after the
// proto package (`skald.relay.v2`) and lands in OUT_DIR; `src/proto.rs`
// `include!`s it under a `v2` submodule.
//
// We intentionally use `Config::new()` (no `extern_path`, no `include_file`)
// so the generated code is self-contained — `proto.rs` is the single
// integration point and downstream crates depend on `skald_relay_common::proto`
// without having to plumb any extra paths through their own `build.rs`.

fn main() -> std::io::Result<()> {
    let mut config = prost_build::Config::new();
    // Default for `bytes` fields is `Vec<u8>`, but state it explicitly so a
    // future prost default change can't silently flip the wire encoding.
    config.bytes(["."]);

    config.compile_protos(
        &["proto/skald/relay/v2/relay_frame.proto"],
        &["proto"],
    )?;

    // Rerun if the schema (or this build script) changes.
    println!("cargo:rerun-if-changed=proto/skald/relay/v2/relay_frame.proto");
    println!("cargo:rerun-if-changed=build.rs");

    Ok(())
}
