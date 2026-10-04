// FightNight front end: boots the wasm game, owns the menus, forwards input, and runs the frame loop.
import init, * as fn from './pkg/fightnight.js';
import { Hud, drawFullMap } from './hud.js';
import { GameAudio } from './audio.js';
import { RARITY, RARITY_NAMES, RARITY_DARK, drawItem, AMMO_NAMES, AMMO_COLORS, MAT_NAMES, MAT_COLORS, WEAPON_NAMES } from './icons.js';

const $ = (id) => document.getElementById(id);
const q = new URLSearchParams(location.search);
const NOLOCK = q.get('nolock') === '1';

// ------------------------------------------------------------------------------------------------
// Settings
// ------------------------------------------------------------------------------------------------
const DEFAULTS = { name: 'You', bots: 39, difficulty: 'normal', quality: 'high', skipbus: false, sens: 10, fov: 62, vol: 70, invert: false, autosprint: true, tags: false, perf: false };
let cfg = { ...DEFAULTS };
try { Object.assign(cfg, JSON.parse(localStorage.getItem('fightnight.settings') || '{}')); } catch (e) { /* storage unavailable */ }
function saveCfg() { try { localStorage.setItem('fightnight.settings', JSON.stringify(cfg)); } catch (e) { /* ignore */ } }

// ------------------------------------------------------------------------------------------------
// State
// ------------------------------------------------------------------------------------------------
let state = 'loading'; // loading | menu | playing | paused | over
let overlay = null; // 'inventory' | 'map' | null (game keeps running)
let ready = false;
let hudState = null;
let lastFrame = performance.now();
let perfT = 0;
let spectating = false;
let endShown = false;
let pois = [];
const audio = new GameAudio();
const gpuCanvas = $('gpu');
const hudCanvas = $('hud');
const mapImage = document.createElement('canvas');
const hud = new Hud(hudCanvas, mapImage);
window.__game = { get state() { return state; }, get hud() { return hudState; }, fn, cfg, frames: 0, audio };
// Test helper: read back the rendered frame and paint it into a plain 2D canvas so a normal page
// screenshot (which cannot see the WebGPU swapchain in headless mode) shows the whole UI.
window.__game.freezeFrame = async () => {
  fn.capture_request();
  let d = [];
  for (let i = 0; i < 300 && !d.length; i++) { await new Promise((r) => requestAnimationFrame(r)); d = fn.capture_poll(); }
  if (!d.length) return false;
  const dv = new DataView(d.buffer, d.byteOffset);
  const w = dv.getUint32(0, true), h = dv.getUint32(4, true);
  const c = document.createElement('canvas'); c.width = w; c.height = h; c.id = 'frozen';
  c.style.cssText = 'position:fixed;inset:0;width:100vw;height:100vh;z-index:0';
  c.getContext('2d').putImageData(new ImageData(new Uint8ClampedArray(d.buffer, d.byteOffset + 8, w * h * 4), w, h), 0, 0);
  document.body.appendChild(c);
  gpuCanvas.style.visibility = 'hidden';
  return true;
};
window.__game.unfreeze = () => { const c = document.getElementById('frozen'); if (c) c.remove(); gpuCanvas.style.visibility = 'visible'; };

const TIPS = [
  'Hit trees and rocks with your pickaxe to collect wood, stone and metal.',
  'Hold forward and click with a ramp selected to ramp-rush up cliffs.',
  'Shield potions stack up to 100 shield; bandages only heal to 75 HP.',
  'Headshots deal bonus damage. Aim for the helmet.',
  'The storm shrinks in phases. Check the map (M) for the next safe zone.',
  'Walls block bullets. Build cover before you heal.',
  'Shotguns are deadly up close, snipers reward patience.',
  'You can open the glider early by pressing Space in free fall.',
];

