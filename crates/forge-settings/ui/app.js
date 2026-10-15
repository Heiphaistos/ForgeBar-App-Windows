'use strict';
// Éditeur Forge Bar : JavaScript pur, aucune dépendance. Le jeton vient de l'adresse (?t=...).
const TOKEN = new URLSearchParams(location.search).get('t') || '';
const $ = (s) => document.querySelector(s);
const clone = (x) => JSON.parse(JSON.stringify(x));
let S = null; // état serveur (thèmes, listes)
let cfg = null; // configuration en cours d'édition
let sel = 0; // écran édité
let dirty = false;
let tab = 'themes';
let live = true; // chaque changement est enregistré aussitôt : la vraie barre sert d'aperçu
try { live = localStorage.getItem('live') !== '0'; } catch { /* stockage indisponible */ }
let orig = null; // configuration à l'ouverture, pour « Rétablir »
let liveTimer = 0;
let gen = 0; // invalide les aperçus d'une page quittée

function h(tag, attrs, ...kids) {
  const e = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs || {})) {
    if (v == null || v === false) continue;
    if (k === 'class') e.className = v;
    else if (k === 'style') Object.assign(e.style, v);
    else if (k.startsWith('on')) e.addEventListener(k.slice(2), v);
    else if (k in e) e[k] = v;
    else e.setAttribute(k, v === true ? '' : v);
  }
  for (const c of kids.flat()) if (c != null && c !== false) e.append(c.nodeType ? c : document.createTextNode(c));
  return e;
}

function request(path, body) {
  const opt = { headers: { 'X-Forge-Token': TOKEN } };
  if (body !== undefined) {
    opt.method = 'POST';
    opt.headers['Content-Type'] = 'application/json';
    opt.body = JSON.stringify(body);
  }
  return fetch(path, opt);
}
async function api(path, body) {
  const r = await request(path, body);
  const j = await r.json().catch(() => ({ ok: false, error: 'réponse illisible' }));
  if (!r.ok || j.ok === false) throw new Error(j.error || 'erreur HTTP ' + r.status);
  return j;
}
async function fetchPng(path, body) {
  const r = await request(path, body);
  if (!r.ok) throw new Error('HTTP ' + r.status);
  return URL.createObjectURL(await r.blob());
}

function msg(text, kind) {
  const m = $('#msg');
  m.textContent = text || '';
  m.className = kind || '';
}
function touch() {
  dirty = true;
  $('#save').disabled = false;
  if (live) { clearTimeout(liveTimer); liveTimer = setTimeout(saveConfig, 350); }
}

// ---------- champs génériques ----------
function fld(label, input, hint) {
  return h('label', { class: 'field' }, h('span', {}, label), input, hint ? h('span', { class: 'hint' }, hint) : null);
}
function chk(o, k, label, cb = touch) {
  return h('label', { class: 'check' }, h('input', { type: 'checkbox', checked: !!o[k], onchange: (e) => { o[k] = e.target.checked; cb(); } }), label);
}
function num(o, k, label, opt = {}) {
  const i = h('input', {
    type: 'number', min: opt.min, max: opt.max, step: opt.step || 1, value: o[k] ?? '',
    onchange: (e) => {
      const v = e.target.value;
      if (v === '') { if (opt.nullable) o[k] = null; else e.target.value = o[k]; } else o[k] = Number(v);
      (opt.cb || touch)();
    },
  });
  return fld(label, i, opt.hint);
}
function txt(o, k, label, opt = {}) {
  return fld(label, h('input', { type: 'text', value: o[k] ?? '', placeholder: opt.ph, onchange: (e) => { o[k] = e.target.value; (opt.cb || touch)(); } }), opt.hint);
}
function selectEl(value, options, onchange) {
  const s = h('select', { onchange: (e) => onchange(e.target.value) }, options.map((o) => (Array.isArray(o) ? h('option', { value: o[0] }, o[1]) : h('option', { value: o }, o))));
  s.value = value;
  return s;
}
function pick(o, k, label, options, cb = touch) {
  return fld(label, selectEl(o[k], options, (v) => { o[k] = v; cb(); }));
}
function members(o, k, label, values, names = {}) {
  return h('div', { class: 'row' }, h('span', { class: 'hint' }, label),
    values.map((v) => h('label', { class: 'check' }, h('input', {
      type: 'checkbox', checked: o[k].includes(v),
      onchange: (e) => { o[k] = e.target.checked ? [...o[k], v] : o[k].filter((x) => x !== v); touch(); },
    }), names[v] || v)));
}

