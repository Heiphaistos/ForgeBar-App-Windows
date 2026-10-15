//! Mise à jour automatique : interroge la dernière release GitHub, télécharge le MSI de la bonne
//! architecture, vérifie son SHA-256 contre `SHA256SUMS.txt` de la même release, puis prévient la fenêtre
//! principale. Tourne sur un fil de fond ; l'installation (silencieuse, par utilisateur) est lancée par l'appli.

use super::http;
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Security::Cryptography::{BCryptHash, BCRYPT_SHA256_ALG_HANDLE};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

const HOST: &str = "api.github.com";
const REPO: &str = "Heiphaistos/ForgeBar-App-Windows";
const CHECK_EVERY: Duration = Duration::from_secs(6 * 3600);
const MAX_MSI: usize = 64 * 1024 * 1024;

/// Résultat d'une vérification, lu par la fenêtre principale.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    UpToDate,
    /// MSI téléchargé et vérifié, prêt à installer.
    Ready {
        version: String,
        msi: std::path::PathBuf,
    },
    Failed(String),
}

#[derive(Default, Clone)]
pub struct Shared {
    pub outcome: Arc<Mutex<Option<Outcome>>>,
    /// Levé pour une vérification immédiate (menu, ou réglage activé).
    pub wake: Arc<AtomicBool>,
    pub enabled: Arc<AtomicBool>,
    /// Vrai si la vérification vient d'une demande explicite : le résultat « à jour » est alors annoncé.
    pub manual: Arc<AtomicBool>,
}

/// `v0.6.1` ou `0.6.1` -> (0, 6, 1).
fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let mut it = s.trim().trim_start_matches('v').split('.').map(|p| p.parse::<u32>().ok());
    Some((it.next()??, it.next()??, it.next()??))
}

/// Url du MSI de l'architecture courante et de la liste des empreintes, dans une réponse `releases/latest`.
fn pick_assets(release: &Value, version: &str, arch: &str) -> Option<(String, String)> {
    let wanted = format!("ForgeBar-{version}-{arch}.msi");
    let url_of = |name: &str| {
        release["assets"]
            .as_array()?
            .iter()
            .find(|a| a["name"] == name)
            .and_then(|a| a["browser_download_url"].as_str())
            .map(String::from)
    };
    Some((url_of(&wanted)?, url_of("SHA256SUMS.txt")?))
}

/// Empreinte attendue d'un fichier dans un `SHA256SUMS.txt` (« <hash>  <nom> », fins de ligne CRLF tolérées).
fn expected_hash(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let (hash, file) = l.trim().split_once(char::is_whitespace)?;
        (file.trim().trim_start_matches('*') == name).then(|| hash.to_ascii_lowercase())
    })
}

fn sha256(data: &[u8]) -> Option<String> {
    let mut out = [0u8; 32];
    let rc = unsafe { BCryptHash(BCRYPT_SHA256_ALG_HANDLE, None, data, &mut out) };
    rc.is_ok().then(|| out.iter().map(|b| format!("{b:02x}")).collect())
}

/// Sépare `https://hôte/chemin` en (hôte, chemin).
fn split_url(url: &str) -> Option<(&str, &str)> {
    let rest = url.strip_prefix("https://")?;
    let i = rest.find('/')?;
    Some((&rest[..i], &rest[i..]))
}

fn download(url: &str, max: usize) -> Result<Vec<u8>, String> {
    let (host, path) = split_url(url).ok_or("adresse de téléchargement inattendue")?;
    let r = http::fetch("GET", host, path, &[], &[], max)?;
    if r.status == 200 {
        Ok(r.body)
    } else {
        Err(format!("HTTP {}", r.status))
    }
}

fn check() -> Result<Outcome, String> {
    let body =
        http::get(HOST, &format!("/repos/{REPO}/releases/latest"), &[("Accept", "application/vnd.github+json")])?;
    let release: Value = serde_json::from_str(&body).map_err(|e| format!("réponse illisible : {e}"))?;
    let tag = release["tag_name"].as_str().ok_or("release sans nom de version")?;
    let latest = parse_version(tag).ok_or("numéro de version illisible")?;
    let current = parse_version(env!("CARGO_PKG_VERSION")).ok_or("version courante illisible")?;
    if latest <= current {
        return Ok(Outcome::UpToDate);
    }
    let version = tag.trim_start_matches('v').to_string();
    let arch = if cfg!(target_arch = "x86_64") { "x64" } else { "x86" };
    let (msi_url, sums_url) = pick_assets(&release, &version, arch).ok_or("MSI absent de la release")?;
    let sums = String::from_utf8_lossy(&download(&sums_url, 1 << 20)?).into_owned();
    let name = format!("ForgeBar-{version}-{arch}.msi");
    let want = expected_hash(&sums, &name).ok_or("empreinte absente de SHA256SUMS.txt")?;
    let data = download(&msi_url, MAX_MSI)?;
    if sha256(&data).as_deref() != Some(want.as_str()) {
        return Err("empreinte SHA-256 du MSI différente de celle publiée : installation refusée".into());
    }
    let dir = std::env::temp_dir().join("ForgeBar-update");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let msi = dir.join(name);
    std::fs::write(&msi, &data).map_err(|e| e.to_string())?;
    Ok(Outcome::Ready { version, msi })
}

