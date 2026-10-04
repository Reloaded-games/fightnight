// Usage: node ui_shot.mjs out.png [--w 1280 --h 720 --opts "..." --cmd "tp ..|..." --keys "KeyW:300" --wait 1500 --state menu|playing --click "#btn-play" --after "js"]
// Boots index.html (the real UI). The WebGPU frame is read back and frozen into a 2D canvas so the
// regular screenshot includes menus, the HUD canvas and DOM overlays.
import fs from 'node:fs';
import { serve, launch } from './browser.mjs';

const args = process.argv.slice(2);
const out = args[0] || 'ui.png';
const opt = (n, d) => { const i = args.indexOf('--' + n); return i >= 0 ? args[i + 1] : d; };
const w = +opt('w', 1280), h = +opt('h', 720);
const { srv, url } = await serve();
const { browser, page, logs } = await launch({ width: w, height: h });
const want = opt('state', 'menu');
const qs = new URLSearchParams({ nolock: '1', quality: opt('quality', 'high') });
if (want === 'playing') { qs.set('autostart', '1'); qs.set('opts', opt('opts', 'skipbus=1;bots=12;god=1')); }
await page.goto(url + 'index.html?' + qs);
try {
  await page.waitForFunction((s) => window.__game && window.__game.state === s, want, { timeout: 240000 });
} catch (e) { console.log('timeout waiting for state', want, await page.evaluate(() => window.__game && window.__game.state)); }
await page.waitForTimeout(+opt('settle', 800));
for (const c of (opt('cmd', '')).split('|').filter(Boolean)) console.log('>', c, '=>', await page.evaluate((c) => window.__game.fn.debug(c), c));
if (opt('keys')) {
  for (const k of opt('keys').split(',')) {
    let [code, ms] = k.split(':');
    let mode = 'tap';
    if (code.startsWith('+')) { mode = 'down'; code = code.slice(1); } else if (code.startsWith('-')) { mode = 'up'; code = code.slice(1); }
    const isMouse = code.startsWith('Mouse');
    if (mode === 'tap') {
      if (isMouse) { await page.mouse.down({ button: code === 'Mouse0' ? 'left' : 'right' }); await page.waitForTimeout(+ms || 100); await page.mouse.up({ button: code === 'Mouse0' ? 'left' : 'right' }); }
      else { await page.keyboard.down(code); await page.waitForTimeout(+ms || 100); await page.keyboard.up(code); }
    } else if (isMouse) { await (mode === 'down' ? page.mouse.down({ button: code === 'Mouse0' ? 'left' : 'right' }) : page.mouse.up({ button: code === 'Mouse0' ? 'left' : 'right' })); await page.waitForTimeout(+ms || 100); }
    else { await (mode === 'down' ? page.keyboard.down(code) : page.keyboard.up(code)); await page.waitForTimeout(+ms || 100); }
  }
}
if (opt('click')) { await page.click(opt('click')); await page.waitForTimeout(400); }
if (opt('eval')) console.log('eval =>', await page.evaluate(opt('eval')));
await page.waitForTimeout(+opt('wait', 1200));
if (opt('freeze', '1') === '1') console.log('frozen:', await page.evaluate(() => window.__game.freezeFrame()));
await page.screenshot({ path: out });
console.log('wrote', out, '| state', await page.evaluate(() => window.__game.state));
if (opt('hud')) console.log('hud:', await page.evaluate(() => JSON.stringify(window.__game.hud)));
const bad = logs.filter(l => /error|warn|panick/i.test(l) && !/WebGPU is experimental|favicon|404|Failed to load resource/.test(l));
if (bad.length) console.log('LOGS:\n' + bad.join('\n'));
await browser.close(); srv.close();
