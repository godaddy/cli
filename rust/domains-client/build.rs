//! Generates the typed Domains API client from the vendored OpenAPI 3.0 spec.
//!
//! The spec (`openapi/domains.oas3.json`) is committed and trimmed to the
//! availability + suggest operations (see `scripts/regenerate-spec.sh`); this
//! build step is hermetic and never touches the network.

use std::{env, fs, path::Path};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let spec_path = "openapi/domains.oas3.json";
    println!("cargo:rerun-if-changed={spec_path}");
    println!("cargo:rerun-if-changed=build.rs");

    let spec_text = fs::read_to_string(spec_path)?;
    let spec: openapiv3::OpenAPI = serde_json::from_str(&spec_text)?;

    let mut generator = progenitor::Generator::default();
    let tokens = generator.generate_tokens(&spec)?;
    let ast = syn::parse2(tokens)?;
    let formatted = prettyplease::unparse(&ast);

    let out_dir = env::var("OUT_DIR")?;
    fs::write(Path::new(&out_dir).join("codegen.rs"), formatted)?;
    Ok(())
}
