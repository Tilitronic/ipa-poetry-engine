use std::env;
use std::fs;
use std::process;

use core::{analyze_stream, FeatureRegistry, IpaStream};

fn main() {
    let mut phonemes_path: Option<String> = None;
    let mut stream_path: Option<String> = None;
    let mut pretty = false;

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--phonemes" => {
                phonemes_path = args.next();
            }
            "--stream" => {
                stream_path = args.next();
            }
            "--pretty" => {
                pretty = true;
            }
            "-h" | "--help" => {
                print_help();
                return;
            }
            _ => {
                eprintln!("Unknown argument: {arg}");
                print_help();
                process::exit(2);
            }
        }
    }

    let Some(stream_path) = stream_path else {
        eprintln!("Missing required argument: --stream <path>");
        print_help();
        process::exit(2);
    };

    if let Err(err) = run(phonemes_path.as_deref(), &stream_path, pretty) {
        eprintln!("Error: {err}");
        process::exit(1);
    }
}

fn run(phonemes_path: Option<&str>, stream_path: &str, pretty: bool) -> Result<(), String> {
    let stream_bytes = fs::read(stream_path)
        .map_err(|e| format!("failed to read stream file '{stream_path}': {e}"))?;

    let registry = if let Some(path) = phonemes_path {
        let phonemes_bytes = fs::read(path)
            .map_err(|e| format!("failed to read phonemes file '{path}': {e}"))?;
        FeatureRegistry::from_json_bytes(&phonemes_bytes)
            .map_err(|e| format!("failed to parse phonemes registry: {e}"))?
    } else {
        FeatureRegistry::build()
    };

    let stream = IpaStream::from_json_bytes(&stream_bytes)
        .map_err(|e| format!("failed to parse stream json: {e}"))?;

    let analysis = analyze_stream(&stream, &registry)
        .map_err(|e| format!("analysis failed: {e}"))?;

    let output = if pretty {
        serde_json::to_string_pretty(&analysis)
            .map_err(|e| format!("failed to serialize result: {e}"))?
    } else {
        serde_json::to_string(&analysis)
            .map_err(|e| format!("failed to serialize result: {e}"))?
    };

    println!("{output}");
    Ok(())
}

fn print_help() {
    eprintln!(
        "Usage: analyze_stream_cli --stream <path> [--phonemes <path>] [--pretty]\n\n\
--stream    Path to IPA Stream v1.1 JSON file\n\
--phonemes  Optional path to phonemes.json (defaults to embedded registry)\n\
--pretty    Pretty-print JSON output\n"
    );
}