/// Vérifie 30 s après le démarrage puis toutes les 6 h, ou tout de suite sur `wake`. Prévient `main` avec `msg`.
pub fn spawn(main: HWND, msg: u32, shared: &Shared) {
    let (main, s) = (main.0 as isize, shared.clone());
    std::thread::spawn(move || {
        let mut waited = Duration::ZERO;
        let first = Duration::from_secs(30);
        let mut next = first;
        loop {
            std::thread::sleep(Duration::from_secs(1));
            waited += Duration::from_secs(1);
            let manual = s.wake.swap(false, Ordering::Relaxed);
            if !manual && (waited < next || !s.enabled.load(Ordering::Relaxed)) {
                continue;
            }
            waited = Duration::ZERO;
            next = CHECK_EVERY;
            s.manual.store(manual, Ordering::Relaxed);
            let result = check().unwrap_or_else(Outcome::Failed);
            *s.outcome.lock().unwrap() = Some(result);
            unsafe {
                let _ = PostMessageW(HWND(main as _), msg, WPARAM(0), LPARAM(0));
            }
        }
    });
}

/// Vrai si l'exécutable tourne depuis le dossier d'installation du MSI (%LOCALAPPDATA%\Programs\ForgeBar).
pub fn is_installed_copy(exe: &str) -> bool {
    let Ok(base) = std::env::var("LOCALAPPDATA") else { return false };
    let dir = std::path::Path::new(&base).join("Programs").join("ForgeBar");
    std::path::Path::new(exe).starts_with(dir)
}

/// Contenu du script d'installation : attend la fermeture de l'appli, ferme les exécutables verrouillés,
/// installe le MSI en silence, note le code de sortie puis relance la barre.
fn apply_script(msi: &std::path::Path, exe: &str, log: &std::path::Path) -> String {
    let lines = [
        "@echo off".to_string(),
        "ping -n 3 127.0.0.1 >nul".to_string(),
        "taskkill /F /IM forge-settings.exe >nul 2>&1".to_string(),
        format!("msiexec /i \"{}\" /qn /norestart", msi.display()),
        format!("echo %date% %time% msiexec code %errorlevel% >> \"{}\"", log.display()),
        format!("start \"\" \"{exe}\""),
    ];
    lines.join("\r\n") + "\r\n"
}

/// Installe le MSI en silence puis relance la barre : un script détaché survit à la fermeture de l'appli.
/// (Un script plutôt qu'une ligne `cmd /C` : les guillemets imbriqués y sont mal interprétés.)
pub fn install_and_restart(msi: &std::path::Path, exe: &str) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let dir = msi.parent().unwrap_or(std::path::Path::new("."));
    let bat = dir.join("apply.cmd");
    if std::fs::write(&bat, apply_script(msi, exe, &dir.join("update.log"))).is_err() {
        return;
    }
    let _ = std::process::Command::new("cmd")
        .raw_arg(format!("/C \"\"{}\"\"", bat.display()))
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_numerically() {
        assert!(parse_version("v0.10.0") > parse_version("0.9.9"));
        assert_eq!(parse_version("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("nightly"), None);
    }

    #[test]
    fn picks_msi_for_the_architecture() {
        let r: Value = serde_json::from_str(
            r#"{"assets":[{"name":"ForgeBar-0.6.0-x64.msi","browser_download_url":"https://h/a"},
               {"name":"ForgeBar-0.6.0-x86.msi","browser_download_url":"https://h/b"},
               {"name":"SHA256SUMS.txt","browser_download_url":"https://h/s"}]}"#,
        )
        .unwrap();
        assert_eq!(pick_assets(&r, "0.6.0", "x86"), Some(("https://h/b".into(), "https://h/s".into())));
        assert_eq!(pick_assets(&r, "0.7.0", "x64"), None);
    }

    #[test]
    fn checksum_file_with_crlf_and_hash_match() {
        let sums = "ABC123  other.zip\r\nDEF456  ForgeBar-0.6.0-x64.msi\r\n";
        assert_eq!(expected_hash(sums, "ForgeBar-0.6.0-x64.msi"), Some("def456".into()));
        assert_eq!(expected_hash(sums, "missing"), None);
        assert_eq!(sha256(b"abc").as_deref(), Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"));
    }

    #[test]
    fn apply_script_quotes_every_path() {
        let msi = std::path::Path::new(r"C:\a b\x.msi");
        let s = apply_script(msi, r"C:\c d\forge-bar.exe", std::path::Path::new(r"C:\a b\u.log"));
        assert!(s.contains("msiexec /i \"C:\\a b\\x.msi\" /qn"));
        assert!(s.contains("start \"\" \"C:\\c d\\forge-bar.exe\""));
        assert!(s.ends_with("\r\n"));
    }

    #[test]
    fn splits_download_url() {
        assert_eq!(split_url("https://github.com/x/y.msi"), Some(("github.com", "/x/y.msi")));
        assert_eq!(split_url("http://github.com/x"), None);
    }

    #[test]
    fn installed_copy_detection() {
        let base = std::env::var("LOCALAPPDATA").unwrap();
        assert!(is_installed_copy(&format!("{base}\\Programs\\ForgeBar\\forge-bar.exe")));
        assert!(!is_installed_copy("D:\\Projet\\ForgeBar\\target\\release\\forge-bar.exe"));
    }
}