// ------------------------------------------------------------------------------------------------
// UI helpers
// ------------------------------------------------------------------------------------------------
function show(id, on = true) { $(id).classList.toggle('active', on); }
function setProgress(p, text) {
  $('load-bar').style.width = Math.round(p * 100) + '%';
  if (text) $('load-text').textContent = text;
}
const nextFrame = () => new Promise((r) => requestAnimationFrame(() => r()));
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
function click() { audio.playUi('ui_click', 0.5); }

function bindSeg(id, key) {
  const seg = $(id);
  const apply = () => seg.querySelectorAll('button').forEach((b) => b.classList.toggle('on', b.dataset.v === cfg[key]));
  seg.querySelectorAll('button').forEach((b) => b.addEventListener('click', () => { cfg[key] = b.dataset.v; saveCfg(); apply(); click(); if (key === 'quality' && ready) fn.set_quality(cfg.quality); }));
  apply();
}
function bindRange(id, valId, key, fmt = (v) => v, onChange) {
  const el = $(id), val = $(valId);
  el.value = cfg[key]; val.textContent = fmt(+el.value);
  el.addEventListener('input', () => { cfg[key] = +el.value; val.textContent = fmt(cfg[key]); saveCfg(); if (onChange) onChange(); });
}
function bindCheck(id, key, onChange) {
  const el = $(id); el.checked = !!cfg[key];
  el.addEventListener('change', () => { cfg[key] = el.checked; saveCfg(); click(); if (onChange) onChange(); });
}

function applyRuntimeOptions() {
  if (!ready) return;
  fn.set_options(0.00022 * cfg.sens, !!cfg.invert, cfg.fov, !!cfg.tags, !!cfg.autosprint);
  audio.setVolume(cfg.vol / 100);
  hud.showPerf = !!cfg.perf;
}

function wireUi() {
  $('opt-name').value = cfg.name;
  $('opt-name').addEventListener('input', () => { cfg.name = $('opt-name').value; saveCfg(); });
  bindRange('opt-bots', 'val-bots', 'bots');
  bindSeg('seg-diff', 'difficulty');
  bindSeg('seg-quality', 'quality');
  bindCheck('opt-skipbus', 'skipbus');
  bindRange('set-sens', 'val-sens', 'sens', (v) => v, applyRuntimeOptions);
  bindRange('set-fov', 'val-fov', 'fov', (v) => v, applyRuntimeOptions);
  bindRange('set-vol', 'val-vol', 'vol', (v) => v, applyRuntimeOptions);
  bindCheck('set-invert', 'invert', applyRuntimeOptions);
  bindCheck('set-autosprint', 'autosprint', applyRuntimeOptions);
  bindCheck('set-tags', 'tags', applyRuntimeOptions);
  bindCheck('set-fps', 'perf', applyRuntimeOptions);

  $('btn-fullscreen').addEventListener('click', () => {
    try { if (document.fullscreenElement) document.exitFullscreen(); else document.documentElement.requestFullscreen(); } catch (e) { /* not allowed */ }
  });
  $('btn-play').addEventListener('click', () => { click(); startMatch(); });
  $('btn-settings').addEventListener('click', () => { click(); show('settings'); });
  $('btn-help').addEventListener('click', () => { click(); show('help'); });
  $('btn-settings-close').addEventListener('click', () => { click(); show('settings', false); });
  $('btn-help-close').addEventListener('click', () => { click(); show('help', false); });
  $('btn-resume').addEventListener('click', () => { click(); resume(); });
  $('btn-pause-settings').addEventListener('click', () => { click(); show('settings'); });
  $('btn-quit').addEventListener('click', () => { click(); toMenu(); });
  $('btn-again').addEventListener('click', () => { click(); startMatch(); });
  $('btn-menu').addEventListener('click', () => { click(); toMenu(); });
  $('btn-reload').addEventListener('click', () => location.reload());
  $('btn-spectate').addEventListener('click', () => { click(); spectating = true; show('over', false); stopConfetti(); lockPointer(); });
  for (const b of document.querySelectorAll('.btn')) b.addEventListener('mouseenter', () => audio.playUi('ui_hover', 0.25));
}

