// Real browser controls exercise the two new modes and drivable vehicles.
import { serve, launch } from './browser.mjs';
const { srv, url } = await serve();
const { browser, page, logs } = await launch({ width: 960, height: 540 });
let failed = 0;
const check = (name, ok, extra = '') => { console.log(`${ok ? 'PASS' : 'FAIL'}  ${name} ${extra}`); if (!ok) failed++; };
const dbg = (c) => page.evaluate((c) => window.__game.fn.debug(c), c);
const state = async () => JSON.parse(await dbg('state'));
const hud = () => page.evaluate(() => window.__game.hud);
async function waitTime(seconds) {
  const t = (await state()).t + seconds;
  await page.waitForFunction(t => JSON.parse(window.__game.fn.debug('state')).t >= t, t, { timeout: 120000 });
}
async function start(mode) {
  await page.goto(url + 'index.html?nolock=1&autostart=1&quality=low&opts=' + encodeURIComponent(`skipbus=1;bots=3;god=1;mode=${mode}`));
  await page.waitForFunction(() => window.__game?.state === 'playing', null, { timeout: 240000 });
  await waitTime(0.3);
}
try {
  // Use the visible menu controls and reload in the same browser session so
  // saved preferences are tested independently from the autostart overrides.
  await page.goto(url + 'index.html?nolock=1&quality=low');
  await page.waitForFunction(() => window.__game?.state === 'menu', null, { timeout: 240000 });
  const choices = await page.locator('#seg-mode button').allTextContents();
  check('menu offers Battle Royale, Zero Build and LEGO', choices.join(',') === 'Battle Royale,Zero Build,LEGO', choices.join(', '));
  for (const mode of ['zero-build', 'lego']) {
    await page.click(`#seg-mode button[data-v="${mode}"]`);
    check(`${mode} is selected through the visible menu`, await page.getAttribute(`#seg-mode button[data-v="${mode}"]`, 'aria-pressed') === 'true');
    await page.reload();
    await page.waitForFunction(() => window.__game?.state === 'menu', null, { timeout: 240000 });
    const saved = await page.evaluate(() => ({ mode: window.__game.cfg.mode, selected: document.querySelector('#seg-mode button[aria-pressed="true"]')?.dataset.v, description: document.getElementById('mode-description').textContent }));
    check(`${mode} selection persists after reloading`, saved.mode === mode && saved.selected === mode, JSON.stringify(saved));
    check(`${mode} keeps its matching mode description`, mode === 'zero-build' ? saved.description.includes('No building') : saved.description.includes('brick'));
  }
  await page.click('#btn-multi'); await page.click('#btn-mp-host');
  check('host lobby inherits the selected LEGO mode', await page.getAttribute('#lb-mode button[data-v="lego"]', 'aria-pressed') === 'true');
  await page.click('#lb-mode button[data-v="zero-build"]');
  check('lobby mode updates the shared match preference', await page.evaluate(() => window.__game.cfg.mode === 'zero-build' && document.querySelector('#seg-mode button[data-v="zero-build"]').getAttribute('aria-pressed') === 'true'));
  await page.click('#btn-lobby-leave'); await page.click('#btn-mp-back');

  await start('zero-build');
  check('Zero Build options reach simulation', (await hud()).gameMode === 'zero-build');
  check('larger island reaches renderer and HUD', (await hud()).worldSize === 1920);
  check('Zero Build starts without materials', (await hud()).mats.every(n => n === 0));
  await page.keyboard.press('KeyQ'); await page.keyboard.press('KeyZ');
  await page.mouse.down(); await waitTime(0.3); await page.mouse.up();
  check('Zero Build rejects real build controls', !(await hud()).build.on && (await state()).pieces === 0);
  const before = (await hud()).mats;
  await dbg('tree'); await page.mouse.down(); await waitTime(1.5); await page.mouse.up();
  check('Zero Build harvesting awards no building materials', (await hud()).mats.every((n, i) => n === before[i]));

  await start('lego');
  check('LEGO mode reaches the rendered match', (await hud()).gameMode === 'lego');
  await dbg('tp 52 28'); await dbg('mats 300');
  await page.keyboard.press('KeyQ'); await page.keyboard.press('KeyZ');
  await waitTime(0.2); await page.mouse.down(); await waitTime(0.3); await page.mouse.up();
  check('LEGO keeps playable brick construction', (await hud()).build.on && (await state()).pieces > 0);
  await page.keyboard.press('KeyQ');
  check('parked vehicles spawn across the larger island', (await hud()).vehicles.length >= 12);
  await dbg('vehicle 0'); await waitTime(0.2);
  check('parked car has an E interaction prompt', (await hud()).prompt?.kind === 'vehicle', JSON.stringify((await hud()).prompt));
  await page.keyboard.press('KeyE'); await waitTime(0.2);
  check('E enters the buggy', !!(await hud()).vehicle);
  const initial = JSON.parse(await dbg('vehicles'))[0];
  await page.keyboard.down('KeyW'); await waitTime(0.8); await page.keyboard.up('KeyW');
  const moved = JSON.parse(await dbg('vehicles'))[0];
  check('W accelerates and moves the buggy', moved.speed > 3 && Math.hypot(moved.pos[0] - initial.pos[0], moved.pos[2] - initial.pos[2]) > 1, `${moved.speed} m/s`);
  await page.keyboard.down('KeyW'); await page.keyboard.down('KeyD'); await waitTime(0.3); await page.keyboard.up('KeyD'); await page.keyboard.up('KeyW');
  const turned = JSON.parse(await dbg('vehicles'))[0];
  check('D steers the buggy', Math.abs(turned.yaw - moved.yaw) > 0.05);
  await page.keyboard.down('Space'); await waitTime(0.8); await page.keyboard.up('Space');
  check('Space brakes the buggy', Math.abs(JSON.parse(await dbg('vehicles'))[0].speed) < 0.2);
  await page.keyboard.press('KeyE'); await waitTime(0.2);
  check('E exits and releases the driver seat', !(await hud()).vehicle && JSON.parse(await dbg('vehicles'))[0].driver === null);
  const errors = logs.filter(s => /error|validation|panic|uncaught/i.test(s));
  check('no WebGPU validation or runtime errors in new modes', errors.length === 0, errors.slice(0, 2).join('\n'));
} catch (e) { check('new modes complete their browser scenario', false, e.stack || String(e)); }
finally { await browser.close(); srv.close(); }
process.exitCode = failed ? 1 : 0;
