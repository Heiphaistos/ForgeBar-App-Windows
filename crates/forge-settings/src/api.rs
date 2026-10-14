//! Logique de l'éditeur : état de la configuration vers JSON, validation et écritures.
//! Toutes les écritures restent dans `%APPDATA%\ForgeBar` (`config.toml`, `themes\*.toml`).

use crate::cred;
use crate::http::Response;
use forge_config::{parse_color, Config, Layout, LayoutPreset, Theme, ThemeStore, Zones, ALL_WIDGETS};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

pub const PRESETS: &[&str] =
    &["classic", "centered", "split", "islands", "compact", "hud_first", "double_row", "custom"];
const STYLES: &[&str] = &["text", "gauge", "ring", "graph"];
const CLAUDE_SOURCES: &[&str] = &["claude_code", "api"];
const SUMMARY_ITEMS: &[&str] = &["context", "tokens_today", "cost_today", "cost_week", "cost_month"];

pub struct Ctx {
    pub port: u16,
    pub token: String,
    pub nonce: String,
    /// `%APPDATA%\ForgeBar` : seul endroit où l'on écrit.
    pub dir: PathBuf,
    /// Dossier temporaire des aperçus (`%TEMP%\forgebar-previews`).
    pub previews: PathBuf,
    pub exe_dir: Option<PathBuf>,
    pub quit: AtomicBool,
    pub snap_lock: Mutex<()>,
    start: Instant,
    last: AtomicU64,
}

impl Ctx {
    pub fn new(
        port: u16,
        token: String,
        nonce: String,
        dir: PathBuf,
        previews: PathBuf,
        exe_dir: Option<PathBuf>,
    ) -> Self {
        Self {
            port,
            token,
            nonce,
            dir,
            previews,
            exe_dir,
            quit: AtomicBool::new(false),
            snap_lock: Mutex::new(()),
            start: Instant::now(),
            last: AtomicU64::new(0),
        }
    }
    pub fn touch(&self) {
        self.last.store(self.start.elapsed().as_secs(), Ordering::Relaxed);
    }
    pub fn idle_secs(&self) -> u64 {
        self.start.elapsed().as_secs().saturating_sub(self.last.load(Ordering::Relaxed))
    }
    pub fn config_path(&self) -> PathBuf {
        self.dir.join("config.toml")
    }
    pub fn themes_dir(&self) -> PathBuf {
        self.dir.join("themes")
    }
    pub fn store(&self) -> ThemeStore {
        ThemeStore::load(Some(&self.themes_dir()))
    }
}

#[cfg(test)]
pub fn test_ctx() -> Ctx {
    let base = std::env::temp_dir().join(format!("forge-settings-test-{}", cred::random_hex(6).unwrap_or_default()));
    let dir = base.join("ForgeBar");
    let _ = std::fs::create_dir_all(&dir);
    Ctx::new(4242, "tok-test-0123456789".into(), "nonce".into(), dir, base.join("previews"), None)
}

/// `[a-z0-9-]{1,40}` : le seul format de nom de thème accepté (anti path traversal).
pub fn valid_name(n: &str) -> bool {
    (1..=40).contains(&n.len()) && n.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Écriture atomique : fichier temporaire voisin puis renommage. Refuse tout chemin hors de `root`.
pub fn write_atomic(root: &Path, path: &Path, data: &[u8]) -> Result<(), String> {
    if !path.starts_with(root) || path.components().any(|c| c.as_os_str() == "..") {
        return Err("écriture hors du dossier ForgeBar refusée".into());
    }
    let dir = path.parent().ok_or("chemin invalide")?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let name = path.file_name().ok_or("chemin invalide")?.to_string_lossy();
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    std::fs::write(&tmp, data).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}

fn zones_json(z: &Zones) -> Value {
    json!({ "left": z.left, "center": z.center, "right": z.right })
}

fn presets_json() -> Value {
    let list: Vec<Value> = PRESETS
        .iter()
        .map(|id| {
            let preset: LayoutPreset = serde_json::from_value(json!(id)).unwrap_or(LayoutPreset::Custom);
            let rows = Layout { preset, ..Layout::default() }.rows();
            if preset == LayoutPreset::Custom {
                return json!({ "id": id, "zones": zones_json(&Zones::default()), "row2": null });
            }
            json!({ "id": id, "zones": zones_json(&rows[0]), "row2": rows.get(1).map(zones_json) })
        })
        .collect();
    Value::Array(list)
}

fn user_extends(dir: &Path, id: &str) -> Option<String> {
    let text = std::fs::read_to_string(dir.join(format!("{id}.toml"))).ok()?;
    text.parse::<toml::Value>().ok()?.get("extends")?.as_str().map(String::from)
}

fn themes_json(ctx: &Ctx) -> Value {
    let store = ctx.store();
    let dir = ctx.themes_dir();
    let list: Vec<Value> = store
        .ids()
        .into_iter()
        .map(|id| {
            let mut v = serde_json::to_value(store.get(&id)).unwrap_or(Value::Null);
            let user = dir.join(format!("{id}.toml")).is_file();
            if let Some(o) = v.as_object_mut() {
                o.insert("id".into(), json!(id));
                o.insert("user".into(), json!(user));
                o.insert("extends".into(), json!(if user { user_extends(&dir, &id) } else { None }));
            }
            v
        })
        .collect();
    Value::Array(list)
}

fn icon_packs(ctx: &Ctx) -> Vec<String> {
    let mut v = vec!["forge".to_string()];
    if let Ok(rd) = std::fs::read_dir(ctx.dir.join("icons")) {
        v.extend(rd.flatten().filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().into_owned()));
    }
    v.sort();
    v.dedup();
    v
}

