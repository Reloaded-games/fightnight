// End-to-end test of the page's UI state machine with the REAL pointer lock (e2e.mjs bypasses it with nolock=1):
// play, pause by releasing the lock, resume, win a match, end screen, play again, menu keys and settings.
//   node e2e_flow.mjs            (needs `scripts/build.sh` first)
import { serve, launch } from './browser.mjs';

const { srv, url } = await serve();
const { browser, context, page, logs } = await launch({ width: 1920, height: 1080 });
let failed = 0;
const check = (name, ok, extra = '') => { console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${extra ? '  ' + extra : ''}`); if (!ok) failed++; };
const ev = (p, f, arg) => p.evaluate(f, arg);
const state = (p) => ev(p, () => window.__game.state);
const active = (p, id) => ev(p, (id) => document.getElementById(id).classList.contains('active'), id);
const locked = (p) => ev(p, () => !!document.pointerLockElement);
const until = (p, f, arg, timeout = 240000) => p.waitForFunction(f, arg, { timeout });
// like until(), but reports false instead of throwing (software rendering can delay events by seconds)
const settles = async (p, f, arg, timeout = 90000) => { try { await p.waitForFunction(f, arg, { timeout }); return true; } catch (e) { return false; } };
const frames = async (p, n) => { const f0 = await ev(p, () => window.__game.frames); await until(p, (t) => window.__game.frames >= t, f0 + n, 120000); };

const OPTS = encodeURIComponent('skipbus=1;bots=10;god=1');
await page.goto(url + 'index.html?opts=' + OPTS);
await until(page, () => window.__game && window.__game.state === 'menu');
check('the menu appears after loading', true);

// ---- menu keys --------------------------------------------------------------------------------------------
await page.click('#btn-help');
await page.keyboard.press('Enter');
await page.waitForTimeout(400);
check('Enter does not start a match while How to play is open', (await state(page)) === 'menu' && (await active(page, 'help')));
await page.click('#btn-help-close');

// ---- graphics preset resizes the canvas -------------------------------------------------------------------
const w0 = await ev(page, () => document.getElementById('gpu').width);
await page.click('#seg-quality button[data-v="low"]');
await frames(page, 2);
const w1 = await ev(page, () => document.getElementById('gpu').width);
check('changing the graphics preset resizes the render canvas', w1 !== w0, `${w0} -> ${w1}`);
await page.click('#seg-quality button[data-v="high"]');
await page.setViewportSize({ width: 960, height: 540 });
await frames(page, 2);

// ---- play, release the lock (= Esc), resume --------------------------------------------------------------------
await page.click('#btn-play');
await until(page, () => window.__game.state === 'playing');
await page.waitForTimeout(800);
check('Play grabs the pointer lock', await locked(page));
await ev(page, () => document.exitPointerLock());
const paused = await settles(page, () => window.__game.state === 'paused');
check('releasing the lock pauses the match', paused && (await active(page, 'pause')), paused ? '' : JSON.stringify(await ev(page, () => ({ state: window.__game.state, overlay: window.__game.overlay, waiting: window.__game.waiting, lock: !!document.pointerLockElement }))));
await page.click('#btn-resume', { timeout: 60000 });
check('Resume continues and re-locks', (await settles(page, () => window.__game.state === 'playing' && !!document.pointerLockElement)));

// ---- G drops from the inventory screen -------------------------------------------------------------------------
await ev(page, () => window.__game.fn.debug('give ar rare'));
await frames(page, 4);
const slotsBefore = (await ev(page, () => window.__game.hud)).slots.filter(Boolean).length;
await page.keyboard.press('Tab');
await frames(page, 2);
await page.keyboard.press('KeyG');
await frames(page, 4);
const slotsAfter = (await ev(page, () => window.__game.hud)).slots.filter(Boolean).length;
check('G drops the selected item from the inventory screen', slotsAfter < slotsBefore, `${slotsBefore} -> ${slotsAfter} items`);
await page.keyboard.press('Tab');
await page.waitForTimeout(500);

// ---- winning: the end screen is its own state, no pause menu behind it ---------------------------------------------
await ev(page, () => window.__game.fn.debug('kill_bots'));
await until(page, () => window.__game.state === 'over', null, 120000);
await page.waitForTimeout(300);
check('winning shows the Victory screen', (await active(page, 'over')) && (await ev(page, () => document.getElementById('over-banner').textContent)) === 'Victory Royale');
check('the pause menu does not appear behind it', !(await active(page, 'pause')));
check('the pointer is released on the end screen', !(await locked(page)));
await page.click('#btn-again');
await until(page, () => window.__game.state === 'playing');
check('Play again starts a fresh match without leftovers', !(await active(page, 'over')) && !(await active(page, 'pause')));

// ---- dance emote ---------------------------------------------------------------------------------------------------------------------------
await ev(page, () => window.__game.fn.debug('tp 21 -171'));
await frames(page, 3);
await page.keyboard.press('KeyB');
await frames(page, 3);
const dancing = JSON.parse(await ev(page, () => window.__game.fn.debug('state'))).emoting;
await page.keyboard.down('KeyW');
await frames(page, 3);
await page.keyboard.up('KeyW');
const stopped = !JSON.parse(await ev(page, () => window.__game.fn.debug('state'))).emoting;
check('B starts the dance emote and moving ends it', dancing && stopped, `dancing ${dancing}, ended by moving ${stopped}`);

// ---- being eliminated: result screen, spectate, and the final result afterwards ---------------------------------------------------
await ev(page, () => window.__game.fn.debug('kill_me'));
await until(page, () => window.__game.state === 'over', null, 180000);
check('being eliminated shows the Eliminated screen', (await active(page, 'over')) && (await ev(page, () => document.getElementById('over-banner').textContent)) === 'Eliminated');
check('the Eliminated screen names the killer', /eliminated by/i.test(await ev(page, () => document.getElementById('over-place').textContent)));
await page.click('#btn-spectate', { timeout: 60000 });
await settles(page, () => window.__game.state === 'playing');
check('Spectate resumes the match without the pause menu', (await state(page)) === 'playing' && !(await active(page, 'over')) && !(await active(page, 'pause')));
await ev(page, () => window.__game.fn.debug('kill_bots'));
await until(page, () => window.__game.state === 'over', null, 120000);
check('the final result still appears after spectating', await active(page, 'over'));
await page.click('#btn-again');
await until(page, () => window.__game.state === 'playing');

// ---- quit to menu and Enter ----------------------------------------------------------------------------------------------------
await ev(page, () => document.exitPointerLock());
await settles(page, () => window.__game.state === 'paused');
await page.click('#btn-quit', { timeout: 60000 });
check('Quit returns to the menu', (await settles(page, () => window.__game.state === 'menu')) && (await active(page, 'menu')));
await page.keyboard.press('Enter');
await until(page, () => window.__game.state === 'playing');
check('Enter on the bare menu starts a match', true);

// ---- no pointer lock available: the match waits behind the "click to play" hint --------------------------------
const p2 = await context.newPage();
await p2.addInitScript(() => { HTMLCanvasElement.prototype.requestPointerLock = () => Promise.reject(new Error('denied')); });
await p2.setViewportSize({ width: 960, height: 540 });
await p2.goto(url + 'index.html?opts=' + OPTS);
await until(p2, () => window.__game && window.__game.state === 'menu');
await p2.click('#btn-play');
await until(p2, () => window.__game.state === 'playing');
await p2.waitForTimeout(1500);
const hintShown = await ev(p2, () => getComputedStyle(document.getElementById('click-to-play')).display !== 'none');
const t0 = JSON.parse(await ev(p2, () => window.__game.fn.debug('state'))).t;
await frames(p2, 3);
const t1 = JSON.parse(await ev(p2, () => window.__game.fn.debug('state'))).t;
check('without a pointer lock the hint is shown and the match is paused', hintShown && Math.abs(t1 - t0) < 0.05, `hint ${hintShown}, match time ${t0} -> ${t1}`);
await p2.keyboard.press('Escape');
check('Esc still opens the pause menu then', (await settles(p2, () => window.__game.state === 'paused')) && (await active(p2, 'pause')));
await p2.close();

// ---- the whole drop: bus -> free fall -> glider -> landing --------------------------------------------------------------------------
const p3 = await context.newPage();
await p3.setViewportSize({ width: 640, height: 360 });
await p3.goto(url + 'index.html?nolock=1&autostart=1&opts=' + encodeURIComponent('bots=10;god=1'));
await until(p3, () => window.__game && window.__game.state === 'playing');
await frames(p3, 3);
const mode = async () => JSON.parse(await ev(p3, () => window.__game.fn.debug('state'))).mode;
check('a match that does not skip the bus starts on the Battle Bus', (await mode()) === 'Bus');
await p3.keyboard.press('Space');
await frames(p3, 4);
check('Space jumps from the bus into free fall', (await mode()) === 'Freefall');
await p3.keyboard.press('Space');
await frames(p3, 4);
check('Space again opens the glider', (await mode()) === 'Glide');
await ev(p3, () => window.__game.fn.debug('ff 90'));
await frames(p3, 3);
const landed = await mode();
check('the glider brings the player down to land', landed === 'Ground' || landed === 'Swim', landed);
await p3.close();

const bad = logs.filter((l) => /error|panick|unreachable/i.test(l) && !/WebGPU is experimental|denied/.test(l));
check('no console errors', bad.length === 0, bad.slice(0, 3).join(' | '));
await browser.close(); srv.close();
console.log(failed ? `\n${failed} check(s) failed` : '\nall checks passed');
process.exit(failed ? 1 : 0);
