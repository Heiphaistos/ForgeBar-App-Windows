// ---------- page : disposition (glisser-déposer) ----------
const ZONES = [['left', 'Gauche'], ['center', 'Centre'], ['right', 'Droite']];
let L = null; // { hand, rows: [zones, zones|null] }
let drag = null;
let wsel = null; // widget sélectionné
const WNAMES = { desktop: 'Bureau', media: 'Média', claude: 'Claude', cpu: 'CPU', ram: 'RAM', gpu: 'GPU', disk: 'Disque', fps: 'Images par seconde (FPS)', temp: 'Températures', apps: 'Applications ouvertes', toggle: 'Bouton masquer', net: 'Réseau', weather: 'Météo', clipboard: 'Presse-papiers', wifi: 'Wi-Fi', sound: 'Son', battery: 'Batterie', clock: 'Horloge' };
const wname = (w) => WNAMES[w] || w;
const PRESET_NAMES = { classic: 'Classique', centered: 'Centré', split: 'Réparti', islands: 'Îlots', compact: 'Compact', hud_first: 'HUD Claude en premier', double_row: 'Deux rangées', custom: 'Personnalisé' };

function layoutLoad() {
  const lay = cfg.screen[sel].layout, p = S.presets.find((x) => x.id === lay.preset);
  const mk = (z) => ({ left: [...z.left], center: [...z.center], right: [...z.right] });
  const some = (z) => ZONES.some(([k]) => z[k].length);
  if (some(lay) || some(lay.row2)) {
    L = { hand: true, rows: [mk(lay), some(lay.row2) ? mk(lay.row2) : p && p.row2 ? mk(p.row2) : null] };
  } else L = { hand: false, rows: [mk(p.zones), p.row2 ? mk(p.row2) : null] };
}
function layoutCommit() {
  const lay = cfg.screen[sel].layout, empty = { left: [], center: [], right: [] };
  const [r1, r2] = L.hand ? L.rows : [empty, null];
  for (const [k] of ZONES) { lay[k] = [...r1[k]]; lay.row2[k] = r2 ? [...r2[k]] : []; }
  touch();
}
function usedWidgets() { return L.rows.flatMap((r) => (r ? ZONES.flatMap(([k]) => r[k]) : [])); }
function moveWidget(w, row, zone, idx) {
  for (const r of L.rows) if (r) for (const [k] of ZONES) r[k] = r[k].filter((x) => x !== w);
  if (row != null) {
    const list = L.rows[row][zone];
    list.splice(Math.min(idx, list.length), 0, w);
  }
  L.hand = true;
  layoutCommit();
  wsel = w;
  setTab('layout');
}
function dropIndex(zoneEl, e) {
  const chips = [...zoneEl.querySelectorAll('.w')];
  const i = chips.findIndex((c) => { const r = c.getBoundingClientRect(); return e.clientY < r.bottom && e.clientX < r.left + r.width / 2; });
  return i < 0 ? chips.length : i;
}
function chip(w, pool) {
  const c = h('span', { class: 'w' + (wsel === w ? ' on' : ''), draggable: true, tabindex: 0, role: 'button', title: pool ? "Cliquer pour l'ajouter à la barre (ou le glisser dans une zone)" : 'Glisser pour déplacer, cliquer pour régler' }, pool ? '+ ' + wname(w) : wname(w));
  c.addEventListener('dragstart', (e) => { drag = w; c.classList.add('dragging'); e.dataTransfer.setData('text/plain', w); e.dataTransfer.effectAllowed = 'move'; });
  c.addEventListener('dragend', () => { drag = null; c.classList.remove('dragging'); });
  // Un widget de la réserve s'ajoute d'un clic (fin de la zone Droite) : le glisser-déposer reste possible mais n'est plus obligatoire.
  c.addEventListener('click', () => { if (pool) moveWidget(w, 0, 'right', 999); else { wsel = wsel === w ? null : w; setTab('layout'); } });
  c.addEventListener('keydown', (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); c.click(); } });
  return c;
}
function zoneEl(title, widgets, row, zone) {
  const z = h('div', { class: 'zone' }, h('small', {}, title), widgets.map((w) => chip(w, row == null)));
  z.addEventListener('dragover', (e) => { if (drag) { e.preventDefault(); z.classList.add('over'); } });
  z.addEventListener('dragleave', () => z.classList.remove('over'));
  z.addEventListener('drop', (e) => { e.preventDefault(); z.classList.remove('over'); if (drag) moveWidget(drag, row, zone, row == null ? 0 : dropIndex(z, e)); });
  return z;
}
// Décalage en pixels le long de la barre, pour un widget ou une zone (`zone_center`…). 0 = position naturelle.
function offsetField(key, label) {
  const s = cfg.screen[sel];
  s.offsets = s.offsets || {};
  return fld(label, h('input', {
    type: 'number', min: -3000, max: 3000, step: 2, value: s.offsets[key] || 0,
    onchange: (e) => { const v = Math.round(Number(e.target.value) || 0); if (v === 0) delete s.offsets[key]; else s.offsets[key] = v; touch(); },
  }), 'Négatif : vers le début de la barre.');
}
function widgetPanel(w) {
  const s = cfg.screen[sel], err = h('div', { class: 'err' });
  const where = [['', 'Retirer de la barre'], ...L.rows.flatMap((r, ri) => (r ? ZONES.map(([k, n]) => [ri + ':' + k, 'Rangée ' + (ri + 1) + ', ' + n]) : []))];
  const cur = L.rows.flatMap((r, ri) => (r ? ZONES.filter(([k]) => r[k].includes(w)).map(([k]) => ri + ':' + k) : []))[0] || '';
  let timer = 0;
  const cond = h('input', {
    type: 'text', value: s.show_when[w] || '', placeholder: 'always, media, battery<20, cpu>50',
    oninput: (e) => {
      clearTimeout(timer);
      timer = setTimeout(async () => {
        const v = e.target.value.trim();
        if (!v || v === 'always') { delete s.show_when[w]; err.textContent = ''; touch(); return; }
        try { await api('/api/condition', { text: v }); s.show_when[w] = v; err.textContent = ''; touch(); } catch (x) { err.textContent = x.message; }
      }, 300);
    },
  });
  return h('div', { class: 'panel' }, h('h2', {}, 'Widget « ' + wname(w) + ' »'),
    h('div', { class: 'row' },
      fld('Style', selectEl(s.styles[w] || 'text', [['text', 'Texte'], ['gauge', 'Jauge'], ['ring', 'Anneau'], ['graph', 'Mini-graphe']], (v) => { if (v === 'text') delete s.styles[w]; else s.styles[w] = v; touch(); })),
      fld('Afficher seulement si', cond, 'Métriques : ' + S.metrics.join(', ') + '. Vide = toujours.'),
      offsetField(w, 'Décalage (px)'),
      fld('Emplacement', selectEl(cur, where, (v) => { if (!v) moveWidget(w, null); else { const [r, z] = v.split(':'); moveWidget(w, Number(r), z, 99); } }))),
    err);
}
function pageLayout() {
  layoutLoad();
  const lay = cfg.screen[sel].layout, dbl = lay.preset === 'double_row';
  const rowBox = (r, ri) => h('div', { class: 'panel' }, h('h2', {}, ri ? 'Rangée 2' : 'Rangée 1'),
    h('div', { class: 'zones' }, ZONES.map(([k, n]) => zoneEl(n, r[k], ri, k))));
  const missing = S.widgets.filter((w) => !usedWidgets().includes(w));
  const pool = zoneEl('Widgets disponibles : cliquez pour ajouter à la barre (déposez ici pour retirer)', missing, null, null);
  const addAll = missing.length ? h('button', { onclick: () => { for (const w of missing) { L.rows[0].right.push(w); } L.hand = true; layoutCommit(); setTab('layout'); } }, 'Tout ajouter à la barre (' + missing.length + ')') : null;
  return [h('div', { class: 'panel' }, h('h2', {}, 'Disposition de la barre'),
    h('div', { class: 'row' },
      fld('Préréglage', selectEl(lay.preset, S.presets.map((p) => [p.id, PRESET_NAMES[p.id] || p.id]), (v) => {
        if (L.hand && !confirm('Remplacer vos zones personnalisées par ce préréglage ?')) { setTab('layout'); return; }
        lay.preset = v; for (const [k] of ZONES) { lay[k] = []; lay.row2[k] = []; }
        touch(); wsel = null; setTab('layout');
      })),
      h('label', { class: 'check' }, h('input', {
        type: 'checkbox', checked: !!L.rows[1], disabled: dbl,
        onchange: (e) => { L.rows[1] = e.target.checked ? { left: [], center: [], right: [] } : null; L.hand = true; layoutCommit(); setTab('layout'); },
      }), 'Deuxième rangée' + (dbl ? ' (imposée par double_row)' : ''))),
    h('p', { class: 'hint' }, 'Glissez les widgets entre les zones. Dès que vous en déplacez un, les zones deviennent personnalisées (elles priment sur le préréglage).')),
    h('div', { class: 'panel' }, h('h2', {}, 'Placement fin des zones'),
      h('div', { class: 'row' }, offsetField('zone_left', 'Zone début (px)'), offsetField('zone_center', 'Zone centre (px)'), offsetField('zone_right', 'Zone fin (px)')),
      h('p', { class: 'hint' }, 'Décale toute une zone, par exemple le HUD Claude pour mieux le centrer. Pour un seul widget : cliquez-le ci-dessous.')),
    L.rows.map((r, i) => (r ? rowBox(r, i) : null)),
    h('div', { class: 'panel' }, pool, addAll),
    wsel ? widgetPanel(wsel) : h('p', { class: 'hint' }, 'Cliquez un widget pour régler son style et son affichage conditionnel.')];
}

