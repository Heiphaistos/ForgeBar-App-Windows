//! Aperçus réels : `forge-bar.exe --snapshot <png> --theme <id> --width <px> [--preset p] [--edge e] --config <chemin>`.
//! Les PNG sont mis en cache dans `%TEMP%\forgebar-previews`. Si l'exécutable manque ou échoue,
//! la route répond 404/502 et l'interface dessine un aperçu de repli en CSS.

use crate::api::{build_theme_table, valid_name, write_atomic, Ctx, PRESETS};
use crate::http::Response;
use serde::Deserialize;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

const EDGES: &[&str] = &["top", "bottom", "left", "right"];
const TIMEOUT: Duration = Duration::from_secs(10);
const DRAFT_ID: &str = "zz-draft";

#[derive(Debug, PartialEq)]
pub struct Spec {
    pub theme: String,
    pub preset: Option<String>,
    pub edge: Option<String>,
    pub width: u32,
}

fn opt_in(v: Option<&str>, allowed: &[&str], what: &str) -> Result<Option<String>, String> {
    match v.filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some(s) if allowed.contains(&s) => Ok(Some(s.to_string())),
        Some(s) => Err(format!("{what} inconnu : {s}")),
    }
}

pub fn parse_spec(theme: &str, preset: Option<&str>, edge: Option<&str>, width: Option<&str>) -> Result<Spec, String> {
    if !valid_name(theme) {
        return Err("nom de thème invalide".into());
    }
    let width = match width.filter(|w| !w.is_empty()) {
        None => 900,
        Some(w) => w.parse::<u32>().ok().filter(|w| (100..=3000).contains(w)).ok_or("largeur entre 100 et 3000")?,
    };
    Ok(Spec {
        theme: theme.into(),
        preset: opt_in(preset, PRESETS, "préréglage")?,
        edge: opt_in(edge, EDGES, "bord")?,
        width,
    })
}

pub fn exe_path(ctx: &Ctx) -> Option<PathBuf> {
    let name = if cfg!(windows) { "forge-bar.exe" } else { "forge-bar" };
    ctx.exe_dir.as_ref().map(|d| d.join(name)).filter(|p| p.is_file())
}

fn mtime(p: &Path) -> u64 {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs())
}

fn key(parts: &[&str], stamps: &[u64]) -> String {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    parts.hash(&mut h);
    stamps.hash(&mut h);
    format!("{:016x}", h.finish())
}

/// Lance le snapshot et attend au plus `TIMEOUT` ; ne tue que son propre enfant.
fn run_snapshot(exe: &Path, spec: &Spec, config: &Path, out: &Path, appdata: Option<&Path>) -> Result<(), String> {
    let mut cmd = Command::new(exe);
    cmd.arg("--snapshot").arg(out).arg("--theme").arg(&spec.theme).arg("--width").arg(spec.width.to_string());
    if let Some(p) = &spec.preset {
        cmd.args(["--preset", p]);
    }
    if let Some(e) = &spec.edge {
        cmd.args(["--edge", e]);
    }
    cmd.arg("--config").arg(config).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    if let Some(a) = appdata {
        cmd.env("APPDATA", a);
    }
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut cmd, 0x0800_0000);
    let mut child = cmd.spawn().map_err(|e| format!("lancement de forge-bar impossible : {e}"))?;
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(st)) if st.success() => return Ok(()),
            Ok(Some(st)) => return Err(format!("forge-bar --snapshot a échoué ({st})")),
            Ok(None) if start.elapsed() > TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("forge-bar --snapshot : délai dépassé".into());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// Rend (ou relit du cache) le PNG d'une spécification.
fn render(ctx: &Ctx, spec: &Spec, cache_key: &str, appdata: Option<&Path>) -> Result<Vec<u8>, Response> {
    let Some(exe) = exe_path(ctx) else {
        return Err(Response::err(404, "forge-bar introuvable à côté de forge-settings"));
    };
    let png = ctx.previews.join(format!("{cache_key}.png"));
    if let Ok(b) = std::fs::read(&png) {
        return Ok(b);
    }
    let _guard = ctx.snap_lock.lock().unwrap_or_else(|e| e.into_inner());
    if let Ok(b) = std::fs::read(&png) {
        return Ok(b);
    }
    std::fs::create_dir_all(&ctx.previews).map_err(|e| Response::err(500, &e.to_string()))?;
    let part = ctx.previews.join(format!("{cache_key}.partial.png"));
    let res = run_snapshot(&exe, spec, &ctx.config_path(), &part, appdata);
    let done = res.and_then(|()| std::fs::rename(&part, &png).map_err(|e| e.to_string()));
    let _ = std::fs::remove_file(&part);
    done.map_err(|e| Response::err(502, &e))?;
    std::fs::read(&png).map_err(|e| Response::err(500, &e.to_string()))
}