pub fn state(ctx: &Ctx) -> Response {
    let (cfg, config_error) = match Config::load_or_create(&ctx.config_path()) {
        Ok(c) => (c, None),
        Err(e) => (Config::default(), Some(e.to_string())),
    };
    Response::json(
        200,
        &json!({
            "ok": true,
            "config": cfg,
            "config_error": config_error,
            "config_path": ctx.config_path().display().to_string(),
            "themes": themes_json(ctx),
            "widgets": ALL_WIDGETS,
            "presets": presets_json(),
            "styles": STYLES,
            "claude_sources": CLAUDE_SOURCES,
            "summary_items": SUMMARY_ITEMS,
            "metrics": ["battery", "cpu", "ram", "gpu", "claude"],
            "icon_packs": icon_packs(ctx),
            "admin_key": cred::admin_key_exists(),
            "can_preview": crate::previews::exe_path(ctx).is_some(),
        }),
    )
}

fn all_widgets(list: &[String], what: &str) -> Result<(), String> {
    match list.iter().find(|w| !ALL_WIDGETS.contains(&w.as_str())) {
        Some(w) => Err(format!("{what} : widget inconnu « {w} »")),
        None => Ok(()),
    }
}

fn subset(list: &[String], allowed: &[&str], what: &str) -> Result<(), String> {
    match list.iter().find(|w| !allowed.contains(&w.as_str())) {
        Some(w) => Err(format!("{what} : valeur inconnue « {w} »")),
        None => Ok(()),
    }
}

/// Validation de `Config::validate` plus les contrôles que l'éditeur impose avant d'écrire.
pub fn check_config(cfg: &Config, theme_ids: &[String]) -> Result<(), String> {
    cfg.validate().map_err(|e| e.to_string())?;
    for (i, s) in cfg.screen.iter().enumerate() {
        let n = i + 1;
        let id_ok = matches!(s.id.as_str(), "primary" | "all") || s.id.parse::<u32>().is_ok_and(|v| v >= 1);
        if !id_ok {
            return Err(format!("écran {n} : identifiant « {} » invalide (primary, all ou un numéro)", s.id));
        }
        if !theme_ids.contains(&s.theme) {
            return Err(format!("écran {n} : thème inconnu « {} »", s.theme));
        }
        if s.margin > 200 {
            return Err(format!("écran {n} : marge limitée à 200"));
        }
        let l = &s.layout;
        for (name, z) in [("left", &l.left), ("center", &l.center), ("right", &l.right)] {
            all_widgets(z, &format!("écran {n}, zone {name}"))?;
        }
        for (name, z) in [("left", &l.row2.left), ("center", &l.row2.center), ("right", &l.row2.right)] {
            all_widgets(z, &format!("écran {n}, rangée 2, zone {name}"))?;
        }
        if let Some(w) = s.styles.keys().chain(s.show_when.keys()).find(|k| !ALL_WIDGETS.contains(&k.as_str())) {
            return Err(format!("écran {n} : style ou condition sur un widget inconnu « {w} »"));
        }
    }
    let pack = &cfg.general.icon_pack;
    if pack.contains(['/', '\\', ':']) || pack.contains("..") {
        return Err("pack d'icônes invalide".into());
    }
    let c = &cfg.claude;
    subset(&c.sources, CLAUDE_SOURCES, "claude.sources")?;
    subset(&c.bar_summary, SUMMARY_ITEMS, "claude.bar_summary")?;
    if c.alerts.iter().any(|a| !(1..=100).contains(a)) {
        return Err("claude.alerts : pourcentages entre 1 et 100".into());
    }
    if c.context_window == 0 {
        return Err("claude.context_window doit être positif".into());
    }
    let money = [c.budgets.daily_usd, c.budgets.weekly_usd, c.budgets.monthly_usd, c.subscription.five_hour_limit_usd];
    if money.iter().flatten().any(|v| !v.is_finite() || *v < 0.0) {
        return Err("les budgets doivent être des montants positifs".into());
    }
    if c.floating.margin > 500 {
        return Err("claude.floating.margin limitée à 500".into());
    }
    let w = &cfg.weather;
    if w.latitude.is_some_and(|v| !(-90.0..=90.0).contains(&v))
        || w.longitude.is_some_and(|v| !(-180.0..=180.0).contains(&v))
    {
        return Err("météo : latitude entre -90 et 90, longitude entre -180 et 180".into());
    }
    if w.refresh_minutes == 0 || w.city.len() > 100 {
        return Err("météo : rafraîchissement d'au moins 1 minute, ville de 100 caractères maximum".into());
    }
    if !(1..=100).contains(&cfg.clipboard.history) || !(10..=500).contains(&cfg.clipboard.max_chars) {
        return Err("presse-papiers : historique de 1 à 100, longueur de 10 à 500".into());
    }
    Ok(())
}