// ---------- page : écrans ----------
function pageScreens() {
  const ids = ['primary', 'all', '1', '2', '3', '4', '5', '6'];
  const list = cfg.screen.map((s, i) => {
    const id = ids.includes(s.id) ? ids : [...ids, s.id];
    return h('div', { class: 'panel' }, h('h2', {}, 'Écran ' + (i + 1)),
      h('div', { class: 'row' }, chk(s, 'enabled', 'Activé'), chk(s, 'autohide', 'Masquage automatique')),
      h('div', { class: 'row' },
        pick(s, 'id', 'Moniteur', id.map((x) => [x, x === 'primary' ? 'Principal' : x === 'all' ? 'Tous' : 'Numéro ' + x]), () => { touch(); fillScreens(); }),
        pick(s, 'edge', 'Bord', [['top', 'Haut'], ['bottom', 'Bas'], ['left', 'Gauche'], ['right', 'Droite']]),
        pick(s, 'mode', 'Mode', [['reserved', 'Réservé (comme la barre des tâches)'], ['overlay', 'Superposé']]),
        pick(s, 'theme', 'Thème', S.themes.map((t) => [t.id, t.name])),
        num(s, 'thickness', 'Épaisseur (16 à 96)', { min: 16, max: 96 }),
        num(s, 'margin', 'Marge (flottant si > 0)', { min: 0, max: 200 })),
      h('div', { class: 'row' }, h('button', { class: 'danger', disabled: cfg.screen.length < 2, onclick: () => {
        cfg.screen.splice(i, 1); sel = Math.min(sel, cfg.screen.length - 1); touch(); fillScreens(); setTab('screens');
      } }, 'Supprimer cet écran')));
  });
  return [...list, h('button', { onclick: () => {
    const n = clone(cfg.screen[sel]);
    n.id = String(cfg.screen.length + 1);
    cfg.screen.push(n); sel = cfg.screen.length - 1; touch(); fillScreens(); setTab('screens');
  } }, '+ Ajouter un écran')];
}