// ------------------------------------------------------------------------------------------------
// Canvas sizing
// ------------------------------------------------------------------------------------------------
function resizeAll() {
  const dprCap = cfg.quality === 'high' ? 2 : cfg.quality === 'medium' ? 1.5 : 1.25;
  let dpr = Math.min(window.devicePixelRatio || 1, dprCap);
  // keep the frame to a sane pixel budget on very large screens
  const budget = cfg.quality === 'high' ? 4.0e6 : cfg.quality === 'medium' ? 2.6e6 : 1.6e6;
  const px = innerWidth * innerHeight * dpr * dpr;
  if (px > budget) dpr *= Math.sqrt(budget / px);
  const w = Math.max(2, Math.floor(innerWidth * dpr)), h = Math.max(2, Math.floor(innerHeight * dpr));
  if (gpuCanvas.width !== w || gpuCanvas.height !== h) {
    gpuCanvas.width = w; gpuCanvas.height = h;
    if (ready) fn.resize(w, h);
  }
  hud.resize(innerWidth, innerHeight, Math.min(window.devicePixelRatio || 1, 2));
}

// ------------------------------------------------------------------------------------------------
// Boot
// ------------------------------------------------------------------------------------------------
// Called from the wasm module when the GPU device is lost (driver reset, GPU unplugged, ...).
window.__fnDeviceLost = (message) => {
  ready = false;
  document.exitPointerLock?.();
  for (const id of ['menu', 'pause', 'inventory', 'map', 'over', 'settings', 'help', 'loading']) show(id, false);
  hud.clear();
  $('nowebgpu-title').textContent = 'Graphics device lost';
  $('nowebgpu-msg').textContent = 'The connection to your GPU was interrupted' + (message ? ' (' + message + ')' : '') + '. Reload the page to keep playing.';
  show('nowebgpu');
};

async function boot() {
  wireUi();
  $('tip').textContent = TIPS[Math.floor(Math.random() * TIPS.length)];
  if (!navigator.gpu) { show('loading', false); show('nowebgpu'); return; }
  try {
    setProgress(0.05, 'Loading engine');
    await init();
    setProgress(0.2, 'Starting WebGPU');
    resizeAll();
    await fn.init_gpu(gpuCanvas);
    setProgress(0.3, 'Building the island');
    await nextFrame(); await nextFrame();
    const opts = `bots=${cfg.bots};difficulty=${cfg.difficulty};name=${cfg.name};quality=${cfg.quality};skipbus=${cfg.skipbus ? 1 : 0}` + (q.get('opts') ? ';' + q.get('opts') : '');
    fn.start_match(opts);
    ready = true;
    setProgress(0.75, 'Drawing the map');
    await nextFrame();
    buildMap();
    pois = fn.poi_list().split(';').filter(Boolean).map((s) => { const p = s.split(','); return { name: p[0], x: +p[1], z: +p[2], kind: p[3], r: +p[4] }; });
    applyRuntimeOptions();
    fn.set_menu(true);
    resizeAll();
    setProgress(1, 'Ready');
    requestAnimationFrame(loop);
    await sleep(250);
    state = 'menu';
    $('loading').classList.add('fade');
    show('menu');
    setTimeout(() => { show('loading', false); $('loading').classList.remove('fade'); }, 700);
    if (q.get('autostart') === '1') startMatch();
  } catch (e) {
    console.error(e);
    $('load-text').textContent = 'Could not start: ' + (e && e.message ? e.message : e);
    $('nowebgpu-msg').textContent = String(e && e.message ? e.message : e);
    show('loading', false); show('nowebgpu');
  }
}

function buildMap() {
  const data = fn.minimap();
  const size = Math.round(Math.sqrt(data.length / 4));
  mapImage.width = size; mapImage.height = size;
  const ictx = mapImage.getContext('2d');
  ictx.putImageData(new ImageData(new Uint8ClampedArray(data.buffer, data.byteOffset, data.length), size, size), 0, 0);
}