pub fn post_config(ctx: &Ctx, body: &[u8]) -> Response {
    let cfg: Config = match serde_json::from_slice(body) {
        Ok(c) => c,
        Err(e) => return Response::err(400, &format!("configuration illisible : {e}")),
    };
    if let Err(e) = check_config(&cfg, &ctx.store().ids()) {
        return Response::err(400, &e);
    }
    match write_atomic(&ctx.dir, &ctx.config_path(), cfg.to_toml().as_bytes()) {
        Ok(()) => Response::ok(),
        Err(e) => Response::err(500, &e),
    }
}

pub fn post_condition(body: &[u8]) -> Response {
    #[derive(Deserialize)]
    struct In {
        text: String,
    }
    match serde_json::from_slice::<In>(body) {
        Ok(i) => match forge_config::Condition::parse(&i.text) {
            Ok(_) => Response::ok(),
            Err(e) => Response::err(400, &e),
        },
        Err(e) => Response::err(400, &e.to_string()),
    }
}

fn merge(base: &mut toml::Value, over: toml::Value) {
    match (base, over) {
        (toml::Value::Table(b), toml::Value::Table(o)) => {
            for (k, v) in o {
                match b.get_mut(&k) {
                    Some(e) => merge(e, v),
                    None => {
                        b.insert(k, v);
                    }
                }
            }
        }
        (b, o) => *b = o,
    }
}

fn check_theme(t: &Theme) -> Result<(), String> {
    let colors = [&t.colors.text, &t.colors.text_secondary, &t.colors.accent, &t.colors.warning, &t.colors.critical];
    if colors.iter().any(|c| parse_color(c).is_none()) {
        return Err("couleur invalide (format #RRGGBB ou #RRGGBBAA)".into());
    }
    if !t.colors.border.is_empty() && parse_color(&t.colors.border).is_none() {
        return Err("couleur de bordure invalide".into());
    }
    let bg = &t.background;
    if bg.colors.is_empty() || bg.colors.len() > 8 || bg.colors.iter().any(|c| parse_color(c).is_none()) {
        return Err("fond : de 1 à 8 couleurs valides".into());
    }
    let ranges = [
        ("rayon", t.radius, 0.0, 48.0),
        ("bordure", t.border_width, 0.0, 16.0),
        ("taille de police", t.font_size, 6.0, 48.0),
        ("espacement", t.spacing, 0.0, 64.0),
        ("opacité", bg.opacity, 0.0, 1.0),
    ];
    for (what, v, lo, hi) in ranges {
        if !(lo..=hi).contains(&v) {
            return Err(format!("{what} hors limites ({lo} à {hi})"));
        }
    }
    if t.name.trim().is_empty() || t.name.len() > 60 || t.font_family.len() > 80 || t.mono_family.len() > 80 {
        return Err("nom ou police trop long ou vide".into());
    }
    Ok(())
}

