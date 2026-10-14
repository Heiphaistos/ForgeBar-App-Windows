# Forge Bar

Barre d'extension pour Windows 10 et 11 (x86 et x64) : raccourcis système, monitoring et HUD de consommation Claude. Un seul exécutable natif de moins de 1 Mo, sans runtime ni WebView.

## État du projet

| Jalon | Contenu | État |
| --- | --- | --- |
| 1 · Socle | Barre AppBar réservée ou superposée, masquage auto, multi-écrans, rendu Direct2D, Mica/Acrylic, config TOML à chaud, CI x86/x64 | Codé, à tester sous Windows |
| 2 · Widgets système | Bureau, Wi-Fi, son (clic = sourdine, molette = volume), CPU, RAM, batterie, horloge | Codé, à tester sous Windows |
| 2 · Widgets système (suite) | GPU, débit réseau, calendrier déroulant | À faire |
| 3 · HUD Claude | Logs Claude Code : contexte, tokens, coût jour/semaine/mois, budgets, alertes de couleur | Codé, à tester sous Windows |
| 3 · HUD Claude (suite) | Panneau détaillé, panneau flottant, notifications, Admin API, limites Pro/Max | À faire |
| 4 · Personnalisation | 50 thèmes, héritage `extends`, 6 dispositions, réglages par écran | Codé, à tester sous Windows |
| 4 · Personnalisation (suite) | Éditeur visuel, dispositions double ligne et dock vertical avancé, packs d'icônes | À faire |
| 5 · Finition | Météo, média, presse-papiers, installateur MSI et winget | À faire |

## Installation

1. Téléchargez `ForgeBar-<version>-x64.zip` (ou `x86` pour Windows 10 32 bits) depuis les releases.
2. Décompressez et lancez `forge-bar.exe`. Aucun droit administrateur n'est nécessaire.
3. Clic droit sur la barre : paramètres, dossier des thèmes, recharger, quitter.

Au premier lancement, `%APPDATA%\ForgeBar\config.toml` est créé. Toute modification s'applique dans les 2 secondes.

## Configuration

```toml
[general]
autostart = true            # lancement avec Windows (clé Run de l'utilisateur)
hide_on_fullscreen = true

[[screen]]
id = "primary"              # primary, all, ou numéro d'écran ("2")
edge = "top"                # top, bottom, left, right
mode = "reserved"           # reserved (comme la barre des tâches) ou overlay
autohide = false            # en mode overlay uniquement
theme = "mica-dark"
thickness = 32
margin = 0                  # > 0 : barre flottante, coins arrondis selon le thème

[screen.layout]
preset = "split"            # classic, centered, split, islands, compact, hud_first, custom
# Zones à la main (prioritaires sur le préréglage) :
# left = ["desktop"]
# center = ["claude"]
# right = ["cpu", "ram", "wifi", "sound", "battery", "clock"]

[claude]
sources = ["claude_code"]
bar_summary = ["context", "cost_today"]   # + tokens_today, cost_week, cost_month
alerts = [80, 95]                         # seuils orange et rouge, en %
context_window = 200000
refresh_seconds = 60

[claude.budgets]
daily_usd = 5.0
monthly_usd = 100.0
```

Widgets disponibles : `desktop`, `wifi`, `sound`, `cpu`, `ram`, `battery`, `clock`, `claude`. Un clic sur le widget Claude affiche aussi les coûts de la semaine et du mois.

La grille de prix (`[[claude.prices]]`) est écrite dans le fichier au premier lancement. Vérifiez-la sur la page de tarifs d'Anthropic : le coût affiché n'est qu'une estimation à partir des tokens. Un modèle absent de la grille est compté en tokens, sans coût.

### En ligne de commande

```
forge-settings themes              liste les thèmes
forge-settings theme nord          applique un thème à l'écran 1
forge-settings layout islands 2    change la disposition de l'écran 2
forge-settings edge bottom
```

## Thèmes

