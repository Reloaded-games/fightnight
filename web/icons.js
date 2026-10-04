// Vector icons for the HUD and inventory, drawn on a canvas inside the box (x, y, w, h).
// Everything is polygons in a 100 x 50 design space so nothing needs to be loaded.

export const RARITY = ['#b4b8c2', '#5bd13a', '#3d9eff', '#b65dff', '#ffae1a'];
export const RARITY_NAMES = ['Common', 'Uncommon', 'Rare', 'Epic', 'Legendary'];
export const RARITY_DARK = ['#3b3f4a', '#1d5b13', '#123f8f', '#4b1a85', '#8a5a00'];
export const WEAPON_NAMES = ['Pistol', 'Submachine Gun', 'Assault Rifle', 'Pump Shotgun', 'Bolt-Action Sniper', 'Rocket Launcher'];
export const CONSUMABLE_NAMES = ['Bandage', 'Medkit', 'Mini Shield', 'Shield Potion', 'Chug Jug'];
export const AMMO_NAMES = ['Light', 'Medium', 'Heavy', 'Shells', 'Rockets'];
export const AMMO_COLORS = ['#f2bf40', '#66bf59', '#5980f2', '#e65940', '#f28c26'];
export const MAT_NAMES = ['Wood', 'Stone', 'Metal'];
export const MAT_COLORS = ['#d9a05a', '#c9a08a', '#9db9d9'];
export const PIECE_NAMES = ['Wall', 'Floor', 'Ramp', 'Roof'];

function poly(ctx, pts, ox, oy, sx, sy) {
  ctx.beginPath();
  for (let i = 0; i < pts.length; i += 2) {
    const x = ox + pts[i] * sx, y = oy + pts[i + 1] * sy;
    if (i === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y);
  }
  ctx.closePath();
}

function rect(ctx, x, y, w, h, ox, oy, sx, sy) {
  ctx.fillRect(ox + x * sx, oy + y * sy, w * sx, h * sy);
}

// Each weapon is a list of [kind, ...args]: 'r' rect (x,y,w,h), 'p' polygon (flat points), shade = 0..1 darkness
const WEAPONS = [
  // pistol
  [['r', 30, 14, 42, 11, 0], ['r', 72, 16, 8, 6, 0.4], ['p', [30, 25, 46, 25, 43, 46, 30, 46], 0.2], ['r', 34, 25, 10, 4, 0.5]],
  // smg
  [['r', 18, 17, 54, 12, 0], ['r', 72, 20, 18, 5, 0.4], ['r', 38, 29, 8, 19, 0.3], ['p', [26, 29, 35, 29, 33, 46, 25, 46], 0.2], ['r', 4, 20, 14, 7, 0.3], ['r', 38, 11, 14, 6, 0.5]],
  // assault rifle
  [['r', 22, 17, 48, 12, 0], ['r', 70, 20, 26, 5, 0.4], ['r', 52, 15, 20, 15, 0.25], ['r', 2, 19, 20, 11, 0.3], ['p', [38, 29, 47, 29, 49, 47, 41, 47], 0.3], ['p', [26, 29, 34, 29, 32, 44, 25, 44], 0.2], ['r', 28, 9, 18, 7, 0.5]],
  // shotgun
  [['r', 28, 17, 66, 5, 0.1], ['r', 30, 23, 54, 5, 0.4], ['r', 48, 21, 18, 10, 0.2], ['r', 2, 19, 30, 12, 0.3], ['r', 26, 17, 16, 14, 0], ['p', [32, 31, 40, 31, 37, 45, 30, 45], 0.2]],
  // sniper
  [['r', 28, 22, 68, 4, 0.2], ['r', 32, 9, 32, 9, 0.5], ['r', 12, 20, 32, 11, 0], ['r', 0, 20, 16, 13, 0.3], ['r', 30, 31, 6, 9, 0.3], ['r', 38, 18, 4, 4, 0.5]],
  // rocket launcher
  [['r', 8, 16, 76, 15, 0.1], ['p', [84, 17, 96, 24, 84, 30], 0.0], ['r', 4, 14, 8, 19, 0.5], ['p', [38, 31, 46, 31, 44, 46, 36, 46], 0.2], ['r', 22, 8, 12, 8, 0.5]],
];