// ---------- page : Claude ----------
async function adminKeyAction(path, body, okMsg) {
  try { const r = await api(path, body); S.admin_key = r.admin_key; msg(okMsg, 'ok'); setTab('claude'); } catch (e) { msg(e.message, 'bad'); }
}
function pageClaude() {
  const c = cfg.claude, keyIn = h('input', { type: 'password', autocomplete: 'off', placeholder: 'sk-ant-admin…' });
  const alerts = h('input', {
    type: 'text', value: c.alerts.join(', '),
    onchange: (e) => {
      const v = e.target.value.split(/[,\s]+/).filter(Boolean).map(Number);
      if (v.some((n) => !Number.isInteger(n) || n < 1 || n > 100)) { msg('Alertes : pourcentages entiers entre 1 et 100.', 'bad'); e.target.value = c.alerts.join(', '); return; }
      c.alerts = v; touch();
    },
  });
  const names = { context: 'Contexte', tokens_today: 'Tokens du jour', cost_today: 'Coût du jour', cost_week: 'Coût de la semaine', cost_month: 'Coût du mois', claude_code: 'Logs Claude Code', api: 'Admin API' };
  return [h('div', { class: 'panel' }, h('h2', {}, 'HUD Claude'),
    h('div', { class: 'row' }, chk(c, 'enabled', 'Activé'), chk(c, 'notifications', 'Notifications Windows aux seuils')),
    members(c, 'sources', 'Sources', S.claude_sources, names),
    members(c, 'bar_summary', 'Résumé dans la barre', S.summary_items, names),
    h('div', { class: 'row' }, fld('Alertes (%)', alerts, 'ex. 80, 95'), num(c, 'context_window', 'Fenêtre de contexte (tokens)', { min: 1000 }),
      num(c, 'refresh_seconds', 'Rafraîchissement (s, 5 minimum)', { min: 5 }), txt(c, 'logs_dir', 'Dossier des logs', { ph: '%USERPROFILE%\\.claude\\projects' })),
    h('h3', {}, 'Budgets (USD)'),
    h('div', { class: 'row' }, num(c.budgets, 'daily_usd', 'Par jour', { min: 0, step: 0.5, nullable: true }), num(c.budgets, 'weekly_usd', 'Par semaine', { min: 0, step: 0.5, nullable: true }),
      num(c.budgets, 'monthly_usd', 'Par mois', { min: 0, step: 0.5, nullable: true })),
    h('h3', {}, 'Abonnement Pro / Max (estimation sur logs locaux)'),
    h('div', { class: 'row' }, chk(c.subscription, 'enabled', 'Activé'), num(c.subscription, 'five_hour_limit_usd', 'Budget des 5 heures (USD)', { min: 0, step: 0.5, nullable: true })),
    h('h3', {}, 'Panneau flottant'),
    h('div', { class: 'row' }, chk(c.floating, 'enabled', 'Activé'), chk(c.floating, 'click_through', 'Clics à travers'),
      num(c.floating, 'opacity', 'Opacité (0,1 à 1)', { min: 0.1, max: 1, step: 0.05 }),
      pick(c.floating, 'corner', 'Coin', [['top_left', 'Haut gauche'], ['top_right', 'Haut droite'], ['bottom_left', 'Bas gauche'], ['bottom_right', 'Bas droite']]),
      num(c.floating, 'margin', 'Marge', { min: 0, max: 500 }))),
  h('div', { class: 'panel' }, h('h2', {}, 'Clé Admin API Anthropic'),
    h('p', {}, S.admin_key ? 'Une clé est enregistrée dans le Gestionnaire d\'identification Windows (jamais affichée).' : 'Aucune clé enregistrée.'),
    h('div', { class: 'row' }, fld('Nouvelle clé', keyIn, 'Doit commencer par sk-ant-admin. Enregistrée immédiatement, sans passer par config.toml.'),
      h('button', { class: 'primary', onclick: () => { const k = keyIn.value; keyIn.value = ''; adminKeyAction('/api/admin-key', { key: k }, 'Clé enregistrée.'); } }, 'Enregistrer la clé'),
      h('button', { class: 'danger', disabled: !S.admin_key, onclick: () => adminKeyAction('/api/admin-key/clear', {}, 'Clé supprimée.') }, 'Supprimer la clé')))];
}

