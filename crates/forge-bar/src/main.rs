//! Forge Bar : barre d'extension Windows légère.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
// Lint de clippy récent (1.98) : les pixels BGRA sont lus par paquets de 4, as_chunks n'apporte rien ici.
#![allow(unknown_lints, clippy::chunks_exact_to_as_chunks)]

mod calendar;
mod content;
mod dominant;
mod svg;
mod weather;
mod widgets;
#[cfg(windows)]
mod win;

#[cfg(windows)]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a.starts_with("--snapshot")) {
        std::process::exit(win::snapshot::run(&args));
    }
    if let Err(e) = win::run() {
        win::fatal(&format!("Forge Bar n'a pas pu démarrer : {e}"));
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Forge Bar ne fonctionne que sous Windows 10 ou 11.");
}
