//! Journal minimal `%APPDATA%\ForgeBar\logs\forge-bar.log` : `[ISO8601] [NIVEAU] message`, archivé au-delà de 1 Mo.
//! Sert à comprendre à distance pourquoi un widget manque ou une configuration est rejetée.

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};

/// Seule la vraie barre écrit dans le journal : les rendus d'aperçu (`--snapshot`) le polluaient.
static ENABLED: AtomicBool = AtomicBool::new(false);

pub fn enable() {
    ENABLED.store(true, Ordering::Relaxed);
}

const MAX_BYTES: u64 = 1_000_000;

pub fn info(msg: &str) {
    write("INFO", msg);
}

fn write(level: &str, msg: &str) {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }
    let dir = forge_config::app_dir().join("logs");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let file = dir.join("forge-bar.log");
    if std::fs::metadata(&file).is_ok_and(|m| m.len() > MAX_BYTES) {
        let _ = std::fs::create_dir_all(dir.join("archive"));
        let _ = std::fs::rename(&file, dir.join("archive").join("forge-bar.log"));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&file) {
        let _ = writeln!(f, "[{}] [{level}] {msg}", chrono::Local::now().to_rfc3339());
    }
}