/// Construit la table à écrire (`extends` + valeurs modifiées) après avoir vérifié que
/// le thème résolu (base + surcharges) est valide.
pub fn build_theme_table(ctx: &Ctx, id: &str, extends: &str, overrides: &Value) -> Result<toml::Table, String> {
    if !valid_name(id) {
        return Err("nom de thème invalide (a-z, 0-9 et tirets, 40 caractères maximum)".into());
    }
    let store = ctx.store();
    if id == extends || !store.ids().iter().any(|i| i == extends) {
        return Err(format!("thème de base inconnu ou identique : « {extends} »"));
    }
    let dir = ctx.themes_dir();
    let mut cur = Some(extends.to_string());
    for _ in 0..8 {
        match cur.take() {
            Some(c) if c == id => return Err("héritage circulaire".into()),
            Some(c) => cur = user_extends(&dir, &c),
            None => break,
        }
    }
    let Value::Object(map) = overrides else { return Err("surcharges attendues sous forme d'objet".into()) };
    if map.contains_key("extends") {
        return Err("la clé extends est gérée par l'éditeur".into());
    }
    let mut over = toml::Value::try_from(overrides).map_err(|e| format!("surcharges invalides : {e}"))?;
    if let Some(t) = over.as_table_mut() {
        t.entry("name").or_insert_with(|| toml::Value::String(id.into()));
    }
    let mut full = toml::Value::try_from(store.get(extends)).map_err(|e| e.to_string())?;
    merge(&mut full, over.clone());
    let resolved: Theme = full.try_into().map_err(|e: toml::de::Error| format!("thème invalide : {e}"))?;
    check_theme(&resolved)?;
    let mut table = toml::Table::new();
    table.insert("extends".into(), toml::Value::String(extends.into()));
    if let toml::Value::Table(t) = over {
        table.extend(t);
    }
    Ok(table)
}

#[derive(Deserialize)]
pub struct ThemeIn {
    pub id: String,
    pub extends: String,
    pub overrides: Value,
}

pub fn post_theme(ctx: &Ctx, body: &[u8]) -> Response {
    let input: ThemeIn = match serde_json::from_slice(body) {
        Ok(i) => i,
        Err(e) => return Response::err(400, &e.to_string()),
    };
    let table = match build_theme_table(ctx, &input.id, &input.extends, &input.overrides) {
        Ok(t) => t,
        Err(e) => return Response::err(400, &e),
    };
    let text = match toml::to_string_pretty(&table) {
        Ok(t) => t,
        Err(e) => return Response::err(400, &e.to_string()),
    };
    let path = ctx.themes_dir().join(format!("{}.toml", input.id));
    match write_atomic(&ctx.dir, &path, text.as_bytes()) {
        Ok(()) => Response::json(200, &json!({ "ok": true, "id": input.id })),
        Err(e) => Response::err(500, &e),
    }
}

pub fn post_theme_delete(ctx: &Ctx, body: &[u8]) -> Response {
    #[derive(Deserialize)]
    struct In {
        id: String,
    }
    let Ok(i) = serde_json::from_slice::<In>(body) else { return Response::err(400, "requête invalide") };
    if !valid_name(&i.id) {
        return Response::err(400, "nom de thème invalide");
    }
    if Config::load_or_create(&ctx.config_path()).is_ok_and(|c| c.screen.iter().any(|s| s.theme == i.id)) {
        return Response::err(400, "ce thème est utilisé par un écran : changez-le d'abord");
    }
    let path = ctx.themes_dir().join(format!("{}.toml", i.id));
    if !path.is_file() {
        return Response::err(404, "thème utilisateur introuvable");
    }
    match std::fs::remove_file(&path) {
        Ok(()) => Response::ok(),
        Err(e) => Response::err(500, &e.to_string()),
    }
}

pub fn post_admin_key(body: &[u8]) -> Response {
    #[derive(Deserialize)]
    struct In {
        key: String,
    }
    let Ok(i) = serde_json::from_slice::<In>(body) else { return Response::err(400, "requête invalide") };
    match cred::set_admin_key(&i.key) {
        Ok(()) => Response::json(200, &json!({ "ok": true, "admin_key": true })),
        Err(e) => Response::err(400, &e),
    }
}