// ------------------------------------------------------------------------------------------------
// Match lifecycle
// ------------------------------------------------------------------------------------------------
async function startMatch() {
  if (state === 'loading') return;
  state = 'loading';
  $('tip').textContent = TIPS[Math.floor(Math.random() * TIPS.length)];
  show('menu', false); show('over', false); show('pause', false); stopConfetti();
  $('loading').classList.remove('fade');
  show('loading');
  setProgress(0.05, 'Preparing the match');
  await nextFrame(); await nextFrame();
  try {
    if (!audio.ready) {
      setProgress(0.1, 'Tuning the sound');
      await audio.init(fn, (p) => setProgress(0.1 + p * 0.5, 'Tuning the sound'));
    } else audio.resume();
    setProgress(0.65, 'Building the island');
    await nextFrame();
    const opts = `bots=${cfg.bots};difficulty=${cfg.difficulty};name=${cfg.name};quality=${cfg.quality};skipbus=${cfg.skipbus ? 1 : 0}` + (q.get('opts') ? ';' + q.get('opts') : '');
    fn.start_match(opts);
    applyRuntimeOptions();
    fn.set_menu(false);
    fn.set_paused(false);
    setProgress(1, 'Ready');
  } catch (e) {
    console.error(e);
    $('err').textContent = 'Could not start the match: ' + e;
  }
  spectating = false; endShown = false; overlay = null;
  state = 'playing';
  $('loading').classList.add('fade');
  setTimeout(() => { show('loading', false); $('loading').classList.remove('fade'); }, 600);
  lockPointer();
  audio.playUi('drop_in', 0.0);
}

function toMenu() {
  state = 'menu'; overlay = null;
  fn.set_paused(false); fn.set_menu(true);
  audio.silenceLoops();
  show('pause', false); show('over', false); show('inventory', false); show('map', false); show('settings', false); stopConfetti();
  show('menu');
  unlockPointer();
}

function pause() {
  if (state !== 'playing') return;
  state = 'paused';
  fn.set_paused(true);
  show('pause');
  audio.silenceLoops();
}

function resume() {
  if (state !== 'paused') return;
  show('pause', false); show('settings', false);
  state = 'playing';
  fn.set_paused(false);
  lockPointer();
}

// ------------------------------------------------------------------------------------------------
// Pointer lock & input
// ------------------------------------------------------------------------------------------------
function locked() { return NOLOCK || document.pointerLockElement === gpuCanvas || document.pointerLockElement === hudCanvas; }
function lockPointer() {
  if (NOLOCK) return;
  try {
    const p = gpuCanvas.requestPointerLock && gpuCanvas.requestPointerLock();
    if (p && p.catch) p.catch(() => {});
  } catch (e) { /* needs a user gesture */ }
}
function unlockPointer() { if (document.pointerLockElement) document.exitPointerLock(); }

document.addEventListener('pointerlockchange', () => {
  const hint = $('click-to-play');
  if (locked()) { hint.style.display = 'none'; return; }
  if (state === 'playing' && !overlay) {
    // Esc releases the lock: that is the pause button
    pause();
  }
});

function gameKey(code) {
  return /^(Key[A-Z]|Digit[1-9]|Space|Shift(Left|Right)|Control(Left|Right)|Arrow(Up|Down|Left|Right))$/.test(code);
}

