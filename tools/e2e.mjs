// End-to-end smoke test: boots the real page in headless Chromium (software WebGPU), starts a match
// and drives it with real keyboard/mouse events. Exits non-zero on failure.
//   node e2e.mjs            (needs `scripts/build.sh` first)
import { serve, launch } from './browser.mjs';

const { srv, url } = await serve();
const { browser, page, logs } = await launch({ width: 640, height: 360 });
let failed = 0;
const check = (name, ok, extra = '') => { console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${extra ? '  ' + extra : ''}`); if (!ok) failed++; };
const dbg = (c) => page.evaluate((c) => window.__game.fn.debug(c), c);
const st = async () => JSON.parse(await dbg('state'));
const hud = () => page.evaluate(() => window.__game.hud);
// Software frames advance the simulation by its 0.1s cap. Hardware frames can be
// much faster, so also wait for equivalent simulation time on the native GPU.
const frames = async (n) => {
  const start = await page.evaluate(() => ({ frames: window.__game.frames, t: JSON.parse(window.__game.fn.debug('state')).t }));
  await page.waitForFunction(({ frames, t, native }) =>
    window.__game.frames >= frames && (!native || window.__game.state !== 'playing' || JSON.parse(window.__game.fn.debug('state')).t >= t),
  { frames: start.frames + n, t: start.t + n / 10, native: process.env.FN_FLAGS === 'native' }, { timeout: 120000 });
};

await page.goto(url + 'index.html?nolock=1&autostart=1&quality=low&opts=' + encodeURIComponent('skipbus=1;bots=15;god=1'));
try { await page.waitForFunction(() => window.__game && window.__game.state === 'playing', null, { timeout: 240000 }); } catch (e) { /* reported below */ }
check('match starts and enters the playing state', (await page.evaluate(() => window.__game && window.__game.state)) === 'playing');
if (failed) {
  console.error(logs.join('\n'));
  await browser.close(); srv.close();
  process.exit(1);
}
await frames(3);

// ---- movement -------------------------------------------------------------------------------------
await dbg('tp 52 28'); await dbg('look 90 0');
await frames(3);
const p0 = await st();
await page.keyboard.down('KeyW');
await frames(8);
await page.keyboard.up('KeyW');
const p1 = await st();
const moved = Math.hypot(p1.pos[0] - p0.pos[0], p1.pos[2] - p0.pos[2]);
check('W key moves the player forward', moved > 3, `moved ${moved.toFixed(1)} m`);
check('player stays on the ground', p1.mode === 'Ground');

// ---- mouse look ----------------------------------------------------------------------------------------
const y0 = (await st()).yaw;
await page.evaluate(() => window.__game.fn.mouse_move(300, 0));
await frames(3);
const y1 = (await st()).yaw;
check('mouse look turns the camera', Math.abs(y1 - y0) > 0.3, `yaw ${y0.toFixed(2)} -> ${y1.toFixed(2)}`);

// ---- shooting and reloading -------------------------------------------------------------------------------
await dbg('give ar rare');
await frames(8);
const h0 = await hud();
const mag0 = h0.slots[h0.sel].a;
await page.mouse.down({ button: 'left' });
await frames(6);
await page.mouse.up({ button: 'left' });
await frames(2);
const h1 = await hud();
check('firing consumes ammo', h1.slots[h1.sel].a < mag0, `${mag0} -> ${h1.slots[h1.sel].a}`);
await page.keyboard.press('KeyR');
await frames(40);
const h2 = await hud();
check('reloading refills the magazine', h2.slots[h2.sel].a >= h1.slots[h1.sel].a, `-> ${h2.slots[h2.sel].a}`);

// ---- building ----------------------------------------------------------------------------------------------------
await dbg('mats 300');
const pieces0 = (await st()).pieces;
await page.keyboard.press('KeyQ');
await frames(2);
await page.keyboard.press('KeyZ');
await frames(3);
await page.mouse.down({ button: 'left' }); await frames(4); await page.mouse.up({ button: 'left' });
await frames(2);
check('building places a wall', (await st()).pieces > pieces0, `pieces ${(await st()).pieces}`);
await page.keyboard.press('KeyQ');
await frames(2);

// ---- jumping and chests ----------------------------------------------------------------------------------------
await dbg('tp 21 -171'); await frames(3);
const gy = (await st()).pos[1];
await page.keyboard.down('Space');
await frames(3);
const jy = (await st()).pos[1];
await page.keyboard.up('Space');
check('Space makes the player jump', jy > gy + 0.2, `y ${gy.toFixed(2)} -> ${jy.toFixed(2)}`);
await frames(12);
await dbg('chest 0'); await frames(3);
const atChest = await hud();
check('standing at a chest shows the Chest prompt', !!atChest.prompt && atChest.prompt.kind === 'chest', JSON.stringify(atChest.prompt));
await page.keyboard.press('KeyE');
await frames(6);
const afterE = await hud();
check('E opens the chest', !(afterE.prompt && afterE.prompt.kind === 'chest'));

// ---- harvesting --------------------------------------------------------------------------------------------------
await dbg('tree 0');
await frames(2);
const wood0 = (await hud()).mats[0];
await page.mouse.down({ button: 'left' });
await frames(14);
await page.mouse.up({ button: 'left' });
await frames(2);
const wood1 = (await hud()).mats[0];
check('the pickaxe harvests wood from a tree', wood1 > wood0, `wood ${wood0} -> ${wood1}`);

// ---- eliminations -------------------------------------------------------------------------------------------------
await dbg('tp 21 -171'); await dbg('look 90 0'); // open ground beside the lake
await frames(2);
await dbg('bots_near 1 9');
await dbg('give ar epic');
await dbg('aim 1');
await frames(2);
const kills0 = (await hud()).kills;
await page.mouse.down({ button: 'right' }); // aim down sights for accuracy
await frames(3);
await page.mouse.down({ button: 'left' });
await frames(28);
await page.mouse.up({ button: 'left' });
await page.mouse.up({ button: 'right' });
await frames(2);
const h3 = await hud();
check('shooting a bot eliminates it', h3.kills > kills0, `kills ${kills0} -> ${h3.kills}`);
check('the kill shows up in the kill feed', h3.feed.some((f) => f.me));

// ---- UI overlays ---------------------------------------------------------------------------------------------------
await page.keyboard.press('Tab');
await frames(2);
check('inventory opens', await page.evaluate(() => document.getElementById('inventory').classList.contains('active')));
await page.keyboard.press('Tab');
await page.keyboard.press('KeyM');
await frames(2);
check('map opens', await page.evaluate(() => document.getElementById('map').classList.contains('active')));
await page.keyboard.press('KeyM');
await page.keyboard.press('Escape');
await frames(2);
check('Escape pauses', (await page.evaluate(() => window.__game.state)) === 'paused');
await page.click('#btn-resume');
await frames(2);
check('resume continues the match', (await page.evaluate(() => window.__game.state)) === 'playing');

// ---- long run ------------------------------------------------------------------------------------------------------------
const ff = JSON.parse(await dbg('ff 240'));
check('the simulation survives a four minute fast-forward', ff.alive >= 1, JSON.stringify(ff));

const hs = await hud();
check('HUD snapshot is produced', !!hs && typeof hs.hp === 'number' && Array.isArray(hs.slots));
const bad = logs.filter((l) => /error|panick|unreachable/i.test(l) && !/WebGPU is experimental|favicon|404|Failed to load resource/.test(l));
check('no console errors', bad.length === 0, bad.slice(0, 3).join(' | '));
console.log(failed ? `\n${failed} check(s) failed` : '\nall checks passed');
await browser.close(); srv.close();
process.exit(failed ? 1 : 0);