export function drawWeapon(ctx, kind, x, y, w, h, color = '#fff') {
  const parts = WEAPONS[kind] || WEAPONS[0];
  const sx = w / 100, sy = h / 50;
  ctx.save();
  for (const p of parts) {
    ctx.fillStyle = shade(color, p[p.length - 1]);
    if (p[0] === 'r') rect(ctx, p[1], p[2], p[3], p[4], x, y, sx, sy);
    else { poly(ctx, p[1], x, y, sx, sy); ctx.fill(); }
  }
  ctx.restore();
}

function shade(hex, k) {
  const c = hex.startsWith('#') ? hex.slice(1) : hex;
  const n = parseInt(c.length === 3 ? c.split('').map((d) => d + d).join('') : c, 16);
  const r = (n >> 16) & 255, g = (n >> 8) & 255, b = n & 255;
  const f = 1 - k;
  return `rgb(${Math.round(r * f)},${Math.round(g * f)},${Math.round(b * f)})`;
}

export function drawPickaxe(ctx, x, y, w, h, color = '#fff') {
  const sx = w / 100, sy = h / 50;
  ctx.save();
  ctx.strokeStyle = shade(color, 0.35);
  ctx.lineWidth = 5 * Math.min(sx, sy) * 1.4;
  ctx.lineCap = 'round';
  ctx.beginPath(); ctx.moveTo(x + 22 * sx, y + 46 * sy); ctx.lineTo(x + 62 * sx, y + 8 * sy); ctx.stroke();
  ctx.fillStyle = color;
  poly(ctx, [40, 6, 62, 2, 84, 14, 78, 17, 62, 11, 46, 14], x, y, sx, sy); ctx.fill();
  poly(ctx, [62, 8, 70, 16, 66, 20, 58, 12], x, y, sx, sy); ctx.fillStyle = shade(color, 0.3); ctx.fill();
  ctx.restore();
}

export function drawConsumable(ctx, kind, x, y, w, h) {
  const cx = x + w / 2, cy = y + h / 2, s = Math.min(w, h);
  ctx.save();
  if (kind === 0) { // bandage roll
    ctx.fillStyle = '#f3f1e8'; ctx.beginPath(); ctx.ellipse(cx, cy + s * 0.04, s * 0.34, s * 0.3, 0, 0, Math.PI * 2); ctx.fill();
    ctx.fillStyle = '#d63a3a'; ctx.fillRect(cx - s * 0.05, cy - s * 0.26, s * 0.1, s * 0.6);
    ctx.fillStyle = '#c9c7bd'; ctx.beginPath(); ctx.ellipse(cx, cy + s * 0.04, s * 0.12, s * 0.1, 0, 0, Math.PI * 2); ctx.fill();
  } else if (kind === 1) { // medkit
    ctx.fillStyle = '#f6f6f4'; ctx.fillRect(cx - s * 0.38, cy - s * 0.24, s * 0.76, s * 0.55);
    ctx.fillStyle = '#7e8590'; ctx.fillRect(cx - s * 0.16, cy - s * 0.34, s * 0.32, s * 0.1);
    ctx.fillStyle = '#e2342e'; ctx.fillRect(cx - s * 0.06, cy - s * 0.17, s * 0.12, s * 0.42); ctx.fillRect(cx - s * 0.21, cy - s * 0.02, s * 0.42, s * 0.12);
  } else if (kind === 2 || kind === 3) { // shield potions
    const r = kind === 2 ? s * 0.24 : s * 0.3;
    const g = ctx.createRadialGradient(cx - r * 0.3, cy + s * 0.08 - r * 0.3, 2, cx, cy + s * 0.08, r * 1.2);
    g.addColorStop(0, '#9fe0ff'); g.addColorStop(1, kind === 2 ? '#2a8bff' : '#1e5fe8');
    ctx.fillStyle = g; ctx.beginPath(); ctx.arc(cx, cy + s * 0.1, r, 0, Math.PI * 2); ctx.fill();
    ctx.fillStyle = '#c9ecff'; ctx.fillRect(cx - r * 0.3, cy + s * 0.1 - r * 1.5, r * 0.6, r * 0.8);
    ctx.fillStyle = '#b07a43'; ctx.fillRect(cx - r * 0.34, cy + s * 0.1 - r * 1.75, r * 0.68, r * 0.3);
  } else { // chug jug
    const g = ctx.createLinearGradient(cx, cy - s * 0.3, cx, cy + s * 0.4);
    g.addColorStop(0, '#b27dff'); g.addColorStop(1, '#6a2fe0');
    ctx.fillStyle = g; ctx.beginPath(); ctx.ellipse(cx, cy + s * 0.08, s * 0.28, s * 0.3, 0, 0, Math.PI * 2); ctx.fill();
    ctx.fillStyle = '#e9dcff'; ctx.fillRect(cx - s * 0.09, cy - s * 0.34, s * 0.18, s * 0.2);
    ctx.fillStyle = '#ffd93d'; ctx.fillRect(cx - s * 0.13, cy - s * 0.42, s * 0.26, s * 0.1);
    ctx.strokeStyle = '#e9e9f2'; ctx.lineWidth = s * 0.05; ctx.beginPath(); ctx.arc(cx + s * 0.3, cy + s * 0.02, s * 0.12, -Math.PI / 2, Math.PI / 2); ctx.stroke();
  }
  ctx.restore();
}