fn png_response(bytes: Vec<u8>) -> Response {
    Response { status: 200, ctype: "image/png", body: bytes, csp: None }
}

pub fn get(ctx: &Ctx, q: &HashMap<String, String>) -> Response {
    let g = |k: &str| q.get(k).map(String::as_str);
    let spec = match parse_spec(g("theme").unwrap_or(""), g("preset"), g("edge"), g("width")) {
        Ok(s) => s,
        Err(e) => return Response::err(400, &e),
    };
    let exe = exe_path(ctx);
    let stamps = [
        mtime(&ctx.themes_dir().join(format!("{}.toml", spec.theme))),
        mtime(&ctx.config_path()),
        exe.as_deref().map_or(0, mtime),
    ];
    let w = spec.width.to_string();
    let k = key(&[&spec.theme, spec.preset.as_deref().unwrap_or(""), spec.edge.as_deref().unwrap_or(""), &w], &stamps);
    match render(ctx, &spec, &k, None) {
        Ok(b) => png_response(b),
        Err(r) => r,
    }
}

#[derive(Deserialize)]
struct DraftIn {
    extends: String,
    overrides: serde_json::Value,
    preset: Option<String>,
    edge: Option<String>,
    width: Option<u32>,
}

/// Aperçu d'un thème non enregistré : le brouillon vit dans un APPDATA temporaire passé à l'enfant,
/// pour ne rien ajouter aux thèmes réels de l'utilisateur.
pub fn draft(ctx: &Ctx, body: &[u8]) -> Response {
    let d: DraftIn = match serde_json::from_slice(body) {
        Ok(d) => d,
        Err(e) => return Response::err(400, &e.to_string()),
    };
    let spec =
        match parse_spec(DRAFT_ID, d.preset.as_deref(), d.edge.as_deref(), d.width.map(|w| w.to_string()).as_deref()) {
            Ok(s) => s,
            Err(e) => return Response::err(400, &e),
        };
    let table = match build_theme_table(ctx, DRAFT_ID, &d.extends, &d.overrides) {
        Ok(t) => t,
        Err(e) => return Response::err(400, &e),
    };
    let text = match toml::to_string_pretty(&table) {
        Ok(t) => t,
        Err(e) => return Response::err(400, &e.to_string()),
    };
    let w = spec.width.to_string();
    let user_stamp = std::fs::read_dir(ctx.themes_dir()).map_or(0, |rd| rd.flatten().map(|e| mtime(&e.path())).sum());
    let k = key(
        &["draft", &text, spec.preset.as_deref().unwrap_or(""), spec.edge.as_deref().unwrap_or(""), &w],
        &[user_stamp, mtime(&ctx.config_path()), exe_path(ctx).as_deref().map_or(0, mtime)],
    );
    let fake = ctx.previews.join(format!("appdata-{k}"));
    let themes = fake.join("ForgeBar").join("themes");
    let cached = ctx.previews.join(format!("{k}.png")).is_file();
    if !cached {
        if let Ok(rd) = std::fs::read_dir(ctx.themes_dir()) {
            for e in rd.flatten().filter(|e| e.path().extension().is_some_and(|x| x == "toml")) {
                if let Ok(b) = std::fs::read(e.path()) {
                    let _ = write_atomic(&fake, &themes.join(e.file_name()), &b);
                }
            }
        }
        if let Err(e) = write_atomic(&fake, &themes.join(format!("{DRAFT_ID}.toml")), text.as_bytes()) {
            return Response::err(500, &e);
        }
    }
    let out = render(ctx, &spec, &k, Some(&fake));
    let _ = std::fs::remove_dir_all(&fake);
    match out {
        Ok(b) => png_response(b),
        Err(r) => r,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_validation() {
        let s = parse_spec("nord", Some("double_row"), Some("top"), Some("800")).unwrap();
        assert_eq!(s.width, 800);
        assert_eq!(parse_spec("nord", None, None, None).unwrap().width, 900);
        assert!(parse_spec("../x", None, None, None).is_err());
        assert!(parse_spec("nord", Some("xx"), None, None).is_err());
        assert!(parse_spec("nord", None, Some("diag"), None).is_err());
        assert!(parse_spec("nord", None, None, Some("99999")).is_err());
        assert!(parse_spec("nord", None, None, Some("abc")).is_err());
    }

    #[test]
    fn missing_exe_gives_404_for_fallback() {
        let ctx = crate::api::test_ctx();
        let q: HashMap<String, String> = [("theme".to_string(), "nord".to_string())].into();
        assert_eq!(get(&ctx, &q).status, 404);
        assert_eq!(key(&["a"], &[1]), key(&["a"], &[1]));
        assert_ne!(key(&["a"], &[1]), key(&["a"], &[2]));
    }
}
