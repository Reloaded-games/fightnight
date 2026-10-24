// The recreated Fortnite maps in a real browser: the menu choice, a match on each map, its places and the lettered grid.
import { serve, launch } from './browser.mjs';
const { srv, url } = await serve();
const { browser, page, logs } = await launch({ width: 960, height: 540 });
const shots = process.env.SHOTS || '';
let failed = 0;
const check = (name, ok, extra = '') => { console.log(`${ok ? 'PASS' : 'FAIL'}  ${name} ${extra}`); if (!ok) failed++; };
const dbg = (c) => page.evaluate((c) => window.__game.fn.debug(c), c);
const state = async () => JSON.parse(await dbg('state'));
const info = async () => JSON.parse(await page.evaluate(() => window.__game.fn.map_info()));
const pois = async () => (await page.evaluate(() => window.__game.fn.poi_list())).split(';').filter(Boolean).map((s) => { const p = s.split(','); return { name: p[0], x: +p[1], z: +p[2], kind: p[3] }; });
async function waitTime(seconds) {
  const t = (await state()).t + seconds;
  await page.waitForFunction(t => JSON.parse(window.__game.fn.debug('state')).t >= t, t, { timeout: 120000 });
}
// a picture for a person to look at (SHOTS=dir); headless Chromium cannot read back the WebGPU canvas itself, so
// the frame is frozen into an image first, and a picture that cannot be taken never fails the checks
async function shot(name) {
  if (!shots) return;
  try {
    await page.evaluate(() => window.__game.freezeFrame());
    await page.screenshot({ path: `${shots}/${name}.png`, timeout: 120000 });
  } catch (e) { console.log(`(no picture ${name}: ${e.message})`); }
  finally { await page.evaluate(() => window.__game.unfreeze()); }
}
async function start(map) {
  await page.goto(url + 'index.html?nolock=1&autostart=1&quality=low&opts=' + encodeURIComponent(`skipbus=1;bots=3;god=1;map=${map}`));
  await page.waitForFunction(() => window.__game?.state === 'playing', null, { timeout: 240000 });
  await waitTime(0.3);
}
// the grid cell a world position lies in, as the in-game map labels it (A-J west to east, 1-10 north to south)
function cell(x, z, grid) {
  const col = Math.floor(x / grid + 5), row = Math.floor(z / grid + 5);
  return 'ABCDEFGHIJ'[col] + (row + 1);
}
const expected = {
  chapter1: { count: 20, cells: { 'Tilted Towers': ['D5', 'D6'], 'Pleasant Park': ['C3', 'C4'], 'Retail Row': ['H6'], 'Junk Junction': ['B1', 'B2'], 'Lonely Lodge': ['J4', 'J5', 'I4', 'I5'], 'Lucky Landing': ['F10'], 'Moisty Mire': ['I8', 'I9', 'J8'] } },
  chapter2: { count: 13, cells: { 'Sweaty Sands': ['B3', 'B4', 'C3', 'C4'], 'Steamy Stacks': ['G2', 'H2'], 'Salty Springs': ['D4'], 'Slurpy Swamp': ['C6', 'C7'], 'Lazy Lake': ['F6'], 'Retail Row': ['G6', 'H6'], 'Dirty Docks': ['H4'] } },
};
try {
  await page.goto(url + 'index.html?nolock=1&quality=low');
  await page.waitForFunction(() => window.__game?.state === 'menu', null, { timeout: 240000 });
  const choices = await page.locator('#seg-map button').allTextContents();
  check('menu offers Chapter 1, Chapter 2 and the original island', choices.join(',') === 'Chapter 1,Chapter 2,Original', choices.join(', '));
  check('Chapter 1 is chosen on a first visit', await page.getAttribute('#seg-map button[data-v="chapter1"]', 'aria-pressed') === 'true');
  check('the menu island behind the buttons is Chapter 1', (await info()).key === 'chapter1');
  await page.click('#seg-map button[data-v="chapter2"]');
  await page.reload();
  await page.waitForFunction(() => window.__game?.state === 'menu', null, { timeout: 240000 });
  const saved = await page.evaluate(() => ({ map: window.__game.cfg.map, selected: document.querySelector('#seg-map button[aria-pressed="true"]')?.dataset.v }));
  check('the map choice persists after reloading', saved.map === 'chapter2' && saved.selected === 'chapter2', JSON.stringify(saved));
  await page.click('#btn-multi'); await page.click('#btn-mp-host');
  check('the host lobby shows the same map choice', await page.evaluate(() => document.querySelector('#lb-map button.on')?.dataset.v) === 'chapter2');
  await page.click('#btn-lobby-leave'); await page.click('#btn-mp-back');

  for (const map of ['chapter1', 'chapter2']) {
    await start(map);
    const st = await state(), mi = await info(), list = await pois();
    check(`${map}: the match is played on it`, st.map === map && mi.key === map, `${st.map} ${JSON.stringify(mi)}`);
    check(`${map}: its map has a lettered grid`, mi.grid > 150 && mi.grid < 200, String(mi.grid));
    check(`${map}: every named location is there`, list.length === expected[map].count, list.map(p => p.name).join(', '));
    for (const [name, cells] of Object.entries(expected[map].cells)) {
      const p = list.find(p => p.name === name);
      const at = p ? cell(p.x, p.z, mi.grid) : 'missing';
      check(`${map}: ${name} is in ${cells.join('/')}`, cells.includes(at), at);
    }
    // stand in the first place on the list and look at it, then open the full map
    const first = list[0];
    await dbg(`tp ${first.x + 30} ${first.z + 30}`); await waitTime(0.5);
    await shot(`${map}_ground`);
    await page.keyboard.press('KeyM'); await waitTime(0.3);
    check(`${map}: M opens the full map`, await page.evaluate(() => document.getElementById('map').classList.contains('active')));
    await shot(`${map}_map`);
    await page.keyboard.press('KeyM');
  }
  const errors = logs.filter(s => /error|validation|panic|uncaught/i.test(s));
  check('no WebGPU validation or runtime errors on the recreated maps', errors.length === 0, errors.slice(0, 2).join('\n'));
} catch (e) { check('the maps complete their browser scenario', false, e.stack || String(e)); }
finally { await browser.close(); srv.close(); }
process.exitCode = failed ? 1 : 0;