window.addEventListener('keydown', (e) => {
  if (e.repeat) { if (state === 'playing') e.preventDefault(); return; }
  if (e.code === 'F3') { cfg.perf = !cfg.perf; hud.showPerf = cfg.perf; $('set-fps').checked = cfg.perf; saveCfg(); e.preventDefault(); return; }
  if (state === 'playing') {
    if (e.code === 'Tab') { e.preventDefault(); toggleOverlay('inventory'); return; }
    if (e.code === 'KeyM') { e.preventDefault(); toggleOverlay('map'); return; }
    if (e.code === 'Escape' && overlay) { e.preventDefault(); toggleOverlay(overlay); return; }
    if (e.code === 'Escape' && NOLOCK) { pause(); return; }
    if (!overlay && gameKey(e.code)) { fn.key(e.code, true); if (e.code === 'Space' || e.code.startsWith('Arrow') || e.ctrlKey) e.preventDefault(); }
    if (e.ctrlKey && e.code !== 'ControlLeft') e.preventDefault();
  } else if (state === 'paused' && e.code === 'Escape') {
    resume();
  } else if (state === 'menu' && e.code === 'Enter' && !document.activeElement?.matches('input')) {
    startMatch();
  }
});
window.addEventListener('keyup', (e) => { if (state === 'playing' && gameKey(e.code)) fn.key(e.code, false); });
window.addEventListener('blur', () => { if (ready) fn.release_input(); if (state === 'playing' && !overlay) pause(); });
document.addEventListener('visibilitychange', () => { if (document.hidden) { audio.suspend(); if (state === 'playing' && !overlay) pause(); } else audio.resume(); });

window.addEventListener('mousemove', (e) => { if (state === 'playing' && !overlay && locked()) fn.mouse_move(e.movementX || 0, e.movementY || 0); });
window.addEventListener('mousedown', (e) => {
  if (state === 'playing' && !overlay) {
    if (!locked()) { lockPointer(); return; }
    fn.mouse_button(e.button, true);
  }
});
window.addEventListener('mouseup', (e) => { if (state === 'playing') fn.mouse_button(e.button, false); });
window.addEventListener('wheel', (e) => { if (state === 'playing' && !overlay && locked()) fn.wheel(e.deltaY); }, { passive: true });
window.addEventListener('contextmenu', (e) => e.preventDefault());
window.addEventListener('resize', resizeAll);

// ------------------------------------------------------------------------------------------------
// Overlays: inventory and map
// ------------------------------------------------------------------------------------------------
function toggleOverlay(name) {
  if (overlay === name) {
    overlay = null; show(name, false);
    lockPointer();
    return;
  }
  if (overlay) show(overlay, false);
  overlay = name; show(name);
  fn.release_input();
  unlockPointer();
  if (name === 'inventory') renderInventory(true);
  if (name === 'map') drawMapOverlay();
  audio.playUi('ui_click', 0.4);
}

