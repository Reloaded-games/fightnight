// Supply drops and scattered chests in the real page: a crate is let go by the match clock, shows on the HUD while it is
// in the air, lands, can be opened with E, and the island has chests out in the open.
//   node supply_e2e.mjs            (needs `scripts/build.sh` first)
import { serve, launch } from './browser.mjs';

const { srv, url } = await serve();
const { browser, page, logs } = await launch({ width: 960, height: 540 });
let failed = 0;
const check = (name, ok, extra = '') => { console.log(`${ok ? 'PASS' : 'FAIL'}  ${name} ${extra}`); if (!ok) failed++; };
const dbg = (c) => page.evaluate((c) => window.__game.fn.debug(c), c);
const state = async () => JSON.parse(await dbg('state'));
const hud = () => page.evaluate(() => JSON.parse(window.__game.fn.hud()));
const frames = async (n) => { const f0 = await page.evaluate(() => window.__game.frames); await page.waitForFunction((t) => window.__game.frames >= t, f0 + n, { timeout: 120000 }); };

try {
  await page.goto(url + 'index.html?nolock=1&autostart=1&quality=low&opts=' + encodeURIComponent('skipbus=1;bots=3;god=1'));
  await page.waitForFunction(() => window.__game?.state === 'playing', null, { timeout: 240000 });
  await dbg('freeze_bots');
  await frames(2);
  check('no supply drop before the match is under way', (await hud()).drops.length === 0);

  // the clock lets the first crate go after about fifty seconds
  await dbg('ff 52');
  await frames(2);
  let h = await hud();
  check('a supply drop is on its way after a minute', h.drops.length === 1 && h.drops[0].air === true, JSON.stringify(h.drops));
  check('the player is told about it', h.toast.some((t) => /supply drop/i.test(t[0])), JSON.stringify(h.toast));
  const [x, z] = [h.drops[0].x, h.drops[0].z];

  // it comes down, and then it waits to be opened
  await dbg('ff 25');
  await frames(2);
  h = await hud();
  check('the crate has landed', h.drops.length === 1 && h.drops[0].air === false, JSON.stringify(h.drops));
  check('it landed where it was coming down', Math.abs(h.drops[0].x - x) < 0.5 && Math.abs(h.drops[0].z - z) < 0.5);

  // stand beside it: the prompt asks to open it, and E does
  await dbg(`tp ${x} ${z + 1.6}`);
  await frames(3);
  h = await hud();
  check('beside it the prompt offers the supply drop', h.prompt && h.prompt.kind === 'supply', JSON.stringify(h.prompt));
  const before = (await state()).pickups;
  await page.evaluate(() => window.__game.fn.key('KeyE', true));
  await frames(2);
  await page.evaluate(() => window.__game.fn.key('KeyE', false));
  await frames(3);
  h = await hud();
  check('E opens it', h.drops.length === 0);
  check('the loot comes out', (await state()).pickups >= before + 4, `${before} -> ${(await state()).pickups}`);

  // a crate seen from the ground is drawn: the page keeps rendering with the new meshes
  await dbg('supply 12 30 0');
  await frames(4);
  check('the page keeps running with a crate in the sky', (await hud()).drops.length === 1);
  const errors = logs.filter((l) => /error|panick/i.test(l) && !/favicon|404|WebGPU is experimental/.test(l));
  check('no console errors', errors.length === 0, errors.join(' | '));
} catch (e) {
  console.log('FAIL  the test crashed:', e.message);
  failed++;
}
await browser.close();
srv.close();
console.log(failed ? `${failed} check(s) failed` : 'all checks passed');
process.exit(failed ? 1 : 0);