pub fn post_admin_key_clear() -> Response {
    match cred::clear_admin_key() {
        Ok(()) => Response::json(200, &json!({ "ok": true, "admin_key": false })),
        Err(e) => Response::err(500, &e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(ctx: &Ctx) -> Vec<String> {
        ctx.store().ids()
    }

    #[test]
    fn names() {
        assert!(valid_name("my-theme-2"));
        for bad in ["", "A", "a b", "a_b", "../x", "é", &"x".repeat(41)] {
            assert!(!valid_name(bad), "{bad}");
        }
    }

    #[test]
    fn atomic_write_stays_in_root() {
        let ctx = test_ctx();
        let p = ctx.dir.join("sub").join("a.toml");
        write_atomic(&ctx.dir, &p, b"x = 1").unwrap();
        write_atomic(&ctx.dir, &p, b"x = 2").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "x = 2");
        assert_eq!(std::fs::read_dir(p.parent().unwrap()).unwrap().count(), 1, "pas de .tmp résiduel");
        assert!(write_atomic(&ctx.dir, &ctx.dir.join("..").join("evil"), b"x").is_err());
        assert!(write_atomic(&ctx.dir, &std::env::temp_dir().join("evil.txt"), b"x").is_err());
    }

    #[test]
    fn config_roundtrip_through_api() {
        let ctx = test_ctx();
        let mut cfg = Config::default();
        cfg.screen[0].theme = "nord".into();
        cfg.screen[0].styles.insert("cpu".into(), forge_config::WidgetStyle::Ring);
        cfg.screen[0].show_when.insert("battery".into(), "battery<20".into());
        let body = serde_json::to_vec(&cfg).unwrap();
        assert_eq!(post_config(&ctx, &body).status, 200);
        let back = Config::from_toml(&std::fs::read_to_string(ctx.config_path()).unwrap()).unwrap();
        assert_eq!(back.screen[0].theme, "nord");
        assert_eq!(back.screen[0].show_when["battery"], "battery<20");
        assert_eq!(state(&ctx).status, 200);
    }

    #[test]
    fn invalid_configs_are_refused_and_not_written() {
        let ctx = test_ctx();
        let ids = ids(&ctx);
        let mut c = Config::default();
        c.screen[0].show_when.insert("battery".into(), "oops".into());
        assert!(check_config(&c, &ids).is_err());
        let mut c = Config::default();
        c.screen[0].theme = "inexistant".into();
        assert!(check_config(&c, &ids).is_err());
        let mut c = Config::default();
        c.screen[0].thickness = 3;
        assert_eq!(post_config(&ctx, &serde_json::to_vec(&c).unwrap()).status, 400);
        let mut c = Config::default();
        c.screen[0].layout.left = vec!["nimporte".into()];
        assert!(check_config(&c, &ids).is_err());
        assert_eq!(post_config(&ctx, b"{ pas du json").status, 400);
        assert!(!ctx.config_path().exists());
    }

    #[test]
    fn theme_save_writes_only_overrides() {
        let ctx = test_ctx();
        let body = json!({ "id": "mon-nord", "extends": "nord",
            "overrides": { "name": "Mon Nord", "colors": { "accent": "#FF0000" }, "radius": 6 } });
        assert_eq!(post_theme(&ctx, body.to_string().as_bytes()).status, 200);
        let text = std::fs::read_to_string(ctx.themes_dir().join("mon-nord.toml")).unwrap();
        assert!(text.contains("extends = \"nord\"") && text.contains("#FF0000"), "{text}");
        assert!(!text.contains("font_family"), "seules les valeurs modifiées sont écrites : {text}");
        let t = ctx.store().get("mon-nord");
        assert_eq!(t.colors.accent, "#FF0000");
        assert_eq!(t.colors.text, ctx.store().get("nord").colors.text);
    }

    #[test]
    fn theme_validation_errors() {
        let ctx = test_ctx();
        let go = |v: Value| post_theme(&ctx, v.to_string().as_bytes()).status;
        assert_eq!(go(json!({ "id": "a", "extends": "nord", "overrides": { "colors": { "accent": "rouge" } } })), 400);
        assert_eq!(go(json!({ "id": "a", "extends": "nord", "overrides": { "radius": 999 } })), 400);
        assert_eq!(go(json!({ "id": "a", "extends": "nord", "overrides": { "radius": null } })), 400);
        assert_eq!(go(json!({ "id": "a", "extends": "n-existe-pas", "overrides": {} })), 400);
        assert_eq!(go(json!({ "id": "nord", "extends": "nord", "overrides": {} })), 400);
        assert_eq!(go(json!({ "id": "a", "extends": "nord", "overrides": { "extends": "x" } })), 400);
        assert!(!ctx.themes_dir().join("a.toml").exists());
    }

    #[test]
    fn presets_cover_enum() {
        let p = presets_json();
        assert_eq!(p.as_array().unwrap().len(), PRESETS.len());
        let dbl = &p.as_array().unwrap()[6];
        assert_eq!(dbl["id"], "double_row");
        assert!(dbl["row2"].is_object());
        for id in PRESETS {
            assert!(serde_json::from_value::<LayoutPreset>(json!(id)).is_ok(), "{id}");
        }
    }
}