// ---------- pages : général, météo, presse-papiers ----------
const desc = (t) => h('p', { class: 'hint desc' }, t);
function pageGeneral() {
  const g = cfg.general;
  const packs = [['', 'Glyphes Segoe (aucun pack)'], ...S.icon_packs.map((p) => [p, p === 'forge' ? 'forge (intégré)' : p])];
  if (!packs.some(([p]) => p === g.icon_pack)) packs.push([g.icon_pack, g.icon_pack]);
  return [
    h('div', { class: 'panel' }, h('h2', {}, 'Démarrage et mises à jour'),
      desc('Forge Bar se lance avec votre session et se tient à jour tout seul.'),
      chk(g, 'autostart', 'Lancer avec Windows', touch, 'La barre apparaît dès l\'ouverture de session.'),
      chk(g, 'auto_update', 'Mises à jour automatiques', touch, 'Vérification au démarrage puis toutes les 6 h, contrôle SHA-256, installation silencieuse. Clic droit sur la barre : « Rechercher une mise à jour ».')),
    h('div', { class: 'panel' }, h('h2', {}, 'Jeux et plein écran'),
      desc('Forge Bar n\'injecte rien dans les jeux et reste compatible avec les anti-triche. Il peut rester visible par-dessus un jeu en fenêtré, sans bordure ou en plein écran « optimisé » ; un plein écran exclusif le recouvre comme toute autre fenêtre.'),
      chkInv(g, 'hide_on_fullscreen', 'Afficher la barre dans les jeux', touch, 'Désactivé : la barre se masque dès qu\'une application passe en plein écran. Activé : elle reste au-dessus (Ctrl+Alt+B pour la masquer à la main).')),
    h('div', { class: 'panel' }, h('h2', {}, 'Apparence'),
      chk(g, 'accent_from_wallpaper', 'Accent tiré du fond d\'écran', touch, 'Remplace la couleur d\'accent du thème par la couleur dominante de votre fond d\'écran.'),
      h('div', { class: 'row' }, pick(g, 'icon_pack', 'Pack d\'icônes', packs)),
      h('p', { class: 'hint' }, 'Packs personnalisés : sous-dossiers de %APPDATA%\\ForgeBar\\icons.')),
  ];
}
function pageWeather() {
  const w = cfg.weather;
  return [h('div', { class: 'panel' }, h('h2', {}, 'Météo (Open-Meteo)'),
    desc('Service gratuit et sans clé. Renseignez une ville, ou des coordonnées précises.'),
    chk(w, 'enabled', 'Afficher la météo dans la barre'),
    h('div', { class: 'row' }, txt(w, 'city', 'Ville'),
      num(w, 'latitude', 'Latitude', { min: -90, max: 90, step: 0.0001, nullable: true }),
      num(w, 'longitude', 'Longitude', { min: -180, max: 180, step: 0.0001, nullable: true }),
      pick(w, 'unit', 'Unité', [['celsius', '°C'], ['fahrenheit', '°F']]),
      num(w, 'refresh_minutes', 'Rafraîchissement (min)', { min: 1 })))];
}
function pageClipboard() {
  const cl = cfg.clipboard;
  return [h('div', { class: 'panel' }, h('h2', {}, 'Historique du presse-papiers'),
    desc('Gardé en mémoire uniquement : jamais écrit sur le disque, vidé à la fermeture.'),
    chk(cl, 'enabled', 'Activer l\'historique'),
    h('div', { class: 'row' }, num(cl, 'history', 'Éléments gardés', { min: 1, max: 100 }), num(cl, 'max_chars', 'Longueur affichée (caractères)', { min: 10, max: 500 })))];
}