let wdefs = null, cdefs = null;
let invSig = '';
function renderInventory(force) {
  if (!hudState) return;
  if (!wdefs) { try { wdefs = JSON.parse(fn.weapon_defs()); cdefs = JSON.parse(fn.consumable_defs()); } catch (e) { wdefs = []; cdefs = []; } }
  const sig = JSON.stringify([hudState.slots, hudState.sel, hudState.mats, hudState.ammo]);
  if (!force && sig === invSig) return;
  invSig = sig;
  const grid = $('inv-grid');
  grid.innerHTML = '';
  hudState.slots.forEach((slot, i) => {
    const el = document.createElement('div');
    el.className = 'slot' + (slot ? '' : ' empty') + (hudState.sel === i ? ' sel' : '');
    const rar = slot ? (slot.t === 'p' ? 0 : slot.r) : 0;
    el.style.background = slot ? `linear-gradient(180deg, ${RARITY[rar]}44, ${RARITY_DARK[rar]}cc)` : '';
    let html = `<div class="num">${i + 1}</div>`;
    if (slot) {
      const name = slot.n;
      let sub = '';
      if (slot.t === 'w') {
        const d = wdefs[slot.k];
        const dmg = d.damage * (1 + 0.05 * slot.r);
        sub = `<div class="sub">${RARITY_NAMES[slot.r]} &middot; ${AMMO_NAMES[d.ammo]}</div>` +
          stat('Damage', Math.min(1, dmg * (d.pellets > 1 ? d.pellets * 0.35 : 1) / 110), Math.round(dmg) + (d.pellets > 1 ? ` x${d.pellets}` : '')) +
          stat('Rate', Math.min(1, d.rate / 11), d.rate.toFixed(1) + '/s') +
          stat('Magazine', Math.min(1, d.mag / 30), `${slot.a}/${d.mag}`) +
          stat('Range', Math.min(1, d.range / 400), Math.round(d.range) + 'm');
      } else if (slot.t === 'c') {
        const d = cdefs[slot.k];
        sub = `<div class="sub">${RARITY_NAMES[slot.r]} &middot; x${slot.c}</div>` +
          (d.heal ? stat('Heals', d.heal / 100, `+${d.heal} (max ${d.maxHp})`) : '') + (d.shield ? stat('Shield', d.shield / 100, `+${d.shield} (max ${d.maxShield})`) : '') + stat('Use time', Math.min(1, d.time / 10), d.time.toFixed(1) + 's');
      } else sub = '<div class="sub">Harvest trees, rocks and enemy structures for building materials.</div>';
      html += `<canvas width="300" height="170"></canvas><div class="name" style="color:${RARITY[rar]}">${name}</div>${sub}`;
      if (slot.t !== 'p') html += `<button class="drop" data-i="${i}">Drop</button>`;
      html += `<div class="bar" style="background:${RARITY[rar]}"></div>`;
    } else html += '<div class="name" style="margin-top:84px;opacity:.5">Empty</div>';
    el.innerHTML = html;
    grid.appendChild(el);
    if (slot) {
      const c = el.querySelector('canvas');
      const cx = c.getContext('2d');
      drawItem(cx, slot, 10, 10, 280, 150);
      el.addEventListener('click', () => { fn.inventory_select(i); audio.playUi('ui_click', 0.4); });
      const drop = el.querySelector('.drop');
      if (drop) drop.addEventListener('click', (ev) => { ev.stopPropagation(); fn.inventory_drop(i); audio.playUi('ui_back', 0.4); });
    }
  });
  const foot = $('inv-foot');
  foot.innerHTML = '';
  for (let i = 0; i < 3; i++) foot.innerHTML += `<div class="chip"><i style="background:${MAT_COLORS[i]}"></i>${MAT_NAMES[i]} <b>${hudState.mats[i]}</b></div>`;
  for (let i = 0; i < 5; i++) foot.innerHTML += `<div class="chip"><i style="background:${AMMO_COLORS[i]}"></i>${AMMO_NAMES[i]} <b>${hudState.ammo[i]}</b></div>`;
}
function stat(label, frac, text) {
  return `<div class="stat"><span style="width:62px">${label}</span><i style="width:${Math.round(Math.max(0.04, Math.min(1, frac)) * 70)}px"></i><span>${text}</span></div>`;
}

function drawMapOverlay() {
  if (!hudState) return;
  drawFullMap($('mapcanvas'), mapImage, hudState, pois);
}

// ------------------------------------------------------------------------------------------------
// End of match
// ------------------------------------------------------------------------------------------------
let confettiRaf = 0;
function startConfetti() {
  const c = $('confetti');
  c.width = innerWidth; c.height = innerHeight;
  const ctx = c.getContext('2d');
  const parts = Array.from({ length: 170 }, () => ({ x: Math.random() * c.width, y: -Math.random() * c.height, vx: (Math.random() - 0.5) * 2, vy: 2 + Math.random() * 4, r: Math.random() * 6, w: 6 + Math.random() * 8, h: 10 + Math.random() * 10, col: ['#ffe94a', '#3da6ff', '#ff5a8a', '#6dff7a', '#ffffff', '#b65dff'][Math.floor(Math.random() * 6)], vr: (Math.random() - 0.5) * 0.3 }));
  cancelAnimationFrame(confettiRaf);
  const step = () => {
    ctx.clearRect(0, 0, c.width, c.height);
    for (const p of parts) {
      p.x += p.vx + Math.sin(p.y * 0.01) * 0.8; p.y += p.vy; p.r += p.vr;
      if (p.y > c.height + 20) { p.y = -20; p.x = Math.random() * c.width; }
      ctx.save(); ctx.translate(p.x, p.y); ctx.rotate(p.r); ctx.fillStyle = p.col; ctx.fillRect(-p.w / 2, -p.h / 2, p.w, p.h); ctx.restore();
    }
    confettiRaf = requestAnimationFrame(step);
  };
  step();
}
function stopConfetti() { cancelAnimationFrame(confettiRaf); const c = $('confetti'); c.getContext('2d').clearRect(0, 0, c.width, c.height); }

