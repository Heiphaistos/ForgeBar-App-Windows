# Forge Bar

Barre d'extension pour Windows 10 et 11 (x86 et x64) : raccourcis système, monitoring et HUD de consommation Claude. Un seul exécutable natif d'environ 1,4 Mo, sans runtime ni WebView. L'éditeur de paramètres est un second petit exécutable qui s'ouvre dans le navigateur, uniquement quand on le lance.

## Fonctionnalités

| Domaine | Contenu |
| --- | --- |
| Barre | AppBar réservée (comme la barre des tâches) ou superposée, masquage auto, multi-écrans, 4 bords, double rangée, dock vertical, rendu Direct2D, Mica / Acrylic, configuration relue à chaud |
| Widgets système | Bureau, Wi-Fi, son (clic = sourdine, molette = volume, rappels Windows sans interrogation), CPU, RAM, GPU, débit réseau, batterie, horloge (clic = calendrier), météo (Open-Meteo), média en cours (clic = lecture / pause), presse-papiers (historique en mémoire) |
| HUD Claude | Contexte de la session, tokens, coût estimé jour / semaine / mois, budgets, fenêtre de 5 h estimée, coûts réels de l'organisation (Admin API), panneau détaillé au clic, panneau flottant transparent aux clics, notifications Windows aux seuils |
| Personnalisation | 50 thèmes, héritage `extends`, 7 dispositions, styles par widget (texte, jauge, anneau, mini-graphe), affichage conditionnel, packs d'icônes SVG, accent tiré du fond d'écran, éditeur visuel |
| Distribution | Zip x86 / x64, MSI par utilisateur (sans droits administrateur), manifeste winget généré à chaque release |

## Installation

1. Téléchargez le MSI (`ForgeBar-<version>-x64.msi`, ou `x86` pour Windows 10 32 bits) ou le zip depuis les releases.
2. MSI : double-clic, installation dans `%LOCALAPPDATA%\Programs\ForgeBar`, raccourcis dans le menu Démarrer. Zip : décompressez et lancez `forge-bar.exe`.
3. Clic droit sur la barre : paramètres, dossier des thèmes, recharger, quitter. Le même menu est dans la zone de notification.

Les binaires ne sont pas signés : SmartScreen peut demander une confirmation au premier lancement.

Au premier lancement, `%APPDATA%\ForgeBar\config.toml` est créé. Toute modification s'applique dans les 2 secondes.

## Éditeur de paramètres

`forge-settings` (sans argument) ouvre l'éditeur dans le navigateur : galerie de thèmes avec aperçu réel, éditeur de thème (couleurs, dégradé, bordure, police), glisser-déposer des widgets entre zones et rangées, styles et conditions par widget, écrans, Claude, météo, presse-papiers. Il écoute uniquement `127.0.0.1`, avec un jeton aléatoire, et n'écrit que dans `%APPDATA%\ForgeBar`.

```
forge-settings                     éditeur visuel
forge-settings themes              liste les thèmes
forge-settings theme nord          applique un thème à l'écran 1
forge-settings layout islands 2    change la disposition de l'écran 2
forge-settings edge bottom
forge-settings admin-key set       enregistre la clé Admin API (Gestionnaire d'identification)
```

## Configuration

```toml
[general]
autostart = true            # lancement avec Windows (clé Run de l'utilisateur)
hide_on_fullscreen = true
accent_from_wallpaper = false   # accent = couleur dominante du fond d'écran
icon_pack = ""              # "forge" (intégré) ou dossier de %APPDATA%\ForgeBar\icons ; vide = glyphes Segoe

[[screen]]
id = "primary"              # primary, all, ou numéro d'écran ("2")
edge = "top"                # top, bottom, left, right
mode = "reserved"           # reserved (comme la barre des tâches) ou overlay
autohide = false            # en mode overlay uniquement
theme = "mica-dark"
thickness = 32              # une double rangée passe automatiquement à 52 au minimum
margin = 0                  # > 0 : barre flottante, coins arrondis selon le thème

[screen.layout]
preset = "split"            # classic, centered, split, islands, compact, hud_first, double_row, custom
# Zones à la main (prioritaires sur le préréglage) :
# left = ["desktop", "media"]
# center = ["claude"]
# right = ["cpu", "ram", "gpu", "net", "weather", "wifi", "sound", "battery", "clock"]
# [screen.layout.row2]      # deuxième rangée
# left = ["weather"]

[screen.styles]             # text (défaut), gauge, ring, graph
cpu = "ring"
ram = "graph"

[screen.show_when]          # always, media, ou métrique<valeur / métrique>valeur
battery = "battery<20"      # métriques : battery, cpu, ram, gpu, claude
media = "media"

[claude]
sources = ["claude_code"]   # + "api" pour les coûts réels de l'organisation
bar_summary = ["context", "cost_today"]   # + tokens_today, cost_week, cost_month, five_hour, org_today, org_month
alerts = [80, 95]           # seuils orange et rouge, en %
context_window = 200000     # au-delà, la fenêtre est supposée de 1 M de tokens
refresh_seconds = 60
notifications = true        # bulle Windows quand un seuil ou un budget est franchi

[claude.budgets]
daily_usd = 5.0
monthly_usd = 100.0

[claude.subscription]       # estimation de la fenêtre de 5 h des abonnements Pro / Max
enabled = false
five_hour_limit_usd = 8.0   # budget que vous fixez : aucune API publique ne donne la limite réelle

[claude.floating]           # panneau flottant toujours visible
enabled = false
corner = "top_right"        # top_left, top_right, bottom_left, bottom_right
opacity = 0.85
click_through = true

[weather]
enabled = false
city = "Lyon"               # ou latitude / longitude
unit = "celsius"

[clipboard]
enabled = false
history = 10
```