// ---------- navigation par catégories ----------
const ICONS = {
  themes: 'M12 3a9 9 0 1 0 0 18c1.1 0 2-.9 2-2 0-.5-.2-1-.5-1.3-.3-.3-.5-.8-.5-1.2 0-1.1.9-2 2-2h2.3A3.9 3.9 0 0 0 21 10.6C21 6.4 17 3 12 3zM7.5 11h.01M10 7.5h.01M14.5 7.5h.01',
  editor: 'M12 20h9M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4z',
  layout: 'M3 5h18v14H3zM3 10h18M9 10v9',
  screens: 'M3 4h18v12H3zM8 20h8M12 16v4',
  claude: 'M12 3l1.8 5.2L19 10l-5.2 1.8L12 17l-1.8-5.2L5 10l5.2-1.8zM19 16l.8 2.2L22 19l-2.2.8L19 22l-.8-2.2L16 19l2.2-.8z',
  weather: 'M7 18a4 4 0 0 1-.5-7.97A5.5 5.5 0 0 1 17 9a4.5 4.5 0 0 1 .5 9z',
  clipboard: 'M9 3h6v3H9zM6 5h3M15 5h3v16H6V5z',
  general: 'M4 6h10M18 6h2M4 12h4M12 12h8M4 18h12M20 18h0M14 4v4M8 10v4M16 16v4',
};
// [id, libellé, page, groupe, sous-titre]
const TABS = [
  ['themes', 'Thèmes', pageThemes, 'Apparence', 'Choisissez l\'allure de votre barre : un clic l\'applique en direct.'],
  ['editor', 'Éditeur de thème', pageEditor, 'Apparence', 'Créez votre propre thème à partir d\'un existant.'],
  ['layout', 'Disposition', pageLayout, 'Barre', 'Quels widgets, dans quelle zone, avec quel style et quel décalage.'],
  ['screens', 'Écrans et position', pageScreens, 'Barre', 'Bord, taille, mode et thème de la barre sur chaque écran.'],
  ['claude', 'HUD Claude', pageClaude, 'Intégrations', 'Consommation, budgets, alertes et clé Admin API.'],
  ['weather', 'Météo', pageWeather, 'Intégrations', 'Conditions actuelles dans la barre.'],
  ['clipboard', 'Presse-papiers', pageClipboard, 'Intégrations', 'Historique rapide des derniers textes copiés.'],
  ['general', 'Général', pageGeneral, 'Système', 'Démarrage, mises à jour, jeux et apparence.'],
];
function svgIcon(name) {
  const s = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
  s.setAttribute('viewBox', '0 0 24 24');
  s.setAttribute('aria-hidden', 'true');
  const p = document.createElementNS('http://www.w3.org/2000/svg', 'path');
  p.setAttribute('d', ICONS[name] || '');
  s.append(p);
  return s;
}
function setTab(t) {
  tab = t;
  gen++;
  document.querySelectorAll('#tabs button').forEach((b) => b.setAttribute('aria-selected', b.dataset.tab === t));
  const entry = TABS.find((x) => x[0] === t);
  $('#page-title').textContent = entry[1];
  $('#page-sub').textContent = entry[4];
  $('#main').replaceChildren(...[entry[2]()].flat(Infinity).filter(Boolean));
}
function buildNav() {
  const nodes = [];
  let group = '';
  for (const [id, label, , g] of TABS) {
    if (g !== group) { group = g; nodes.push(h('div', { class: 'nav-group' }, g)); }
    nodes.push(h('button', { role: 'tab', 'data-tab': id, onclick: () => setTab(id) }, svgIcon(id), label));
  }
  $('#tabs').replaceChildren(...nodes);
}
function fillScreens() {
  const s = $('#screen-sel');
  s.replaceChildren(...cfg.screen.map((x, i) => h('option', { value: i }, 'Écran ' + (i + 1) + ' (' + (x.id === 'primary' ? 'principal' : x.id === 'all' ? 'tous' : x.id) + ')')));
  s.value = sel;
}
async function start() {
  buildNav();
  $('#save').addEventListener('click', saveConfig);
  $('#live').checked = live;
  $('#live').addEventListener('change', (e) => { live = e.target.checked; try { localStorage.setItem('live', live ? '1' : '0'); } catch { /* stockage indisponible */ } });
  $('#revert').addEventListener('click', async () => { cfg = clone(orig); await saveConfig(); fillScreens(); setTab(tab); msg('Réglages d\'origine rétablis.', 'ok'); });
  $('#screen-sel').addEventListener('change', (e) => { sel = Number(e.target.value); wsel = null; setTab(tab); refreshBarPreview(); });
  $('#quit').addEventListener('click', async () => {
    if (dirty && !confirm('Des modifications ne sont pas enregistrées. Fermer quand même ?')) return;
    try { await api('/api/quit', {}); } catch { /* déjà arrêté */ }
    dirty = false;
    document.body.replaceChildren(h('main', {}, h('p', {}, 'Serveur local arrêté. Vous pouvez fermer cet onglet.')));
  });
  window.addEventListener('beforeunload', (e) => { if (dirty) e.preventDefault(); });
  try {
    await load();
    fillScreens();
    setTab('themes');
    refreshBarPreview();
  } catch (e) {
    $('#main').replaceChildren(h('p', { class: 'err' }, 'Impossible de charger l\'état : ' + e.message + '. Rouvrez l\'éditeur depuis forge-settings (le jeton est dans l\'adresse).'));
  }
}
start();