// ---------- couleurs ----------
function normColor(s) {
  s = String(s).trim();
  const m3 = s.match(/^#([0-9a-f])([0-9a-f])([0-9a-f])$/i);
  if (m3) s = '#' + m3[1] + m3[1] + m3[2] + m3[2] + m3[3] + m3[3];
  if (!/^#[0-9a-f]{6}([0-9a-f]{2})?$/i.test(s)) return null;
  s = s.toUpperCase();
  return s.length === 9 && s.endsWith('FF') ? s.slice(0, 7) : s;
}
// Sélecteur de couleur + transparence + hexadécimal. `optional` autorise la valeur vide.
function colorField(get, set, optional) {
  const v0 = normColor(get()) || '#000000';
  const col = h('input', { type: 'color', value: v0.slice(0, 7).toLowerCase(), 'aria-label': 'Couleur' });
  const alpha = h('input', { type: 'range', min: 0, max: 255, value: v0.length === 9 ? parseInt(v0.slice(7), 16) : 255, 'aria-label': 'Opacité de la couleur' });
  const hex = h('input', { type: 'text', value: get(), 'aria-label': 'Code hexadécimal' });
  const compose = () => {
    const a = Number(alpha.value);
    const c = normColor(col.value + (a === 255 ? '' : a.toString(16).padStart(2, '0')));
    hex.value = c;
    set(c);
  };
  col.addEventListener('input', compose);
  alpha.addEventListener('input', compose);
  hex.addEventListener('change', () => {
    if (optional && hex.value.trim() === '') { set(''); return; }
    const c = normColor(hex.value);
    if (!c) { hex.value = get(); return; }
    col.value = c.slice(0, 7).toLowerCase();
    alpha.value = c.length === 9 ? parseInt(c.slice(7), 16) : 255;
    hex.value = c;
    set(c);
  });
  return h('div', { class: 'colorf' }, col, alpha, hex);
}

// ---------- aperçus (réel via forge-bar --snapshot, sinon repli CSS) ----------
function fallbackPreview(t) {
  const bg = t.background, cols = bg.colors.length ? bg.colors : ['#202020'];
  const css = bg.kind === 'linear' && cols.length > 1 ? 'linear-gradient(' + (bg.angle + 90) + 'deg,' + cols.join(',') + ')' : cols[0];
  const fb = h('div', { class: 'pvfb' }, h('div', { class: 'layer' }),
    h('span', {}, 'Bureau'), h('span', { style: { color: t.colors.accent } }, 'CPU 23 %'),
    h('span', { style: { color: t.colors.text_secondary } }, 'RAM 61 %'), h('span', { style: { color: t.colors.warning } }, '18 %'), h('span', {}, '14:32'));
  const layer = fb.firstChild;
  layer.style.background = css;
  layer.style.opacity = bg.opacity;
  layer.style.borderRadius = t.radius + 'px';
  if (t.colors.border && t.border_width) layer.style.border = t.border_width + 'px solid ' + t.colors.border;
  fb.style.color = t.colors.text;
  fb.style.fontFamily = t.font_family;
  fb.style.fontSize = t.font_size + 'px';
  fb.style.gap = t.spacing + 'px';
  return fb;
}
const pvq = [];
let pvActive = 0;
function pump() {
  while (pvActive < 3 && pvq.length) {
    const job = pvq.shift();
    pvActive++;
    job().finally(() => { pvActive--; pump(); });
  }
}
const io = new IntersectionObserver((entries) => {
  for (const en of entries) if (en.isIntersecting) { io.unobserve(en.target); pvq.push(en.target._job); pump(); }
});
function presetParam() {
  const p = cfg.screen[sel].layout.preset;
  return p === 'custom' ? '' : '&preset=' + p;
}
// Case d'aperçu d'un thème enregistré : repli immédiat, image réelle par-dessus si disponible.
function themeBox(t) {
  const box = h('div', { class: 'pv' }, fallbackPreview(t));
  if (S.can_preview) {
    const g = gen;
    box._job = async () => {
      try {
        const url = await fetchPng('/preview.png?theme=' + encodeURIComponent(t.id) + '&width=340');
        if (g === gen) box.replaceChildren(h('img', { src: url, alt: 'Aperçu de ' + t.name }));
      } catch { /* le repli CSS reste affiché */ }
    };
    io.observe(box);
  }
  return box;
}

// ---------- chargement et enregistrement ----------
async function refreshState() { S = await api('/api/state'); }
async function load() {
  await refreshState();
  cfg = clone(S.config);
  orig = clone(S.config);
  sel = Math.min(sel, cfg.screen.length - 1);
  if (S.config_error) msg('Configuration illisible, valeurs par défaut affichées : ' + S.config_error, 'bad');
}
async function saveConfig() {
  try {
    await api('/api/config', cfg);
    dirty = false;
    $('#save').disabled = true;
    msg('Enregistré. La barre se met à jour d\'elle-même (environ 2 s).', 'ok');
    return true;
  } catch (e) {
    msg('Non enregistré : ' + e.message, 'bad');
    return false;
  }
}

// ---------- page : galerie de thèmes ----------
let famFilter = 'Tous';
function pageThemes() {
  const fams = ['Tous', 'Mes thèmes', ...[...new Set(S.themes.map((t) => t.family))].sort()];
  const grid = h('div', { class: 'grid' });
  const draw = () => {
    gen++;
    grid.replaceChildren(...S.themes
      .filter((t) => famFilter === 'Tous' || (famFilter === 'Mes thèmes' ? t.user : t.family === famFilter))
      .map((t) => {
        const on = cfg.screen[sel].theme === t.id;
        const card = h('div', { class: 'card' + (on ? ' sel' : ''), tabindex: 0, role: 'button', 'aria-pressed': on },
          themeBox(t),
          h('div', { class: 'name' }, h('strong', {}, t.name), h('button', { onclick: (e) => { e.stopPropagation(); editorOpen(t.id); } }, 'Modifier')),
          h('div', { class: 'fam' }, t.family + (t.user ? ' (perso)' : '')));
        const choose = () => { cfg.screen[sel].theme = t.id; touch(); draw(); };
        card.addEventListener('click', choose);
        card.addEventListener('keydown', (e) => { if (e.target === card && (e.key === 'Enter' || e.key === ' ')) { e.preventDefault(); choose(); } });
        return card;
      }));
  };
  const chips = h('div', { class: 'chips' }, fams.map((f) => h('button', {
    class: 'chip', 'aria-pressed': f === famFilter,
    onclick: (e) => { famFilter = f; chips.querySelectorAll('.chip').forEach((c) => c.setAttribute('aria-pressed', c === e.target)); draw(); },
  }, f)));
  draw();
  return [h('p', { class: 'hint' }, 'Cliquez un thème : il s\'applique tout de suite à la vraie barre (aperçu en direct, désactivable en haut). ' + (S.can_preview ? '' : 'forge-bar introuvable à côté de forge-settings : aperçus approximatifs.')), chips, grid];
}

// ---------- page : éditeur de thème ----------
let ed = null; // { id, base, vals, existing }
let edSeq = 0;
let edTimer = 0;
const slug = (s) => s.toLowerCase().normalize('NFD').replace(/[̀-ͯ]/g, '').replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '').slice(0, 40);
function editorOpen(id) {
  const t = S.themes.find((x) => x.id === id);
  if (t.user) {
    const base = S.themes.find((x) => x.id === t.extends) ? t.extends : S.themes[0].id;
    ed = { id: t.id, base, vals: clone(t), existing: true };
  } else {
    ed = { id: '', base: t.id, vals: clone(t), existing: false };
    ed.vals.name = 'Mon ' + t.name;
    ed.vals.family = 'perso';
    ed.id = slug(ed.vals.name);
  }
  setTab('editor');
}
function themeOverrides() {
  const base = S.themes.find((t) => t.id === ed.base), v = ed.vals, o = { name: v.name, family: v.family };
  for (const k of ['backdrop', 'radius', 'border_width', 'font_family', 'mono_family', 'font_size', 'spacing']) {
    if (JSON.stringify(v[k]) !== JSON.stringify(base[k])) o[k] = v[k];
  }
  const c = {};
  for (const k of Object.keys(v.colors)) if (v.colors[k] !== base.colors[k]) c[k] = v.colors[k];
  if (Object.keys(c).length) o.colors = c;
  if (JSON.stringify(v.background) !== JSON.stringify(base.background)) o.background = v.background;
  return o;
}
function edChanged() {
  const box = $('#edpv');
  if (!box) return;
  box.replaceChildren(fallbackPreview(ed.vals));
  clearTimeout(edTimer);
  const my = ++edSeq;
  if (!S.can_preview) return;
  edTimer = setTimeout(async () => {
    try {
      const p = cfg.screen[sel].layout.preset;
      const url = await fetchPng('/api/preview', { extends: ed.base, overrides: themeOverrides(), width: 900, preset: p === 'custom' ? null : p });
      if (my === edSeq && $('#edpv')) $('#edpv').append(h('img', { src: url, alt: 'Aperçu du thème en cours' }));
    } catch (e) {
      if ($('#edmsg')) $('#edmsg').textContent = 'Aperçu réel indisponible (' + e.message + ') : aperçu approximatif.';
    }
  }, 500);
}
function gradientEditor() {
  const bg = ed.vals.background, box = h('div');
  const swap = (i, j) => { [bg.colors[i], bg.colors[j]] = [bg.colors[j], bg.colors[i]]; draw(); edChanged(); };
  const draw = () => {
    box.replaceChildren(...bg.colors.map((c, i) => h('div', { class: 'row' },
      colorField(() => bg.colors[i], (v) => { bg.colors[i] = v; edChanged(); }),
      h('button', { disabled: i === 0, title: 'Monter', onclick: () => swap(i - 1, i) }, '↑'),
      h('button', { disabled: i === bg.colors.length - 1, title: 'Descendre', onclick: () => swap(i, i + 1) }, '↓'),
      h('button', { class: 'danger', disabled: bg.colors.length < 2, title: 'Retirer', onclick: () => { bg.colors.splice(i, 1); draw(); edChanged(); } }, '✕'))),
    h('button', { disabled: bg.colors.length >= 8, onclick: () => { bg.colors.push(bg.colors[bg.colors.length - 1] || '#202020'); draw(); edChanged(); } }, '+ couleur'));
  };
  draw();
  return box;
}
async function saveTheme(apply) {
  const err = $('#edmsg');
  if (!/^[a-z0-9-]{1,40}$/.test(ed.id)) { err.textContent = 'Identifiant invalide : a-z, 0-9 et tirets, 40 caractères maximum.'; return; }
  try {
    await api('/api/theme', { id: ed.id, extends: ed.base, overrides: themeOverrides() });
    await refreshState();
    ed.existing = true;
    err.textContent = '';
    msg('Thème « ' + ed.vals.name + ' » enregistré.', 'ok');
    if (apply) { cfg.screen[sel].theme = ed.id; touch(); await saveConfig(); }
  } catch (e) { err.textContent = e.message; msg('Thème non enregistré : ' + e.message, 'bad'); }
}
async function deleteTheme() {
  if (!confirm('Supprimer le thème « ' + ed.vals.name + ' » ?')) return;
  try {
    await api('/api/theme/delete', { id: ed.id });
    await refreshState();
    ed = null;
    msg('Thème supprimé.', 'ok');
    setTab('themes');
  } catch (e) { $('#edmsg').textContent = e.message; }
}
function pageEditor() {
  if (!ed) {
    let base = cfg.screen[sel].theme;
    return [h('div', { class: 'panel' }, h('h2', {}, 'Éditeur de thème'),
      h('p', {}, 'Choisissez un thème de départ. Le thème créé n\'enregistre que vos modifications (héritage `extends`).'),
      h('div', { class: 'row' }, selectEl(base, S.themes.map((t) => [t.id, t.name + (t.user ? ' (perso)' : '')]), (v) => { base = v; }),
        h('button', { class: 'primary', onclick: () => editorOpen(base) }, 'Modifier / dupliquer')))];
  }
  const v = ed.vals, c = v.colors;
  const colors = [['text', 'Texte'], ['text_secondary', 'Texte secondaire'], ['accent', 'Accent'], ['border', 'Bordure (vide = aucune)'], ['warning', 'Avertissement'], ['critical', 'Critique']];
  const form = h('div', { class: 'panel' }, h('h2', {}, 'Thème'),
    h('div', { class: 'row' },
      fld('Nom affiché', h('input', { type: 'text', value: v.name, onchange: (e) => { v.name = e.target.value; if (!ed.existing) { ed.id = slug(v.name); $('#edid').value = ed.id; } edChanged(); } })),
      fld('Identifiant (fichier)', h('input', { id: 'edid', type: 'text', value: ed.id, disabled: ed.existing, onchange: (e) => { ed.id = e.target.value; } })),
      fld('Famille', h('input', { type: 'text', value: v.family, onchange: (e) => { v.family = e.target.value; } })),
      fld('Thème de base', selectEl(ed.base, S.themes.filter((t) => t.id !== ed.id).map((t) => [t.id, t.name]), (b) => {
        ed.base = b;
        ed.vals = Object.assign(clone(S.themes.find((t) => t.id === b)), { name: v.name, family: v.family });
        setTab('editor');
      }))),
    h('h3', {}, 'Couleurs'),
    colors.map(([k, label]) => h('div', { class: 'row' }, h('span', { class: 'field' }, label), colorField(() => c[k], (x) => { c[k] = x; edChanged(); }, k === 'border'))),
    h('h3', {}, 'Fond'),
    h('div', { class: 'row' }, pick(v.background, 'kind', 'Type', [['solid', 'Uni'], ['linear', 'Dégradé linéaire']], edChanged),
      num(v.background, 'angle', 'Angle (°)', { min: 0, max: 360, cb: edChanged }),
      num(v.background, 'opacity', 'Opacité (0 à 1)', { min: 0, max: 1, step: 0.05, cb: edChanged })),
    gradientEditor(),
    h('h3', {}, 'Forme et texte'),
    h('div', { class: 'row' }, num(v, 'radius', 'Rayon (px)', { min: 0, max: 48, cb: edChanged }), num(v, 'border_width', 'Bordure (px)', { min: 0, max: 16, step: 0.5, cb: edChanged }),
      num(v, 'font_size', 'Taille de police', { min: 6, max: 48, step: 0.5, cb: edChanged }), num(v, 'spacing', 'Espacement', { min: 0, max: 64, cb: edChanged })),
    h('div', { class: 'row' }, txt(v, 'font_family', 'Police', { cb: edChanged }), txt(v, 'mono_family', 'Police à chasse fixe', { cb: edChanged }),
      pick(v, 'backdrop', 'Matériau (Windows 11)', [['none', 'Aucun'], ['mica', 'Mica'], ['acrylic', 'Acrylique (verre dépoli)'], ['tabbed', 'Mica foncé']], edChanged)));
  const side = h('div', {}, h('div', { class: 'panel' }, h('h2', {}, 'Aperçu'), h('div', { id: 'edpv', class: 'bigpv' }, fallbackPreview(v)),
    h('p', { id: 'edmsg', class: 'err' }),
    h('div', { class: 'row' }, h('button', { class: 'primary', onclick: () => saveTheme(false) }, 'Enregistrer le thème'),
      h('button', { onclick: () => saveTheme(true) }, 'Enregistrer et appliquer à l\'écran ' + (sel + 1)),
      ed.existing ? h('button', { class: 'danger', onclick: deleteTheme }, 'Supprimer') : null,
      h('button', { onclick: () => { ed = null; setTab('editor'); } }, 'Changer de thème')),
    h('p', { class: 'hint' }, 'Fichier : %APPDATA%\\ForgeBar\\themes\\' + (ed.id || '…') + '.toml')));
  setTimeout(edChanged, 0);
  return [h('div', { class: 'cols' }, form, side)];
}
