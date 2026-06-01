use std::env;
use std::fs;
use std::path::PathBuf;
use std::process;

use core::{IpaStream, StreamAnalysisResult};
use schemars::schema_for;

fn main() {
    if let Err(err) = run() {
        eprintln!("Error: {err}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let out_dir = output_dir();
    fs::create_dir_all(&out_dir)
        .map_err(|e| format!("failed to create output directory '{}': {e}", out_dir.display()))?;

    let request_schema = schema_for!(IpaStream);
    let response_schema = schema_for!(StreamAnalysisResult);

    let request_path = out_dir.join("ipa_stream.request.schema.json");
    let response_path = out_dir.join("stream_analysis.response.schema.json");

    let request_json = serde_json::to_string_pretty(&request_schema)
        .map_err(|e| format!("failed to serialize request schema: {e}"))?;
    let response_json = serde_json::to_string_pretty(&response_schema)
        .map_err(|e| format!("failed to serialize response schema: {e}"))?;

    fs::write(&request_path, request_json)
        .map_err(|e| format!("failed to write '{}': {e}", request_path.display()))?;
    fs::write(&response_path, response_json)
        .map_err(|e| format!("failed to write '{}': {e}", response_path.display()))?;

    println!("Wrote {}", request_path.display());
    println!("Wrote {}", response_path.display());

    Ok(())
}

fn output_dir() -> PathBuf {
    if let Some(path) = env::args().nth(1) {
        return PathBuf::from(path);
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.join("schemas"))
        .unwrap_or_else(|| PathBuf::from("schemas"))
}
