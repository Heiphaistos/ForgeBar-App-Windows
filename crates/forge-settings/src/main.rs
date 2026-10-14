//! Paramètres de Forge Bar.
//!
//! Jalon 1 : outil en ligne de commande qui liste les thèmes, en applique un et
//! ouvre `config.toml` dans l'éditeur par défaut. La barre recharge le fichier à
//! chaud. L'éditeur visuel complet (galerie, dégradés, dispositions) arrive au jalon 4.

use forge_config::{config_path, user_themes_dir, Config, ThemeStore};
use std::process::ExitCode;

fn usage() {
    println!(
        "forge-settings — paramètres de Forge Bar

  forge-settings                    ouvre config.toml
  forge-settings themes             liste les thèmes disponibles
  forge-settings theme <id> [n]     applique un thème à l'écran n (1 par défaut)
  forge-settings layout <préréglage> [n]
                                    classic, centered, split, islands, compact, hud_first
  forge-settings edge <top|bottom|left|right> [n]
  forge-settings path               affiche le chemin du fichier"
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
    std::fs::write(&path, cfg.to_toml()).map_err(|e| e.to_string())?;
    println!("Enregistré. La barre se met à jour d'elle-même.");
    Ok(())
}

fn open(path: &std::path::Path) -> Result<(), String> {
    #[cfg(windows)]
    let r = std::process::Command::new("cmd").args(["/C", "start", ""]).arg(path).status();
    #[cfg(not(windows))]
    let r = std::process::Command::new("xdg-open").arg(path).status();
    r.map(|_| ()).map_err(|e| e.to_string())
}

fn run(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        None => {
            let path = config_path();
            Config::load_or_create(&path).map_err(|e| e.to_string())?;
            open(&path)
        }
        Some("path") => {
            println!("{}", config_path().display());
            Ok(())
        }
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
