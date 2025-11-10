fn main() {
    println!("cargo:rerun-if-changed=proto/agent.proto");
    tonic_build::configure()
        .build_server(true)
        .compile(&["proto/agent.proto"], &["proto"])
        .expect("failed to compile gRPC definitions");
}
