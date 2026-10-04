// Usage: node game_shot.mjs out.png [--series count,interval_ms (several frames: out_0.png, out_1.png, ...) --crop x,y,w,h,scale (zoom into a region) --w 960 --h 540 --opts "skipbus=1;bots=8" --cmd "tp 10 20|look 90 -10" --wait 1500 --quality high --keys "KeyW:800,Space:100" --after "cmd|cmd"]
// Boots dev.html, runs debug commands, optionally simulates key presses, then captures one frame.
import fs from 'node:fs';
import { serve, launch } from './browser.mjs';
import { encodePng, cropScale } from './png.mjs';

const args = process.argv.slice(2);
const out = args[0] || 'shot.png';
const opt = (n, d) => { const i = args.indexOf('--' + n); return i >= 0 ? args[i + 1] : d; };
const w = +opt('w', 960), h = +opt('h', 540);
const wait = +opt('wait', 1500);
const { srv, url } = await serve();
const { browser, page, logs } = await launch({ width: w, height: h });
const qs = new URLSearchParams({ w, h, quality: opt('quality', 'high'), opts: opt('opts', 'skipbus=1;bots=12;god=1'), cmd: opt('cmd', '') });
await page.goto(url + 'dev.html?' + qs);
try {
  await page.waitForFunction(() => window.__fn && (window.__fn.ready || window.__fn.error), null, { timeout: 180000 });
} catch (e) { console.log('timeout waiting for start'); }
const err = await page.evaluate(() => window.__fn && window.__fn.error);
if (err) { console.log('START ERROR:', err); console.log(logs.join('\n')); await browser.close(); srv.close(); process.exit(1); }
console.log('info:', await page.evaluate(() => window.__fn.info), '|', await page.evaluate(() => window.__fn.start));
await page.waitForTimeout(+opt('settle', 600));
if (opt('keys')) {
  for (const k of opt('keys').split(',')) {
    let [code, ms] = k.split(':');
    // "+Code:ms" presses and keeps the key held, "-Code:ms" releases it
    let mode = 'tap';
    if (code.startsWith('+')) { mode = 'down'; code = code.slice(1); }
    else if (code.startsWith('-')) { mode = 'up'; code = code.slice(1); }
    if (mode !== 'tap') {
      const isMouse = code.startsWith('Mouse');
      await page.evaluate(([c, down, isMouse]) => isMouse ? window.__fn.fn.mouse_button(c === 'Mouse0' ? 0 : 2, down) : window.__fn.fn.key(c, down), [code, mode === 'down', isMouse]);
      await page.waitForTimeout(+ms || 100);
    } else if (code === 'Mouse0' || code === 'Mouse2') {
      const b = code === 'Mouse0' ? 0 : 2;
      await page.evaluate((b) => window.__fn.fn.mouse_button(b, true), b);
      await page.waitForTimeout(+ms || 100);
      await page.evaluate((b) => window.__fn.fn.mouse_button(b, false), b);
    } else if (code === 'Wait') {
      await page.waitForTimeout(+ms || 100);
    } else {
      await page.evaluate((c) => window.__fn.fn.key(c, true), code);
      await page.waitForTimeout(+ms || 100);
      await page.evaluate((c) => window.__fn.fn.key(c, false), code);
    }
  }
}
if (opt('after')) for (const c of opt('after').split('|')) console.log('>', c, '=>', await page.evaluate((c) => window.__fn.fn.debug(c), c));
await page.waitForTimeout(wait);
const [seriesN, seriesMs] = opt('series') ? opt('series').split(',').map(Number) : [1, 0];
for (let shot = 0; shot < seriesN; shot++) {
if (shot > 0) await page.waitForTimeout(seriesMs);
const outFile = seriesN > 1 ? out.replace(/(\.png)?$/, `_${shot}.png`) : out;
const data = await page.evaluate(async () => {
  const c = await window.__fn.capture();
  let s = ''; const CH = 0x8000;
  for (let i = 0; i < c.rgba.length; i += CH) s += String.fromCharCode.apply(null, c.rgba.subarray(i, i + CH));
  return { w: c.w, h: c.h, b64: btoa(s) };
});
const rgba = Buffer.from(data.b64, 'base64');
let img = { w: data.w, h: data.h, rgba: new Uint8Array(rgba.buffer, rgba.byteOffset, rgba.length) };
if (opt('crop')) {
  const [x, y, cw, ch, sc] = opt('crop').split(',').map(Number);
  img = cropScale(data.w, img.rgba, x, y, cw, ch, sc || 1);
}
fs.writeFileSync(outFile, encodePng(img.w, img.h, img.rgba));
console.log('wrote', outFile, img.w + 'x' + img.h, '| frames', await page.evaluate(() => window.__fn.frames), '|', await page.evaluate(() => window.__fn.fn.debug('timings')), '|', await page.evaluate(() => window.__fn.fn.debug('state')));
if (opt('hud') ) console.log('hud:', await page.evaluate(() => window.__fn.fn.hud()));
}
const bad = logs.filter(l => /error|warn|panick/i.test(l) && !/WebGPU is experimental|favicon|404/.test(l));
if (bad.length) console.log('LOGS:\n' + bad.join('\n'));
await browser.close(); srv.close();
