//! Forge Bar : barre d'extension Windows légère.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod widgets;
#[cfg(windows)]
mod win;

#[cfg(windows)]
fn main() {
    if let Err(e) = win::run() {
        win::fatal(&format!("Forge Bar n'a pas pu démarrer : {e}"));
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Forge Bar ne fonctionne que sous Windows 10 ou 11.");
}
