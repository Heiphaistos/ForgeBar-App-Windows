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

Widgets disponibles : `desktop`, `apps` (miniatures des fenêtres ouvertes), `media`, `claude`, `cpu`, `ram`, `gpu`, `disk` (occupation du disque système), `fps` (images par seconde du jeu au premier plan), `temp` (températures CPU et GPU), `net`, `weather`, `clipboard`, `wifi`, `sound`, `battery`, `clock`, `toggle` (bouton qui masque la barre). Un widget sans donnée sur la machine (batterie sur un PC fixe, GPU sans compteur) n'apparaît pas. Quand la barre est trop étroite, les widgets les moins importants disparaissent (média, météo, réseau, GPU…) plutôt que de se chevaucher.

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

## Mise à jour automatique (0.6.0)

Au démarrage (après 30 s) puis toutes les 6 h, Forge Bar lit la dernière release GitHub. Si elle est plus récente, il télécharge le MSI de son architecture, **vérifie la signature Ed25519 de `SHA256SUMS.txt` (clé publique épinglée dans `updater.rs`, constante `UPDATE_PUBLIC_KEY_HEX`) puis son SHA-256 contre ce fichier signé** (refus si différent), l'installe en silence (MSI par utilisateur, sans droits administrateur) puis se relance. Seule la copie installée par le MSI s'installe seule ; une copie de développement ou un zip annonce la version disponible. Menu clic droit : « Rechercher une mise à jour ». Désactivable : `general.auto_update = false`.

**Clé de signature (à fournir par le propriétaire)** : tant que `UPDATE_PUBLIC_KEY_HEX` est vide, toute mise à jour est refusée. Générer une paire Ed25519, mettre la clé publique (64 caractères hexadécimaux) dans `crates/forge-bar/src/win/updater.rs`, et la graine privée (hexadécimal, 32 octets) dans le secret de dépôt `UPDATE_SIGNING_KEY` ; le workflow de release signe `SHA256SUMS.txt` (`scripts/sign-sums.py`) et publie `SHA256SUMS.txt.sig`, et échoue si le secret manque.

## FPS, températures et anti-triche (0.8.0)

Forge Bar est une fenêtre à part : **il n'injecte aucune DLL, ne pose aucun hook, ne charge aucun pilote noyau, n'ouvre aucun handle sur un autre processus et ne lit la mémoire d'aucun jeu**. C'est ce qui le rend compatible avec EAC, BattlEye, Vanguard, FACEIT et consorts. Ce qu'il fait côté système :

- **FPS** : session ETW (suivi d'événements Windows) qui compte les événements « Present » de DXGI et Direct3D 9 de l'application au premier plan, comme PresentMon. Passif. Windows réserve ETW aux administrateurs et au groupe « Utilisateurs du journal de performances » : sans ce droit le widget affiche « FPS — » et le clic droit propose de l'ajouter (une validation administrateur, effective à la prochaine ouverture de session). Vulkan pur n'est pas compté.
- **Températures** : GPU NVIDIA via `nvml.dll` (la bibliothèque de `nvidia-smi`, dossier système uniquement) ; processeur via les zones thermiques ACPI quand la machine en expose. Aucun pilote de type WinRing0 / LibreHardwareMonitor : ils sont justement bloqués par les anti-triche, on n'en charge jamais.
- **Mode jeu** : quand une fenêtre plein écran sans cadre est au premier plan, la barre cesse d'interroger les autres fenêtres (miniatures d'applications) et ne leur envoie plus aucun message.
- Aucune garantie contractuelle n'existe côté éditeurs d'anti-triche ; en cas de doute avec un jeu précis, masquez la barre (Ctrl+Alt+B) ou désactivez les widgets `fps` et `apps`.

Journal : `%APPDATA%\ForgeBar\logsorge-bar.log` (widgets masqués faute de place, etc.). La barre surveille aussi le dossier des thèmes : modifier un thème enregistré s'applique sans toucher à la configuration.

## Disposition : les nouveaux widgets apparaissent tout seuls (0.9.0)

