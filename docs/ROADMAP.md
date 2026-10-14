# Roadmap

Les cinq jalons de départ sont codés. Ce qui a été vérifié sur une vraie machine Windows 11 (barre réservée, calendrier, HUD flottant, MSI, aperçus, éditeur) est dit dans le README ; le reste des vérifications est suivi ici.

## Fait
- [x] Socle : AppBar, Direct2D, Mica / Acrylic, multi-écrans, configuration à chaud
- [x] Widgets : GPU (compteurs PDH), réseau (GetIfTable2), calendrier, météo, média, presse-papiers, son par notifications Windows
- [x] HUD Claude : panneau détaillé, panneau flottant, notifications, Admin API, fenêtre de 5 h estimée, lecture incrémentale des logs
- [x] Personnalisation : éditeur visuel, glisser-déposer, double rangée, dock vertical en pile, affichage conditionnel, styles par widget, packs SVG, accent du fond d'écran
- [x] Finition : MSI par utilisateur, manifeste winget, tests visuels des 50 thèmes

## À vérifier en conditions réelles
- [ ] Admin API avec une vraie clé (analyse testée sur un exemple de la documentation)
- [ ] Météo, média, presse-papiers et notifications sur une session utilisée au quotidien
- [ ] Windows 10 (Mica / Acrylic : repli teinté) et écrans multiples à DPI différents
- [ ] Soumission du manifeste winget à microsoft/winget-pkgs

## Idées
- [ ] Icône propre pour la zone de notification
- [ ] Signature des binaires et du MSI
- [ ] Thèmes générés depuis le fond d'écran (au-delà de l'accent)
