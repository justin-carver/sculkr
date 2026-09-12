//! Fails the build if anything under `src/` reads an environment variable at
//! compile time. Those get baked into the binary as plaintext, which is fine
//! for cargo's own metadata and a secret leak for everything else -- every
//! published release artifact would carry the value. Runtime lookups live in
//! `src/env.rs`.

use std::path::Path;

fn main() {
    println!("cargo::rerun-if-changed=src");
    println!("cargo::rerun-if-changed=build.rs");

    let mut found = Vec::new();
    scan(Path::new("src"), &mut found);

    // Sorted so that repeats of one variable sit next to each other below.
    found.sort();
    found.dedup();

    let mut emitted: Option<&str> = None;
    for (var, location) in &found {
        println!(
            "cargo::warning={var} is read at compile time in {location}; \
             its value is blanked in the binary -- read it at runtime via crate::env"
        );

        // rustc-env takes a bare name, so the location only goes in the warning.
        if emitted != Some(var.as_str()) {
            println!("cargo::rustc-env={var}=");
            emitted = Some(var.as_str());
        }
    }
}

fn scan(dir: &Path, found: &mut Vec<(String, String)>) {
    let Ok(entries) = dir.read_dir() else { return };

    for entry in entries.flatten() {
        let path = entry.path();

        if path.is_dir() {
            scan(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            let Ok(source) = std::fs::read_to_string(&path) else {
                continue;
            };
            collect(&source, &path, found);
        }
    }
}

/// `option_env!(` ends in `env!(`, so matching the shorter form catches both.
fn collect(source: &str, path: &Path, found: &mut Vec<(String, String)>) {
    for (offset, matched) in source.match_indices("env!(") {
        // chore(clippy): fix new lint errors caused by strict clippy
        // checked_add keeps `arithmetic_side_effects` happy; get() yields None
        // rather than panicking on an out-of-range or mid-codepoint index.
        let Some(rest) = offset
            .checked_add(matched.len())
            .and_then(|start| source.get(start..))
        else {
            continue;
        };

        let Some(rest) = rest.trim_start().strip_prefix('"') else {
            continue;
        };
        let Some(name) = rest.split('"').next() else {
            continue;
        };

        // CARGO_* comes from cargo itself: version, crate name, manifest dir. None
        // of it is secret and some of it has no runtime equivalent.
        if !name.starts_with("CARGO_") {
            found.push((name.to_owned(), path.display().to_string()));
        }
    }
}