export function drawAmmo(ctx, kind, x, y, w, h) {
  const c = AMMO_COLORS[kind] || '#fff';
  const s = Math.min(w, h);
  ctx.save();
  ctx.fillStyle = c;
  const n = kind === 3 ? 2 : kind === 4 ? 1 : 3;
  for (let i = 0; i < n; i++) {
    const bx = x + w / 2 + (i - (n - 1) / 2) * s * 0.3;
    if (kind === 4) { ctx.fillRect(bx - s * 0.13, y + h * 0.18, s * 0.26, h * 0.6); ctx.beginPath(); ctx.moveTo(bx - s * 0.13, y + h * 0.18); ctx.lineTo(bx, y + h * 0.02); ctx.lineTo(bx + s * 0.13, y + h * 0.18); ctx.fill(); }
    else if (kind === 3) { ctx.fillRect(bx - s * 0.12, y + h * 0.3, s * 0.24, h * 0.5); ctx.fillStyle = '#d9d9d9'; ctx.fillRect(bx - s * 0.12, y + h * 0.65, s * 0.24, h * 0.15); ctx.fillStyle = c; }
    else { ctx.fillRect(bx - s * 0.08, y + h * 0.36, s * 0.16, h * 0.46); ctx.beginPath(); ctx.moveTo(bx - s * 0.08, y + h * 0.36); ctx.lineTo(bx, y + h * 0.1); ctx.lineTo(bx + s * 0.08, y + h * 0.36); ctx.fill(); }
  }
  ctx.restore();
}

export function drawMaterial(ctx, kind, x, y, w, h) {
  const cx = x + w / 2, cy = y + h / 2, s = Math.min(w, h);
  ctx.save();
  if (kind === 0) { // logs
    for (const [dx, dy] of [[-0.18, 0.12], [0.18, 0.12], [0, -0.14]]) {
      ctx.fillStyle = '#a8703a'; ctx.beginPath(); ctx.ellipse(cx + dx * s, cy + dy * s, s * 0.2, s * 0.2, 0, 0, Math.PI * 2); ctx.fill();
      ctx.fillStyle = '#e4b97b'; ctx.beginPath(); ctx.ellipse(cx + dx * s, cy + dy * s, s * 0.12, s * 0.12, 0, 0, Math.PI * 2); ctx.fill();
    }
  } else if (kind === 1) { // bricks
    ctx.fillStyle = '#c4745a';
    for (let r = 0; r < 3; r++) for (let c = 0; c < 2; c++) ctx.fillRect(cx - s * 0.36 + c * s * 0.37 + (r % 2) * s * 0.18, cy - s * 0.3 + r * s * 0.22, s * 0.34, s * 0.19);
  } else { // metal ingot / gear
    ctx.fillStyle = '#b4cce8';
    ctx.beginPath(); ctx.moveTo(cx - s * 0.36, cy + s * 0.2); ctx.lineTo(cx - s * 0.2, cy - s * 0.2); ctx.lineTo(cx + s * 0.2, cy - s * 0.2); ctx.lineTo(cx + s * 0.36, cy + s * 0.2); ctx.closePath(); ctx.fill();
    ctx.fillStyle = '#7f98b8'; ctx.fillRect(cx - s * 0.36, cy + s * 0.2, s * 0.72, s * 0.1);
  }
  ctx.restore();
}

