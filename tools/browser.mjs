// Shared helpers: static file server + headless Chromium configured for WebGPU.
// In CI / sandboxes without a GPU this uses SwiftShader (software Vulkan).
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright-core';

const here = path.dirname(fileURLToPath(import.meta.url));
export const distDir = path.resolve(here, '..', 'dist');

const MIME = {
  '.html': 'text/html; charset=utf-8', '.js': 'text/javascript', '.mjs': 'text/javascript',
  '.wasm': 'application/wasm', '.css': 'text/css', '.json': 'application/json',
  '.woff2': 'font/woff2', '.png': 'image/png', '.svg': 'image/svg+xml',
};

export function serve(root = distDir, port = 0) {
  return new Promise((resolve) => {
    const srv = http.createServer((req, res) => {
      let p = decodeURIComponent(new URL(req.url, 'http://x').pathname);
      if (p.endsWith('/')) p += 'index.html';
      const file = path.join(root, p);
      if (!file.startsWith(root) || !fs.existsSync(file) || fs.statSync(file).isDirectory()) {
        res.statusCode = 404; res.end('not found'); return;
      }
      res.setHeader('content-type', MIME[path.extname(file)] || 'application/octet-stream');
      res.setHeader('cache-control', 'no-store');
      fs.createReadStream(file).pipe(res);
    }).listen(port, '127.0.0.1', () => resolve({ srv, url: `http://127.0.0.1:${srv.address().port}/` }));
  });
}

export function findChrome() {
  if (process.env.CHROME_PATH) return process.env.CHROME_PATH;
  const base = process.env.PLAYWRIGHT_BROWSERS_PATH || '/opt/pw-browsers';
  if (fs.existsSync(base)) {
    for (const d of fs.readdirSync(base).filter((d) => d.startsWith('chromium-')).sort().reverse()) {
      const c = path.join(base, d, 'chrome-linux', 'chrome');
      if (fs.existsSync(c)) return c;
    }
  }
  return undefined; // let playwright find its own
}

const COMMON = ['--no-sandbox', '--ignore-gpu-blocklist', ...(process.env.FN_CHROME_LOG ? ['--enable-logging=stderr', '--v=1'] : []), '--autoplay-policy=no-user-gesture-required'];
function flagSet(name) {
  const sets = {
    // software Vulkan (SwiftShader) for WebGPU, ANGLE/SwiftShader for compositing
    // SkiaGraphite + SwiftShader Vulkan is what makes the WebGPU canvas swapchain work headlessly
    default: ['--enable-unsafe-webgpu', '--enable-features=Vulkan,SkiaGraphite', '--use-vulkan=swiftshader', '--use-angle=swiftshader', '--use-webgpu-adapter=swiftshader', '--enable-webgpu-developer-features'],
    plain: ['--enable-unsafe-webgpu', '--use-angle=swiftshader', '--use-webgpu-adapter=swiftshader', '--enable-webgpu-developer-features'],
    vk: ['--enable-unsafe-webgpu', '--enable-features=Vulkan', '--use-vulkan=swiftshader', '--use-angle=vulkan', '--disable-vulkan-surface', '--use-webgpu-adapter=swiftshader', '--enable-webgpu-developer-features'],
    gl: ['--enable-unsafe-webgpu', '--use-gl=angle', '--use-angle=swiftshader-webgl', '--use-webgpu-adapter=swiftshader', '--enable-webgpu-developer-features'],
    nogpu: ['--enable-unsafe-webgpu', '--use-webgpu-adapter=swiftshader', '--enable-webgpu-developer-features', '--disable-gpu-compositing'],
  };
  return COMMON.concat(sets[name] || sets.default);
}

export async function launch({ width = 1280, height = 720, headless = true, dpr = +(process.env.FN_DPR || 1) } = {}) {
  const browser = await chromium.launch({
    executablePath: findChrome(),
    headless,
    args: flagSet(process.env.FN_FLAGS || 'default'),
  });
  const context = await browser.newContext({ viewport: { width, height }, deviceScaleFactor: dpr });
  const page = await context.newPage();
  const logs = [];
  page.on('console', (m) => { logs.push(`[${m.type()}] ${m.text()}`); });
  page.on('pageerror', (e) => { logs.push(`[pageerror] ${e.message}`); });
  return { browser, context, page, logs };
}
