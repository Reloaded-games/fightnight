// Usage: node shot.mjs out.png [--w 960 --h 540 --cam x,y,z,yaw,pitch --seed 1234 --quality high --wait 1500]
import fs from 'node:fs';
import { serve, launch } from './browser.mjs';
import { encodePng } from './png.mjs';

const args = process.argv.slice(2);
const out = args[0] || 'shot.png';
const opt = (n, d) => { const i = args.indexOf('--' + n); return i >= 0 ? args[i + 1] : d; };
const w = +opt('w', 960), h = +opt('h', 540);
const wait = +opt('wait', 1500);
const { srv, url } = await serve();
const { browser, page, logs } = await launch({ width: w, height: h });
const qs = new URLSearchParams({ w, h, cam: opt('cam', '0,60,200,0,-0.2'), seed: opt('seed', '1234'), quality: opt('quality', 'high') });
if (opt('at')) qs.set('at', opt('at'));
await page.goto(url + '?' + qs);
try {
  await page.waitForFunction(() => window.__fn && (window.__fn.ready || window.__fn.error), null, { timeout: 120000 });
} catch (e) { console.log('timeout waiting for start'); }
const err = await page.evaluate(() => window.__fn && window.__fn.error);
if (err) { console.log('START ERROR:', err); console.log(logs.join('\n')); await browser.close(); srv.close(); process.exit(1); }
console.log('info:', await page.evaluate(() => window.__fn.info));
if (opt('poi')) {
  // --poi "Maple" --dist 50 --yaw 0.3 --eye 3 --pitch -0.1 --dx 0 --dz 0 : look at a point of interest from `dist` metres away
  const cam = await page.evaluate(({ name, dist, yaw, eye, pitch, dx, dz }) => {
    const pois = window.__fn.pois().split(';').map((p) => p.split(','));
    const q = pois.find((p) => p[0].toLowerCase().includes(name.toLowerCase()));
    if (!q) return null;
    const cx = +q[1] + dx, cz = +q[2] + dz;
    const fx = -Math.sin(yaw), fz = -Math.cos(yaw);
    const x = cx - fx * dist, z = cz - fz * dist;
    window.__fn.setCamera(x, window.__fn.ground(x, z) + eye, z, yaw, pitch);
    return [x, z];
  }, { name: opt('poi'), dist: +opt('dist', 50), yaw: +opt('yaw', 0), eye: +opt('eye', 3), pitch: +opt('pitch', -0.08), dx: +opt('dx', 0), dz: +opt('dz', 0) });
  console.log('camera at', cam);
}
await page.waitForTimeout(wait);
const t0 = Date.now();
// transfer in chunks to avoid huge argument lists
const data = await page.evaluate(async () => {
  const c = await window.__fn.capture();
  let s = ''; const CH = 0x8000;
  for (let i = 0; i < c.rgba.length; i += CH) s += String.fromCharCode.apply(null, c.rgba.subarray(i, i + CH));
  return { w: c.w, h: c.h, b64: btoa(s) };
});
const rgba = Buffer.from(data.b64, 'base64');
fs.writeFileSync(out, encodePng(data.w, data.h, new Uint8Array(rgba.buffer, rgba.byteOffset, rgba.length)));
console.log('wrote', out, data.w + 'x' + data.h, 'stats:', await page.evaluate(() => window.__fn.stats()), 'frames:', await page.evaluate(() => window.__fn.frames), (Date.now() - t0) + 'ms');
const bad = logs.filter(l => /error|warn/i.test(l) && !/WebGPU is experimental|favicon|404/.test(l));
if (bad.length) console.log('LOGS:\n' + bad.join('\n'));
await browser.close(); srv.close();
