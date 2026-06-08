//! Compile the SPINE agentic `.proto` into Rust (client + server stubs) via
//! `tonic-build`. Requires `protoc` on PATH.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(&["proto/spine_agentic.proto"], &["proto"])?;
    println!("cargo:rerun-if-changed=proto/spine_agentic.proto");
    Ok(())
}
