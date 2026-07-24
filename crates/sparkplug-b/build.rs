//! Compile the checked-in Sparkplug B `.proto` into Rust types via `prost`.
//!
//! Requires `protoc` on PATH (verified present in dev; CI and the Docker build image
//! must provide it too — see Story 0.4 CI and Epic 7 Dockerfile).

fn main() {
    println!("cargo:rerun-if-changed=proto/sparkplug_b.proto");
    prost_build::compile_protos(&["proto/sparkplug_b.proto"], &["proto/"])
        .expect("failed to compile proto/sparkplug_b.proto with prost");
}
