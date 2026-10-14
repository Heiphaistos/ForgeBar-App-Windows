//! Paramètres de Forge Bar : éditeur visuel dans le navigateur (serveur local sur 127.0.0.1)
//! et sous-commandes en ligne de commande. Aucun WebView, aucun runtime.

mod api;
mod cred;
mod http;
mod previews;

use forge_config::{config_path, user_themes_dir, Config, ThemeStore};
use std::net::TcpListener;
use std::process::ExitCode;
use std::sync::Arc;

fn usage() {
    println!(
        "forge-settings — paramètres de Forge Bar

  forge-settings [gui]              ouvre l'éditeur visuel dans le navigateur
  forge-settings themes             liste les thèmes disponibles
  forge-settings theme <id> [n]     applique un thème à l'écran n (1 par défaut)
  forge-settings layout <préréglage> [n]
                                    classic, centered, split, islands, compact, hud_first, double_row
  forge-settings edge <top|bottom|left|right> [n]
  forge-settings admin-key set      enregistre la clé Admin API (lue sur stdin, jamais affichée)
  forge-settings admin-key clear    supprime la clé
  forge-settings admin-key status   indique si une clé est enregistrée
  forge-settings path               affiche le chemin du fichier

Variable d'environnement : FORGE_SETTINGS_NO_BROWSER=1 n'ouvre pas le navigateur (l'adresse est affichée)."
    );
}

fn edit(f: impl FnOnce(&mut Config, usize) -> Result<(), String>, screen: Option<&String>) -> Result<(), String> {
    let path = config_path();
    let mut cfg = Config::load_or_create(&path).map_err(|e| e.to_string())?;
    let n: usize = screen.map(|s| s.parse().map_err(|_| "numéro d'écran invalide")).transpose()?.unwrap_or(1);
    if n == 0 || n > cfg.screen.len() {
        return Err(format!("écran {n} absent de la configuration ({} défini(s))", cfg.screen.len()));
    }
    f(&mut cfg, n - 1)?;
    cfg.validate().map_err(|e| e.to_string())?;
    api::write_atomic(&forge_config::app_dir(), &path, cfg.to_toml().as_bytes())?;
    println!("Enregistré. La barre se met à jour d'elle-même.");
    Ok(())
}

fn open(target: &str) -> Result<(), String> {
    #[cfg(windows)]
    let r = std::process::Command::new("cmd").args(["/C", "start", ""]).arg(target).status();
    #[cfg(not(windows))]
    let r = std::process::Command::new("xdg-open").arg(target).status();
    r.map(|_| ()).map_err(|e| e.to_string())
}

fn gui() -> Result<(), String> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| format!("écoute impossible : {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let (token, nonce) = (cred::random_hex(24)?, cred::random_hex(12)?);
    let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf()));
    let dir = forge_config::app_dir();
    Config::load_or_create(&config_path()).map_err(|e| e.to_string())?;
    let ctx = Arc::new(api::Ctx::new(
        port,
        token.clone(),
        nonce,
        dir,
        std::env::temp_dir().join("forgebar-previews"),
        exe_dir,
    ));
    let url = format!("http://127.0.0.1:{port}/?t={token}");
    println!("Éditeur Forge Bar : {url}");
    println!("(fermeture automatique après 30 minutes d'inactivité)");
    if std::env::var("FORGE_SETTINGS_NO_BROWSER").as_deref() != Ok("1") {
        open(&url)?;
    }
    http::serve(listener, ctx).map_err(|e| e.to_string())
}

fn admin_key(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("set") => {
            let key = cred::read_secret("Clé Admin API Anthropic : ")?;
            cred::set_admin_key(&key)?;
            println!("Clé enregistrée dans le Gestionnaire d'identification.");
            Ok(())
        }
        Some("clear") => {
            cred::clear_admin_key()?;
            println!("Clé supprimée.");
            Ok(())
        }
        Some("status") => {
            println!("{}", if cred::admin_key_exists() { "clé enregistrée" } else { "aucune clé" });
            Ok(())
        }
        _ => Err("usage : forge-settings admin-key set|clear|status".into()),
    }
}

fn run(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        None | Some("gui") => gui(),
        Some("path") => {
            println!("{}", config_path().display());
            Ok(())
        }
        Some("admin-key") => admin_key(&args[1..]),
        Some("themes") => {
            let store = ThemeStore::load(Some(&user_themes_dir()));
            for id in store.ids() {
                let t = store.get(&id);
                println!("{id:<20} {:<28} {}", t.name, t.family);
            }
            Ok(())
        }
        Some("theme") => {
            let id = args.get(1).ok_or("indiquez l'identifiant du thème")?.clone();
            let store = ThemeStore::load(Some(&user_themes_dir()));
            if !store.ids().contains(&id) {
                return Err(format!("thème inconnu : {id} (voir `forge-settings themes`)"));
            }
            edit(
                |c, i| {
                    c.screen[i].theme = id;
                    Ok(())
                },
                args.get(2),
            )
        }
        Some("layout") => {
            let p = args.get(1).ok_or("indiquez un préréglage")?;
            let preset = toml::Value::String(p.clone()).try_into().map_err(|_| format!("préréglage inconnu : {p}"))?;
            edit(
                |c, i| {
                    c.screen[i].layout.preset = preset;
                    Ok(())
                },
                args.get(2),
            )
        }
        Some("edge") => {
            let e = args.get(1).ok_or("indiquez un bord")?;
            let edge = toml::Value::String(e.clone()).try_into().map_err(|_| format!("bord inconnu : {e}"))?;
            edit(
                |c, i| {
                    c.screen[i].edge = edge;
                    Ok(())
                },
                args.get(2),
            )
        }
        _ => {
            usage();
            Ok(())
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Erreur : {e}");
            ExitCode::FAILURE
        }
    }
}
