//! Compile the SPINE agentic `.proto` into Rust (client + server stubs) via
//! `tonic-build`. Requires `protoc` on PATH.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR")?);
    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        // Emit a file-descriptor set so the server can offer gRPC reflection
        // (grpcurl and other tooling introspect the service with no .proto).
        .file_descriptor_set_path(out_dir.join("spine_agentic_descriptor.bin"))
        .compile_protos(&["proto/spine_agentic.proto"], &["proto"])?;
    println!("cargo:rerun-if-changed=proto/spine_agentic.proto");
    Ok(())
}