Widgets disponibles : `desktop`, `apps` (miniatures des fenêtres ouvertes), `media`, `claude`, `cpu`, `ram`, `gpu`, `disk` (occupation du disque système), `net`, `weather`, `clipboard`, `wifi`, `sound`, `battery`, `clock`, `toggle` (bouton qui masque la barre). Un widget sans donnée sur la machine (batterie sur un PC fixe, GPU sans compteur) n'apparaît pas. Quand la barre est trop étroite, les widgets les moins importants disparaissent (média, météo, réseau, GPU…) plutôt que de se chevaucher.

La grille de prix (`[[claude.prices]]`) est écrite dans le fichier au premier lancement. Vérifiez-la sur la page de tarifs d'Anthropic : le coût affiché n'est qu'une estimation à partir des tokens. Un modèle absent de la grille est compté en tokens, sans coût.

### Admin API d'Anthropic

Les coûts réels de l'organisation demandent une clé Admin (`sk-ant-admin…`). `forge-settings admin-key set` la range dans le Gestionnaire d'identification Windows, jamais dans `config.toml`. Ajoutez `"api"` à `claude.sources`, puis `org_today` ou `org_month` à `bar_summary`. L'appel est fait au plus toutes les 5 minutes.

### Packs d'icônes

Un pack est un dossier `%APPDATA%\ForgeBar\icons\<nom>\` avec un SVG par widget : `desktop`, `wifi`, `volume`, `mute`, `battery`, `clock`, `cpu`, `ram`, `gpu`, `disk`, `net`, `weather`, `media`, `clipboard`, `claude`. Sont lus : `path`, `circle`, `rect`, `ellipse`, `line`, `polyline`, `polygon`, remplissage ou contour en `currentColor`. Pas de dégradés, de transformations ni de CSS. Le pack intégré `forge` sert d'exemple (`assets/icons/forge`).

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

Le CRT est lié statiquement (`.cargo/config.toml`) : aucun redistribuable Visual C++ n'est requis.

Aperçus hors écran, sans fenêtre : `forge-bar --snapshot out.png --theme nord [--width 1400] [--preset double_row] [--scene calendar|claude|clipboard|floating]`. `forge-bar --snapshot-all dossier` dessine les 50 thèmes (horizontal et vertical) avec des données de démonstration, écrit `report.txt` et sort en erreur si un rendu est vide : c'est le test visuel de la CI.

MSI : `dotnet tool install --global wix --version 5.0.2`, puis
`wix build -arch x64 -d Version=0.2.0 -d BinDir=target\release -d RepoDir=. installer\forge-bar.wxs -o ForgeBar-0.2.0-x64.msi`.

## Architecture

| Crate | Rôle |
| --- | --- |
| `forge-bar` | Exécutable : fenêtres, AppBar, DWM, rendu Direct2D/DirectWrite, mesures système, panneaux, aperçus hors écran |
| `forge-config` | `config.toml`, thèmes, héritage, dispositions, conditions d'affichage |
| `forge-claude` | Lecture incrémentale des logs Claude Code, coûts, budgets, fenêtre de 5 h, Admin API (sans réseau), seuils de notification |
| `forge-settings` | Éditeur visuel (serveur local + page embarquée), CLI, clé Admin API |

Une fenêtre de messages invisible porte les minuteurs : mesures toutes les secondes (en pause quand toutes les barres sont masquées), surveillance de la configuration toutes les 2 secondes. Les logs Claude, la météo et le média sont lus sur des fils séparés. La barre ne se redessine que sur ces événements.

## Limites connues

- Les logs Claude Code peuvent peser plusieurs gigaoctets : la première lecture prend quelques secondes (à priorité réduite), les suivantes ne lisent que les lignes ajoutées.
- Les limites d'abonnement Pro / Max n'ont pas d'API publique : la fenêtre de 5 h est une estimation à partir de vos propres logs et d'un budget que vous fixez.
- Le coût affiché est une estimation à partir des tokens et de la grille de prix du fichier de configuration.
- `wlanapi.dll` est requise. Elle est présente sur Windows 10 et 11, mais absente de Windows Server sans le service WLAN.
- Les icônes de la zone de notification utilisent l'icône d'application générique de Windows.
- Les binaires ne sont pas signés.

## Clic droit, masquage, thèmes adaptatifs (0.5.0)

- **Clic droit sur un widget** : menu propre au widget (son : couper et 11 paliers de volume ; réseau : test de débit Cloudflare, latence, descendant, montant ; disque, CPU, Wi-Fi, batterie, média, horloge, miniature d'application : activer, réduire, fermer). Le bas du menu est commun : masquer la barre, personnaliser, thèmes, recharger, quitter.
- **Masquer / afficher la barre** : widget `toggle`, icône de notification (clic gauche), raccourci **Ctrl + Alt + B**, ou le menu. L'espace réservé est rendu au bureau tant qu'elle est masquée.
- **Thèmes adaptatifs** : `adaptive = "system"` (mode clair/sombre et couleur d'accentuation de Windows) ou `adaptive = "wallpaper"` (teinte du fond d'écran). Recalculés dès que Windows change de mode ou de couleur. Thèmes fournis : `auto-windows`, `auto-fond`.
- **Éditeur de réglages** : aperçu en direct (chaque changement est appliqué à la vraie barre), bouton Rétablir.
