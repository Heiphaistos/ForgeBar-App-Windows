//! Test de débit à la demande (Cloudflare, gratuit et sans clé) : latence TCP, téléchargement, envoi.
//! Tourne sur un fil de fond ; le résultat est déposé dans un verrou puis la fenêtre principale est prévenue.

use super::http;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

const HOST: &str = "speed.cloudflare.com";
const DOWN_BYTES: usize = 25_000_000;
const UP_BYTES: usize = 10_000_000;

#[derive(Default, Clone)]
pub struct Shared {
    pub running: Arc<AtomicBool>,
    pub result: Arc<Mutex<Option<Result<String, String>>>>,
}

fn mbps(bytes: usize, d: Duration) -> f64 {
    bytes as f64 * 8.0 / d.as_secs_f64().max(1e-3) / 1e6
}

fn ping() -> Option<Duration> {
    let addr = (HOST, 443).to_socket_addrs().ok()?.next()?;
    (0..4)
        .filter_map(|_| {
            let t = Instant::now();
            TcpStream::connect_timeout(&addr, Duration::from_secs(3)).ok().map(|_| t.elapsed())
        })
        .min()
}

fn measure() -> Result<String, String> {
    let ping = ping().ok_or("serveur injoignable")?;
    let down = http::fetch("GET", HOST, &format!("/__down?bytes={DOWN_BYTES}"), &[], &[], DOWN_BYTES + 1)?;
    if down.status != 200 {
        return Err(format!("téléchargement : HTTP {}", down.status));
    }
    let up = http::fetch("POST", HOST, "/__up", &[("Content-Type", "text/plain")], &vec![b'0'; UP_BYTES], 1024)?;
    if up.status != 200 {
        return Err(format!("envoi : HTTP {}", up.status));
    }
    Ok(format!(
        "↓ {:.0} Mb/s   ↑ {:.0} Mb/s   latence {} ms",
        mbps(down.body.len(), down.read_body),
        mbps(UP_BYTES, up.to_response),
        ping.as_millis()
    ))
}

/// Lance un test s'il n'y en a pas déjà un. Prévient `main` avec `msg` à la fin.
pub fn spawn(main: HWND, msg: u32, shared: &Shared) {
    if shared.running.swap(true, Ordering::Relaxed) {
        return;
    }
    let (main, shared) = (main.0 as isize, shared.clone());
    std::thread::spawn(move || {
        *shared.result.lock().unwrap() = Some(measure());
        shared.running.store(false, Ordering::Relaxed);
        unsafe {
            let _ = PostMessageW(HWND(main as _), msg, WPARAM(0), LPARAM(0));
        }
    });
}

#[cfg(test)]
mod tests {
    /// Réseau réel : `cargo test -- --ignored speedtest`.
    #[test]
    #[ignore]
    fn live_measure() {
        let r = super::measure().expect("mesure");
        println!("{r}");
        assert!(r.contains("Mb/s"));
    }
}
