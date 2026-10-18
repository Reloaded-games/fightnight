// Grenades, the Launch Pad, Slurp Juice, ammo boxes and Supply Llamas in the real page: the hotbar names them, a click
// throws a grenade or sets a pad down, stepping on the pad launches the player, a Slurp heals over time, and the two new
// containers have their own prompts and loot.
//   node utility_e2e.mjs            (needs `scripts/build.sh` first)
import { serve, launch } from './browser.mjs';

const { srv, url } = await serve();
const { browser, page, logs } = await launch({ width: 960, height: 540 });
let failed = 0;
const check = (name, ok, extra = '') => { console.log(`${ok ? 'PASS' : 'FAIL'}  ${name} ${extra}`); if (!ok) failed++; };
const dbg = (c) => page.evaluate((c) => window.__game.fn.debug(c), c);
const state = async () => JSON.parse(await dbg('state'));
const hud = () => page.evaluate(() => JSON.parse(window.__game.fn.hud()));
// (software rendering in headless Chromium manages well under one frame a second)
const frames = async (n) => { const f0 = await page.evaluate(() => window.__game.frames); await page.waitForFunction((t) => window.__game.frames >= t, f0 + n, { timeout: 300000 }); };
const click = async () => { await page.mouse.down({ button: 'left' }); await frames(2); await page.mouse.up({ button: 'left' }); await frames(1); };
const hold = async (code, n) => { await page.keyboard.down(code); await frames(n); await page.keyboard.up(code); };

try {
  await page.goto(url + 'index.html?nolock=1&autostart=1&quality=low&opts=' + encodeURIComponent('skipbus=1;bots=3;god=1'));
  await page.waitForFunction(() => window.__game?.state === 'playing', null, { timeout: 240000 });
  await dbg('freeze_bots');
  await frames(2);

  const labels = await page.evaluate(async () => {
    const { slotLabel } = await import('./hud.js');
    return [0, 1, 2, 3].map((k) => slotLabel({ t: 'u', k })).concat(slotLabel({ t: 'c', k: 5 }));
  });
  check('utility items and Slurp Juice are labelled on the hotbar', labels.join(',') === 'FRAG,IMPULSE,BOOGIE,LAUNCH PAD,SLURP', labels.join(','));

  // ---- a frag grenade: in hand, thrown with a click, gone off after its fuse ----------------------------------------
  await dbg('util frag 3');
  await frames(3);
  let h = await hud();
  const held = h.slots[h.sel];
  check('the grenade is in hand', !!held && held.t === 'u' && held.n === 'Frag Grenade' && held.c === 3 && held.throw === true, JSON.stringify(held));
  await click();
  let s = await state();
  h = await hud();
  check('a click throws one', s.thrown === 1 && h.slots[h.sel]?.c === 2, `thrown ${s.thrown}, left ${h.slots[h.sel]?.c}`);
  await dbg('ff 3');
  await frames(1);
  check('it goes off', (await state()).thrown === 0);

  // ---- the Launch Pad: set down with a click; walking onto it flings the player into a glide -------------------------
  await dbg('util pad 1');
  await frames(3);
  await click();
  s = await state();
  check('a click sets the Launch Pad down', s.traps === 1, `traps ${s.traps}`);
  let flew = false;
  await page.keyboard.down('KeyW');
  for (let k = 0; k < 10 && !flew; k++) {
    await frames(1);
    flew = (await state()).mode === 'Glide';
  }
  await page.keyboard.up('KeyW');
  check('stepping on it launches the player into a glide', flew, JSON.stringify(await state()));
  await dbg('ff 30');
  await frames(1);
  check('and they come back down', (await state()).mode === 'Ground', JSON.stringify(await state()));

  // ---- Slurp Juice: health, then shield, a little at a time --------------------------------------------------------------
  await dbg('hurt 40 0');
  await dbg('cons slurp 1');
  await frames(1);
  h = await hud();
  const slurp = h.slots.findIndex((x) => x && x.t === 'c' && x.k === 5);
  check('the Slurp Juice is in the hotbar', slurp > 0, JSON.stringify(h.slots));
  await hold('Digit' + (slurp + 1), 1);
  await frames(4);
  await click();
  await dbg('ff 2.5');
  await frames(1);
  h = await hud();
  const hp0 = h.hp;
  check('once drunk it keeps working', h.regen === true && hp0 < 50, `regen ${h.regen}, hp ${hp0}`);
  await dbg('ff 5');
  await frames(1);
  h = await hud();
  check('health comes back over time', h.hp >= hp0 + 8 && h.hp <= hp0 + 12, `${hp0} -> ${h.hp}`);

  // ---- ammo boxes and Supply Llamas ------------------------------------------------------------------------------------
  // (the nearest thing in reach is what E takes: skip boxes with loose loot lying right beside them)
  for (let k = 0; k < 6; k++) {
    await dbg(`chest ${k} ammobox`);
    await frames(3);
    h = await hud();
    if (h.prompt?.kind === 'ammobox') break;
  }
  check('beside an ammo box the prompt offers to search it', h.prompt?.kind === 'ammobox', JSON.stringify(h.prompt));
  let before = (await state()).pickups;
  await page.keyboard.press('KeyE');
  await frames(2);
  check('E opens it and ammunition comes out', (await state()).pickups >= before + 2, `${before} -> ${(await state()).pickups}`);
  await dbg('chest 0 llama');
  await frames(3);
  h = await hud();
  check('a Supply Llama has its own prompt', h.prompt?.kind === 'llama', JSON.stringify(h.prompt));
  before = (await state()).pickups;
  await page.keyboard.press('KeyE');
  await frames(2);
  check('opening it bursts out a pile of loot', (await state()).pickups >= before + 10, `${before} -> ${(await state()).pickups}`);

  // ---- the inventory screen describes the grenade ----------------------------------------------------------------------
  await page.keyboard.press('Tab');
  await frames(2);
  const inv = await page.evaluate(() => document.getElementById('inv-grid').innerText);
  check('the inventory screen explains what the grenade does', /short fuse/i.test(inv), inv.replace(/\s+/g, ' ').slice(0, 160));
  await page.keyboard.press('Tab');
  await frames(1);

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