50 thèmes intégrés. Copiez-en un dans `%APPDATA%\ForgeBar\themes` pour le modifier, ou créez un fichier qui n'en change que quelques valeurs :

```toml
# %APPDATA%\ForgeBar\themes\mon-nord.toml
extends = "nord"
name = "Mon Nord"
radius = 10.0

[background]
kind = "linear"
colors = ["#2E3440", "#3B4252", "#5E81AC"]
angle = 0.0
opacity = 0.9

[colors]
accent = "#EBCB8B"
```

| Famille | Nombre | Identifiants |
| --- | --- | --- |
| Windows natif | 8 | `acrylic-dark`, `acrylic-light`, `fluent-dark`, `fluent-light`, `high-contrast`, `mica-dark`, `mica-light`, `windows-10` |
| Dégradés | 8 | `aurora`, `ember`, `forest`, `lavender`, `midnight`, `ocean`, `peach`, `sunset` |
| Néon et gaming | 8 | `crimson`, `cyberpunk`, `ice-neon`, `matrix`, `plasma`, `rgb`, `synthwave`, `toxic` |
| Palettes de développeur | 8 | `catppuccin`, `dracula`, `gruvbox`, `monokai`, `nord`, `one-dark`, `solarized`, `tokyo-night` |
| Glass | 6 | `glass-clear`, `glass-emerald`, `glass-frost`, `glass-mica`, `glass-rose`, `glass-smoke` |
| Minimal | 6 | `ink`, `mono-dark`, `mono-light`, `slate`, `transparent-dark`, `transparent-light` |
| Rétro | 6 | `amber-terminal`, `classic-grey`, `green-terminal`, `paper`, `pixel`, `y2k` |

Mica et Acrylic demandent Windows 11 22H2 ou plus. Sous Windows 10, ces thèmes gardent leur fond teinté semi-transparent, sans flou.

## Compiler

```powershell
rustup target add x86_64-pc-windows-msvc i686-pc-windows-msvc
cargo test --workspace
cargo build --release --target x86_64-pc-windows-msvc
cargo build --release --target i686-pc-windows-msvc
```

Le CRT est lié statiquement (`.cargo/config.toml`) : aucun redistribuable Visual C++ n'est requis. La compilation croisée depuis Linux fonctionne aussi avec MinGW-w64 (cibles `*-pc-windows-gnu`).

Les thèmes de `themes/` sont générés par `scripts/gen_themes.py` et intégrés au binaire à la compilation.

## Architecture

| Crate | Rôle |
| --- | --- |
| `forge-bar` | Exécutable : fenêtres, AppBar, DWM, rendu Direct2D/DirectWrite, mesures système |
| `forge-config` | `config.toml`, thèmes, héritage, dispositions |
| `forge-claude` | Lecture des logs Claude Code, agrégats, coûts, budgets |
| `forge-settings` | Paramètres en ligne de commande (l'éditeur visuel viendra au jalon 4) |

Une fenêtre de messages invisible porte les minuteurs : mesures toutes les secondes (en pause quand toutes les barres sont masquées), surveillance de la configuration toutes les 2 secondes. Les logs Claude sont lus sur un fil séparé. La barre ne se redessine que sur ces événements.

## Limites connues

- Pas encore testé sur une vraie machine : le code compile et passe clippy en x86 et x64, mais le rendu et l'AppBar restent à valider sous Windows 10 et 11.
- `wlanapi.dll` est requise. Elle est présente sur Windows 10 et 11, mais absente de Windows Server sans le service WLAN.
- En vertical (bord gauche ou droit), les widgets avec icône n'affichent que l'icône.
- Les limites d'abonnement Pro / Max n'ont pas d'API publique ; elles arriveront comme module optionnel.

## Publier sur GitHub

```powershell
.\scripts\publish-github.ps1 -Visibility private
```

Le script crée le dépôt avec GitHub CLI et pousse le code. Un tag `v0.1.0` déclenche ensuite la release avec les archives x86 et x64.