export function drawPiece(ctx, kind, x, y, w, h, color = '#fff') {
  const cx = x + w / 2, cy = y + h / 2, s = Math.min(w, h);
  ctx.save();
  ctx.fillStyle = color; ctx.strokeStyle = color; ctx.lineWidth = Math.max(2, s * 0.07);
  if (kind === 0) { ctx.strokeRect(cx - s * 0.32, cy - s * 0.36, s * 0.64, s * 0.72); ctx.fillRect(cx - s * 0.32, cy - s * 0.36, s * 0.64, s * 0.14); ctx.fillRect(cx - s * 0.04, cy - s * 0.3, s * 0.08, s * 0.64); }
  else if (kind === 1) { ctx.beginPath(); ctx.moveTo(cx, cy - s * 0.26); ctx.lineTo(cx + s * 0.42, cy); ctx.lineTo(cx, cy + s * 0.26); ctx.lineTo(cx - s * 0.42, cy); ctx.closePath(); ctx.stroke(); ctx.globalAlpha = 0.35; ctx.fill(); }
  else if (kind === 2) { ctx.beginPath(); ctx.moveTo(cx - s * 0.4, cy + s * 0.32); ctx.lineTo(cx + s * 0.4, cy + s * 0.32); ctx.lineTo(cx + s * 0.4, cy - s * 0.32); ctx.closePath(); ctx.stroke(); ctx.globalAlpha = 0.35; ctx.fill(); }
  else { ctx.beginPath(); ctx.moveTo(cx - s * 0.42, cy + s * 0.22); ctx.lineTo(cx, cy - s * 0.34); ctx.lineTo(cx + s * 0.42, cy + s * 0.22); ctx.closePath(); ctx.stroke(); ctx.globalAlpha = 0.35; ctx.fill(); }
  ctx.restore();
}

export function drawSkull(ctx, x, y, s, color = '#fff') {
  ctx.save();
  ctx.fillStyle = color;
  ctx.beginPath(); ctx.arc(x, y - s * 0.08, s * 0.38, Math.PI, 0); ctx.lineTo(x + s * 0.3, y + s * 0.22); ctx.lineTo(x - s * 0.3, y + s * 0.22); ctx.closePath(); ctx.fill();
  ctx.fillRect(x - s * 0.2, y + s * 0.2, s * 0.4, s * 0.2);
  ctx.fillStyle = '#0b1030';
  ctx.beginPath(); ctx.arc(x - s * 0.15, y - s * 0.05, s * 0.1, 0, Math.PI * 2); ctx.arc(x + s * 0.15, y - s * 0.05, s * 0.1, 0, Math.PI * 2); ctx.fill();
  ctx.restore();
}

export function drawPerson(ctx, x, y, s, color = '#fff') {
  ctx.save();
  ctx.fillStyle = color;
  ctx.beginPath(); ctx.arc(x, y - s * 0.22, s * 0.17, 0, Math.PI * 2); ctx.fill();
  ctx.beginPath(); ctx.moveTo(x - s * 0.3, y + s * 0.38); ctx.quadraticCurveTo(x - s * 0.3, y - s * 0.02, x, y - s * 0.02); ctx.quadraticCurveTo(x + s * 0.3, y - s * 0.02, x + s * 0.3, y + s * 0.38); ctx.closePath(); ctx.fill();
  ctx.restore();
}

export function drawStorm(ctx, x, y, s, color = '#fff') {
  ctx.save();
  ctx.strokeStyle = color; ctx.lineWidth = Math.max(2, s * 0.09); ctx.lineCap = 'round';
  for (let i = 0; i < 3; i++) {
    const r = s * (0.14 + i * 0.12);
    ctx.beginPath(); ctx.arc(x, y, r, i * 1.6, i * 1.6 + Math.PI * 1.3); ctx.stroke();
  }
  ctx.restore();
}

/** Draw whatever an inventory slot holds. */
export function drawItem(ctx, slot, x, y, w, h) {
  if (!slot) return;
  if (slot.t === 'p') drawPickaxe(ctx, x + w * 0.1, y + h * 0.1, w * 0.8, h * 0.8, '#d8dde8');
  else if (slot.t === 'w') drawWeapon(ctx, slot.k, x + w * 0.04, y + h * 0.12, w * 0.92, h * 0.76, '#eef1f8');
  else drawConsumable(ctx, slot.k, x, y, w, h);
}
