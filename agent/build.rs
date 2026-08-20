fn main() {
    println!("cargo:rerun-if-changed=proto/agent.proto");
    let protoc_path =
        protoc_bin_vendored::protoc_bin_path().expect("failed to locate bundled protoc binary");
    std::env::set_var("PROTOC", protoc_path);

    tonic_build::configure()
        .build_server(true)
        .compile_protos(&["proto/agent.proto"], &["proto"])
        .expect("failed to compile gRPC definitions");
}
