//! Intègre tous les thèmes de `themes/` dans le binaire.
use std::{env, fs, path::PathBuf};

fn main() {
    let themes = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("../../themes");
    println!("cargo:rerun-if-changed={}", themes.display());
    let mut entries: Vec<_> = fs::read_dir(&themes)
        .expect("dossier themes introuvable")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .collect();
    entries.sort();
    let mut out = String::from("pub static BUILTIN_THEMES: &[(&str, &str)] = &[\n");
    for p in entries {
        let id = p.file_stem().unwrap().to_string_lossy().to_string();
        let abs = fs::canonicalize(&p).unwrap();
        out.push_str(&format!("    ({id:?}, include_str!({:?})),\n", abs.display().to_string()));
    }
    out.push_str("];\n");
    let dest = PathBuf::from(env::var("OUT_DIR").unwrap()).join("builtin_themes.rs");
    fs::write(dest, out).unwrap();
}
