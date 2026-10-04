// Random UI fuzzing of the page's state machine (menus, pause, overlays, end screens, pointer lock):
// fires random keys, clicks, blur events and lock releases and checks that the visible screens always
// agree with the game state.   node fuzz_ui.mjs [seed] [steps]      (needs `scripts/build.sh` first)
import { serve, launch } from './browser.mjs';

let seed = +process.argv[2] || 1;
const steps = +process.argv[3] || 120;
const rnd = () => (seed = (seed * 1664525 + 1013904223) >>> 0) / 4294967296;
const pick = (a) => a[Math.floor(rnd() * a.length)];

const KEYS = ['Escape', 'Escape', 'Tab', 'KeyM', 'Enter', 'KeyB', 'KeyW', 'KeyA', 'KeyS', 'KeyD', 'Space', 'Digit1', 'Digit2', 'Digit3', 'KeyQ', 'KeyZ', 'KeyX', 'KeyC', 'KeyV', 'KeyT', 'KeyG', 'KeyE', 'KeyR', 'ControlLeft', 'F3', 'ArrowLeft', 'ArrowRight'];
const BUTTONS = ['#btn-play', '#btn-settings', '#btn-help', '#btn-settings-close', '#btn-help-close', '#btn-resume', '#btn-pause-settings', '#btn-pause-help', '#btn-quit', '#btn-again', '#btn-menu', '#btn-spectate'];
const DEBUG = ['kill_me', 'kill_bots', 'tp 21 -171', 'hurt 20 0', 'give ar epic', 'storm 3'];
const SCREENS = ['menu', 'loading', 'pause', 'over', 'inventory', 'map', 'settings', 'help', 'nowebgpu'];

const { srv, url } = await serve();
const { browser, page, logs } = await launch({ width: 800, height: 450 });
await page.goto(url + 'index.html?opts=' + encodeURIComponent('skipbus=1;bots=8;god=1'));
await page.waitForFunction(() => window.__game && window.__game.state === 'menu', null, { timeout: 240000 });

const snapshot = () => page.evaluate((screens) => ({
  state: window.__game.state, overlay: window.__game.overlay, waiting: window.__game.waiting,
  active: screens.filter((id) => document.getElementById(id).classList.contains('active')),
  locked: !!document.pointerLockElement,
}), SCREENS);

function violations(s) {
  const v = [];
  const has = (id) => s.active.includes(id);
  if (s.state === 'menu') { if (!has('menu')) v.push('menu state without the menu'); for (const id of ['pause', 'over', 'inventory', 'map']) if (has(id)) v.push(`${id} visible in the menu`); }
  if (s.state === 'playing') { for (const id of ['menu', 'pause', 'over']) if (has(id)) v.push(`${id} visible while playing`); if (has('inventory') !== (s.overlay === 'inventory')) v.push('inventory screen and overlay disagree'); if (has('map') !== (s.overlay === 'map')) v.push('map screen and overlay disagree'); }
  if (s.state === 'paused') { if (!has('pause')) v.push('paused without the pause menu'); for (const id of ['over', 'menu']) if (has(id)) v.push(`${id} visible while paused`); }
  if (s.state === 'over') { if (!has('over')) v.push('over state without the end screen'); for (const id of ['pause', 'menu']) if (has(id)) v.push(`${id} visible on the end screen`); }
  if (s.waiting && s.state !== 'playing') v.push('waiting for the lock outside of play');
  if (s.overlay && s.state !== 'playing') v.push('overlay open outside of play');
  return v;
}

let bad = 0;
const history = [];
for (let i = 0; i < steps; i++) {
  const r = rnd();
  let what;
  try {
    if (r < 0.50) { const k = pick(KEYS); what = `key ${k}`; await page.keyboard.press(k, { delay: 20 }); }
    else if (r < 0.72) { const b = pick(BUTTONS); what = `click ${b}`; await page.click(b, { timeout: 150 }); }
    else if (r < 0.80) { what = 'click in the page'; await page.mouse.click(100 + rnd() * 600, 80 + rnd() * 300); }
    else if (r < 0.86) { what = 'blur'; await page.evaluate(() => window.dispatchEvent(new Event('blur'))); }
    else if (r < 0.92) { what = 'exitPointerLock'; await page.evaluate(() => document.exitPointerLock()); }
    else { const d = pick(DEBUG); what = `debug ${d}`; await page.evaluate((d) => window.__game.fn.debug(d), d); }
  } catch (e) { what += ' (not possible)'; }
  history.push(what);
  // let events settle; loading can take a few seconds
  try { await page.waitForFunction(() => window.__game.state !== 'loading', null, { timeout: 120000 }); } catch (e) { /* reported below */ }
  await page.waitForTimeout(250);
  const s = await snapshot();
  const v = violations(s);
  if (v.length) { bad++; console.log(`step ${i} after "${what}": ${v.join('; ')}  ${JSON.stringify(s)}`); console.log('  recent:', history.slice(-6).join(' | ')); }
}
const errors = logs.filter((l) => /error|pageerror|unreachable/i.test(l) && !/WebGPU is experimental|Failed to load resource/.test(l));
console.log(`seed ${process.argv[2] || 1}: ${steps} steps, ${bad} invariant violations, ${errors.length} console errors`);
for (const e of errors.slice(0, 5)) console.log('  ', e);
await browser.close(); srv.close();
process.exit(bad || errors.length ? 1 : 0);