function maybeShowEnd(s) {
  if (endShown || state !== 'playing') return;
  const won = s.ph === 2 && s.won;
  const lost = s.dead && s.deadT > 2.4 && !spectating;
  const over = s.ph === 2;
  if (!(won || lost || (over && s.dead && !spectating))) return;
  endShown = true;
  if (overlay) { show(overlay, false); overlay = null; }
  const el = $('over');
  el.classList.toggle('win', !!won); el.classList.toggle('lose', !won);
  $('over-banner').textContent = won ? 'Victory Royale' : 'Eliminated';
  const place = s.stats.place;
  $('over-place').innerHTML = won ? `You are the last one standing` : `You placed <b>#${place}</b>` + (s.killer ? ` &middot; eliminated by <b>${s.killer}</b>` : ' &middot; eliminated by the storm');
  const mins = Math.floor(s.stats.time / 60), secs = Math.floor(s.stats.time % 60);
  $('over-stats').innerHTML = `<div class="stat"><b>${s.stats.kills}</b><span>Eliminations</span></div><div class="stat"><b>${Math.round(s.stats.dmg)}</b><span>Damage dealt</span></div><div class="stat"><b>${mins}:${String(secs).padStart(2, '0')}</b><span>Survived</span></div><div class="stat"><b>#${place}</b><span>Placement</span></div>`;
  $('btn-spectate').style.display = won || over ? 'none' : '';
  show('over');
  unlockPointer();
  if (won) startConfetti();
}

// ------------------------------------------------------------------------------------------------
// Frame loop
// ------------------------------------------------------------------------------------------------
function loop(t) {
  requestAnimationFrame(loop);
  const dt = Math.min(0.1, (t - lastFrame) / 1000);
  lastFrame = t;
  if (!ready) return;
  if (state === 'loading') { return; }
  fn.frame(dt);
  window.__game.frames++;
  if (state === 'playing' || state === 'paused') {
    try { hudState = JSON.parse(fn.hud()); } catch (e) { hudState = null; }
    if (hudState) {
      hud.draw(hudState, dt);
      if (state === 'playing') {
        const cues = fn.audio_cues();
        if (cues.length) audio.playCues(cues);
        audio.setLoops(fn.audio_loops());
        maybeShowEnd(hudState);
        if (overlay === 'inventory') renderInventory(false);
        if (overlay === 'map') drawMapOverlay();
        if (!locked() && !overlay && state === 'playing') $('click-to-play').style.display = 'block';
      }
    }
  } else hud.clear();
  perfT += dt;
  if (cfg.perf && perfT > 0.4 && (state === 'playing' || state === 'paused')) {
    perfT = 0;
    try { const p = JSON.parse(fn.debug('timings')); hud.perf = `${Math.round(1 / Math.max(dt, 0.001))} fps  frame ${p.frame.toFixed(1)}ms (update ${p.update.toFixed(1)} scene ${p.scene.toFixed(1)} render ${p.render.toFixed(1)})  ${p.draws} draws  ${(p.tris / 1000).toFixed(0)}k tris  ${p.instances} inst`; } catch (e) { /* ignore */ }
  }
}

boot();
