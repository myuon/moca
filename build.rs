use std::env;
use std::path::PathBuf;

fn main() {
    // Generate C header using cbindgen
    let crate_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let output_path = PathBuf::from(&crate_dir).join("include").join("mica.h");

    // Create include directory if it doesn't exist
    std::fs::create_dir_all(PathBuf::from(&crate_dir).join("include")).ok();

    // Generate the header
    let config = cbindgen::Config::from_file("cbindgen.toml")
        .unwrap_or_else(|_| cbindgen::Config::default());

    if let Err(e) = cbindgen::Builder::new()
        .with_crate(&crate_dir)
        .with_config(config)
        .generate()
        .map(|bindings| bindings.write_to_file(&output_path))
    {
        eprintln!("Warning: Failed to generate C header: {}", e);
    }
}
