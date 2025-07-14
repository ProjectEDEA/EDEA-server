fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure()
        .build_server(true)
        .build_client(false)
        .file_descriptor_set_path("proto/class_descriptor.bin")
        .compile_protos(&["proto/class.proto"], &["proto"])?;
    
    println!("cargo:rerun-if-changed=proto/class.proto");
    
    Ok(())
}
