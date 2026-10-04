// Hunts for the "black square" artefact: renders a loot-rich stretch of town from many camera positions and reports every
// patch of (near) pure black that is not just a dark corner. Usage: node black_squares.mjs [frames] [outdir]
import fs from 'node:fs';
import { serve, launch } from './browser.mjs';
import { encodePng } from './png.mjs';

const N = +(process.argv[2] || 24);
const outDir = process.argv[3] || '';
const { srv, url } = await serve();
const { browser, page, logs } = await launch({ width: 640, height: 360 });
await page.goto(url + 'dev.html?' + new URLSearchParams({ w: 640, h: 360, quality: 'low', opts: 'skipbus=1;bots=10;god=1', cmd: 'tp -10 40' }));
await page.waitForFunction(() => window.__fn && (window.__fn.ready || window.__fn.error), null, { timeout: 240000 });
const spots = [];
let found = 0;
for (let k = 0; k < N; k++) {
  const ang = (k / N) * Math.PI * 2 * 3;
  const r = 6 + (k % 5) * 7;
  const cx = -10 + Math.cos(ang) * r, cz = 40 + Math.sin(ang) * r;
  const yaw = ((ang * 180) / Math.PI + 180 + (k % 7) * 17) % 360;
  const cmd = `cam ${cx.toFixed(1)} ${(23.0 + (k % 4) * 1.2).toFixed(1)} ${cz.toFixed(1)} ${yaw.toFixed(0)} ${-6 - (k % 3) * 5} ${55 + (k % 3) * 8}`;
  await page.evaluate((c) => window.__fn.fn.debug(c), cmd);
  await page.waitForTimeout(700);
  const d = await page.evaluate(async () => {
    const c = await window.__fn.capture();
    let s = ''; const CH = 0x8000;
    for (let i = 0; i < c.rgba.length; i += CH) s += String.fromCharCode.apply(null, c.rgba.subarray(i, i + CH));
    return { w: c.w, h: c.h, b64: btoa(s) };
  });
  const rgba = new Uint8Array(Buffer.from(d.b64, 'base64'));
  const { w, h } = d;
  // near-black pixels, grouped into connected patches
  const dark = new Uint8Array(w * h);
  for (let i = 0; i < w * h; i++) dark[i] = rgba[i * 4] < 5 && rgba[i * 4 + 1] < 5 && rgba[i * 4 + 2] < 5 ? 1 : 0;
  const seen = new Uint8Array(w * h);
  const patches = [];
  for (let i = 0; i < w * h; i++) {
    if (!dark[i] || seen[i]) continue;
    let x0 = w, y0 = h, x1 = 0, y1 = 0, n = 0;
    const stack = [i]; seen[i] = 1;
    while (stack.length) {
      const p = stack.pop(); const x = p % w, y = (p / w) | 0; n++;
      if (x < x0) x0 = x; if (x > x1) x1 = x; if (y < y0) y0 = y; if (y > y1) y1 = y;
      for (const q of [p - 1, p + 1, p - w, p + w]) if (q >= 0 && q < w * h && dark[q] && !seen[q] && Math.abs((q % w) - x) <= 1) { seen[q] = 1; stack.push(q); }
    }
    if (n >= 6) patches.push({ x0, y0, x1, y1, n });
  }
  if (patches.length) {
    found += patches.length;
    console.log(`frame ${k} (${cmd}): ${patches.length} dark patch(es)`, patches.slice(0, 5).map((p) => `${p.x1 - p.x0 + 1}x${p.y1 - p.y0 + 1}@${p.x0},${p.y0} (${p.n}px)`).join('  '));
    if (outDir) { fs.mkdirSync(outDir, { recursive: true }); fs.writeFileSync(`${outDir}/frame_${k}.png`, encodePng(w, h, rgba)); }
  } else console.log(`frame ${k}: clean`);
}
console.log(found ? `\n${found} dark patches` : '\nno dark patches');
await browser.close(); srv.close();
