use std::env;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure()
        .build_server(true)
        .build_client(false)
        .file_descriptor_set_path(
            PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR not set"))
                .join("class_descriptor.bin"),
        )
        .compile_protos(&["proto/class.proto"], &["proto"])
        .unwrap();
    Ok(())
}