Une disposition écrite à la main (glisser-déposer) ne recevait jamais les widgets créés après elle : après une mise à jour, les FPS, les températures, le disque ou les miniatures restaient dans la réserve, invisibles. Forge Bar ajoute maintenant automatiquement ces widgets (`layout.seen` retient ceux que la disposition connaît déjà ; un widget retiré ensuite à la main reste retiré). Dans l'éditeur, un clic sur un widget de la réserve l'ajoute à la barre, et « Tout ajouter à la barre » les place tous.

## Fenêtres ouvertes et barre en jeu (0.10.0)

- **Miniatures d'applications** : mêmes règles que la barre des tâches de Windows (ni fenêtres outils, ni surcouches qui ne prennent jamais le focus, ni les fenêtres de Forge Bar), jusqu'à 16 fenêtres. Les applications UWP (Calculatrice, Paramètres…) ont leur vraie icône, retrouvée par le shell. Un clic sur une fenêtre élevée (Gestionnaire des tâches) fonctionne aussi depuis une barre non élevée.
- **Barre visible en jeu** : clic droit sur la barre, « Afficher la barre dans les jeux » (équivalent de `general.hide_on_fullscreen = false`). La barre est alors remise au-dessus de la fenêtre du jeu chaque seconde. Elle s'affiche par-dessus les jeux en fenêtré, sans bordure ou en plein écran « optimisé » de Windows ; un vrai plein écran exclusif la recouvre, comme n'importe quelle autre fenêtre.

## Nouveaux widgets et widgets personnalisés (0.12.0)

**Widgets facultatifs** (à placer depuis Disposition ; jamais ajoutés d'office) : `date` (jeu. 15 oct.), `uptime` (temps depuis le démarrage), `memory` (RAM en Go), `ping` (latence vers Cloudflare), `diskio` (activité des disques), `keyboard` (FR/EN…), `mic` (clic : couper le micro), `timer` (chronomètre ou minuteur : clic = démarrer/pause, clic droit = minuteur 5 à 60 min, remise à zéro ; une bulle prévient à la fin).

**Widgets personnalisés** : page « Widgets personnalisés » de l'éditeur, ou à la main dans `config.toml` :

```toml
[[custom]]
id = "git"            # minuscules, chiffres, _ et - ; s'utilise comme un widget intégré dans left/center/right
kind = "command"      # launcher (ouvre `target`), command (affiche la 1re ligne de la sortie), text
label = "Git"
icon = ""             # emoji ou code Segoe Fluent Icons (E80F)
command = "git -C C:\Projets\Depot status -sb"
interval_secs = 30    # 2 à 86400
target = ""           # ouvert au clic (programme, dossier, adresse web)
```

Une commande s'exécute avec vos droits (`cmd /C`), 8 secondes au plus, uniquement si le widget est dans une disposition. Aucun processus ne s'accumule.

**FPS** : les présentations de test (`DXGI_PRESENT_TEST`, faites par beaucoup de jeux à chaque image) et celles des surcouches (une chaîne d'échange à part) ne sont plus comptées : seules celles de la chaîne la plus active du jeu au premier plan. Corrige l'écart avec le compteur du jeu. (Avec la génération d'images — DLSS/FSR Frame Generation — le compteur de Forge Bar affiche les images réellement présentées, donc davantage que les images rendues par le jeu.)

**Plein écran exclusif** : Windows ne permet pas d'afficher une fenêtre par-dessus un vrai plein écran exclusif sans que le jeu en sorte et repasse en fenêtré. Avec « Afficher la barre dans les jeux », Forge Bar reste visible par-dessus un jeu en plein écran **sans bordure** ou **fenêtré**, mais se **retire tout seul** dès qu'un jeu passe en plein écran **exclusif** (ou que le plein écran se met à basculer en boucle) et revient 3 secondes après la fin du jeu : plus de clignotement ni de jeu forcé en fenêtré. Pour garder la barre à l'écran en jeu, choisissez « Plein écran fenêtré » / « Sans bordure » dans les options graphiques du jeu.
