// ---------- page : disposition (glisser-déposer) ----------
const ZONES = [['left', 'Gauche'], ['center', 'Centre'], ['right', 'Droite']];
let L = null; // { hand, rows: [zones, zones|null] }
let drag = null;
let wsel = null; // widget sélectionné
const WNAMES = { desktop: 'Bureau', media: 'Média', claude: 'Claude', cpu: 'CPU', ram: 'RAM', gpu: 'GPU', disk: 'Disque', apps: 'Applications ouvertes', toggle: 'Bouton masquer', net: 'Réseau', weather: 'Météo', clipboard: 'Presse-papiers', wifi: 'Wi-Fi', sound: 'Son', battery: 'Batterie', clock: 'Horloge' };
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
function chip(w) {
  const c = h('span', { class: 'w' + (wsel === w ? ' on' : ''), draggable: true, tabindex: 0, role: 'button', title: 'Glisser pour déplacer, cliquer pour régler' }, wname(w));
  c.addEventListener('dragstart', (e) => { drag = w; c.classList.add('dragging'); e.dataTransfer.setData('text/plain', w); e.dataTransfer.effectAllowed = 'move'; });
  c.addEventListener('dragend', () => { drag = null; c.classList.remove('dragging'); });
  c.addEventListener('click', () => { wsel = wsel === w ? null : w; setTab('layout'); });
  c.addEventListener('keydown', (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); c.click(); } });
  return c;
}
function zoneEl(title, widgets, row, zone) {
  const z = h('div', { class: 'zone' }, h('small', {}, title), widgets.map(chip));
  z.addEventListener('dragover', (e) => { if (drag) { e.preventDefault(); z.classList.add('over'); } });
  z.addEventListener('dragleave', () => z.classList.remove('over'));
  z.addEventListener('drop', (e) => { e.preventDefault(); z.classList.remove('over'); if (drag) moveWidget(drag, row, zone, row == null ? 0 : dropIndex(z, e)); });
  return z;
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
      fld('Emplacement', selectEl(cur, where, (v) => { if (!v) moveWidget(w, null); else { const [r, z] = v.split(':'); moveWidget(w, Number(r), z, 99); } }))),
    err);
}
function pageLayout() {
  layoutLoad();
  const lay = cfg.screen[sel].layout, dbl = lay.preset === 'double_row';
  const rowBox = (r, ri) => h('div', { class: 'panel' }, h('h2', {}, ri ? 'Rangée 2' : 'Rangée 1'),
    h('div', { class: 'zones' }, ZONES.map(([k, n]) => zoneEl(n, r[k], ri, k))));
  const pool = zoneEl('Widgets disponibles (déposez ici pour retirer)', S.widgets.filter((w) => !usedWidgets().includes(w)), null, null);
  return [h('div', { class: 'panel' },
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
    L.rows.map((r, i) => (r ? rowBox(r, i) : null)),
    h('div', { class: 'panel' }, pool),
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

// ---------- page : divers ----------
function pageMisc() {
  const g = cfg.general, w = cfg.weather, cl = cfg.clipboard;
  const packs = [['', 'Glyphes Segoe (aucun pack)'], ...S.icon_packs.map((p) => [p, p === 'forge' ? 'forge (intégré)' : p])];
  if (!packs.some(([p]) => p === g.icon_pack)) packs.push([g.icon_pack, g.icon_pack]);
  return [h('div', { class: 'panel' }, h('h2', {}, 'Général'),
    h('div', { class: 'row' }, chk(g, 'autostart', 'Lancer avec Windows'), chk(g, 'hide_on_fullscreen', 'Masquer en plein écran'), chk(g, 'accent_from_wallpaper', 'Accent tiré du fond d\'écran')),
    h('div', { class: 'row' }, pick(g, 'icon_pack', 'Pack d\'icônes', packs)),
    h('p', { class: 'hint' }, 'Packs personnalisés : sous-dossiers de %APPDATA%\\ForgeBar\\icons.')),
  h('div', { class: 'panel' }, h('h2', {}, 'Météo (Open-Meteo)'),
    h('div', { class: 'row' }, chk(w, 'enabled', 'Activée'), txt(w, 'city', 'Ville'), num(w, 'latitude', 'Latitude', { min: -90, max: 90, step: 0.0001, nullable: true }),
      num(w, 'longitude', 'Longitude', { min: -180, max: 180, step: 0.0001, nullable: true }),
      pick(w, 'unit', 'Unité', [['celsius', '°C'], ['fahrenheit', '°F']]), num(w, 'refresh_minutes', 'Rafraîchissement (min)', { min: 1 }))),
  h('div', { class: 'panel' }, h('h2', {}, 'Presse-papiers (en mémoire uniquement)'),
    h('div', { class: 'row' }, chk(cl, 'enabled', 'Activé'), num(cl, 'history', 'Éléments gardés', { min: 1, max: 100 }), num(cl, 'max_chars', 'Longueur affichée', { min: 10, max: 500 })))];
}

// ---------- navigation et démarrage ----------
const TABS = [['themes', 'Thèmes', pageThemes], ['editor', 'Éditeur de thème', pageEditor], ['layout', 'Disposition', pageLayout], ['screens', 'Écrans', pageScreens], ['claude', 'Claude', pageClaude], ['misc', 'Divers', pageMisc]];
function setTab(t) {
  tab = t;
  gen++;
  document.querySelectorAll('#tabs button').forEach((b) => b.setAttribute('aria-selected', b.dataset.tab === t));
  const page = TABS.find((x) => x[0] === t)[2];
  $('#main').replaceChildren(...[page()].flat(Infinity).filter(Boolean));
}
function fillScreens() {
  const s = $('#screen-sel');
  s.replaceChildren(...cfg.screen.map((x, i) => h('option', { value: i }, 'Écran ' + (i + 1) + ' (' + x.id + ')')));
  s.value = sel;
}
async function start() {
  $('#tabs').replaceChildren(...TABS.map(([id, label]) => h('button', { role: 'tab', 'data-tab': id, onclick: () => setTab(id) }, label)));
  $('#save').addEventListener('click', saveConfig);
  $('#live').checked = live;
  $('#live').addEventListener('change', (e) => { live = e.target.checked; try { localStorage.setItem('live', live ? '1' : '0'); } catch { /* stockage indisponible */ } });
  $('#revert').addEventListener('click', async () => { cfg = clone(orig); await saveConfig(); fillScreens(); setTab(tab); msg('Réglages d\'origine rétablis.', 'ok'); });
  $('#screen-sel').addEventListener('change', (e) => { sel = Number(e.target.value); wsel = null; setTab(tab); });
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
  } catch (e) {
    $('#main').replaceChildren(h('p', { class: 'err' }, 'Impossible de charger l\'état : ' + e.message + '. Rouvrez l\'éditeur depuis forge-settings (le jeton est dans l\'adresse).'));
  }
}
start();
