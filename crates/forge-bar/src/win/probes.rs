//! Mesures lentes ou bloquantes, hors du fil de l'interface : latence réseau (widget « ping ») et commandes des
//! widgets personnalisés. Un résultat nouveau est déposé dans un verrou puis la fenêtre principale est prévenue.

use forge_config::{CustomKind, CustomWidget};
use std::collections::HashMap;
use std::io::Read;
use std::net::{TcpStream, ToSocketAddrs};
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// Une commande qui dépasse ce délai est arrêtée : un widget ne doit jamais accumuler des processus.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(8);
const NO_PING: u32 = u32::MAX;

#[derive(Clone)]
pub struct Probes {
    pub want_ping: Arc<AtomicBool>,
    ping_ms: Arc<AtomicU32>,
    /// Widgets de type commande réellement présents dans une disposition.
    pub commands: Arc<Mutex<Vec<CustomWidget>>>,
    pub out: Arc<Mutex<HashMap<String, String>>>,
    /// Un clic sur un widget commande demande une relance immédiate.
    pub refresh: Arc<Mutex<Vec<String>>>,
}

impl Default for Probes {
    fn default() -> Self {
        Self {
            want_ping: Arc::default(),
            ping_ms: Arc::new(AtomicU32::new(NO_PING)),
            commands: Arc::default(),
            out: Arc::default(),
            refresh: Arc::default(),
        }
    }
}

impl Probes {
    pub fn ping(&self) -> Option<u32> {
        Some(self.ping_ms.load(Ordering::Relaxed)).filter(|v| *v != NO_PING)
    }
}

fn measure_ping() -> Option<u32> {
    let addr = ("1.1.1.1", 443).to_socket_addrs().ok()?.next()?;
    (0..2)
        .filter_map(|_| {
            let t = Instant::now();
            TcpStream::connect_timeout(&addr, Duration::from_secs(2)).ok().map(|_| t.elapsed().as_millis() as u32)
        })
        .min()
}

/// Première ligne non vide de la sortie, nettoyée.
pub fn first_line(out: &str) -> String {
    let clean = |l: &str| l.chars().filter(|c| !c.is_control()).collect::<String>().trim().to_string();
    out.lines().map(clean).find(|l| !l.is_empty()).unwrap_or_default()
}

/// Sortie d'une commande en texte : UTF-8 si c'en est, sinon page de codes OEM de la console (accents de `echo`, `dir`…).
fn decode(bytes: &[u8]) -> String {
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }
    use windows::Win32::Globalization::{MultiByteToWideChar, CP_OEMCP, MULTI_BYTE_TO_WIDE_CHAR_FLAGS};
    unsafe {
        let n = MultiByteToWideChar(CP_OEMCP, MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0), bytes, None);
        if n <= 0 {
            return String::from_utf8_lossy(bytes).into_owned();
        }
        let mut wide = vec![0u16; n as usize];
        MultiByteToWideChar(CP_OEMCP, MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0), bytes, Some(&mut wide));
        String::from_utf16_lossy(&wide)
    }
}

/// Exécute la commande (`cmd /C`, console en UTF-8) avec un délai maximal et renvoie la première ligne de sortie.
fn run_command(command: &str) -> String {
    let child = Command::new("cmd")
        .raw_arg(format!("/C chcp 65001 >nul & {command}"))
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let Ok(mut child) = child else { return "erreur".into() };
    let Some(mut stdout) = child.stdout.take() else { return "erreur".into() };
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.by_ref().take(64 * 1024).read_to_end(&mut buf);
        let _ = tx.send(buf);
    });
    match rx.recv_timeout(COMMAND_TIMEOUT) {
        Ok(buf) => {
            let _ = child.wait();
            let line = first_line(&decode(&buf));
            if line.is_empty() {
                "—".into()
            } else {
                line
            }
        }
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            "délai dépassé".into()
        }
    }
}

/// Lance la boucle de mesure. `main` reçoit `msg` à chaque nouveau résultat.
pub fn spawn(main: HWND, msg: u32, probes: &Probes) {
    let (main, p) = (main.0 as isize, probes.clone());
    std::thread::spawn(move || {
        let mut last: HashMap<String, Instant> = HashMap::new();
        let mut tick = 0u64;
        loop {
            std::thread::sleep(Duration::from_secs(1));
            tick += 1;
            let mut changed = false;
            if p.want_ping.load(Ordering::Relaxed) && tick % 5 == 1 {
                let v = measure_ping().unwrap_or(NO_PING);
                changed |= p.ping_ms.swap(v, Ordering::Relaxed) != v;
            }
            let forced: Vec<String> = std::mem::take(&mut *p.refresh.lock().unwrap());
            let todo: Vec<CustomWidget> = p
                .commands
                .lock()
                .unwrap()
                .iter()
                .filter(|c| c.kind == CustomKind::Command && !c.command.trim().is_empty())
                .filter(|c| {
                    forced.contains(&c.id)
                        || last.get(&c.id).is_none_or(|t| t.elapsed() >= Duration::from_secs(c.interval_secs.max(2)))
                })
                .cloned()
                .collect();
            for c in todo {
                last.insert(c.id.clone(), Instant::now());
                let out = run_command(&c.command);
                let mut map = p.out.lock().unwrap();
                changed |= map.insert(c.id.clone(), out.clone()).as_deref() != Some(out.as_str());
            }
            if changed {
                unsafe {
                    let _ = PostMessageW(HWND(main as _), msg, WPARAM(0), LPARAM(0));
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_line_skips_blanks_and_controls() {
        assert_eq!(first_line("\r\n  \n  main ≡ \u{7}\nsuite"), "main ≡");
        assert_eq!(first_line(""), "");
    }

    #[test]
    fn command_output_and_timeout() {
        assert_eq!(run_command("echo bonjour é"), "bonjour é");
        assert_eq!(run_command("exit 0"), "—");
    }
}
