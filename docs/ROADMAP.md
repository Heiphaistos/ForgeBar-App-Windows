# Roadmap détaillée

Le cahier des charges complet est dans le doc Forge Bar. Ce fichier suit ce qui reste à coder.

## Jalon 2 · Widgets système (suite)
- [ ] GPU : compteurs PDH « GPU Engine »
- [ ] Réseau : débit montant et descendant avec GetIfTable2
- [ ] Calendrier mensuel au clic sur l'horloge
- [ ] Notification audio IMMNotificationClient au lieu de l'interrogation chaque seconde

## Jalon 3 · HUD Claude (suite)
- [ ] Panneau détaillé au clic (graphes, historique, sources)
- [ ] Panneau flottant toujours visible, opacité et clic à travers
- [ ] Notifications Windows aux seuils
- [ ] Admin API Anthropic (clé dans le Gestionnaire d'identification)
- [ ] Limites Pro / Max : module optionnel isolé
- [ ] Lecture incrémentale des logs (décalage par fichier)

## Jalon 4 · Personnalisation (suite)
- [ ] Éditeur visuel : galerie avec aperçu, sélecteurs de couleur, éditeur de dégradé
- [ ] Glisser-déposer des widgets entre zones
- [ ] Dispositions double ligne et dock vertical avancé
- [ ] Affichage conditionnel (batterie sous 20 %, média pendant la lecture)
- [ ] Styles par widget : jauge, anneau, mini-graphe
- [ ] Packs d'icônes SVG, couleur extraite du fond d'écran

## Jalon 5 · Finition
- [ ] Météo (Open-Meteo), média (GlobalSystemMediaTransportControls), presse-papiers
- [ ] Installateur MSI et manifeste winget
- [ ] Tests visuels des 50 thèmes en CI
