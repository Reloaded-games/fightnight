// The in-game HUD: drawn with Canvas2D every frame from the JSON snapshot the game produces.
import { RARITY, RARITY_DARK, AMMO_NAMES, MAT_COLORS, MAT_NAMES, PIECE_NAMES, drawItem, drawWeapon, drawAmmo, drawMaterial, drawPiece, drawSkull, drawPerson, drawStorm } from './icons.js';

const FONT = '"Burbank Big Condensed","Bebas Neue","Oswald","Impact","Haettenschweiler","Arial Narrow Bold","Arial Narrow",system-ui,sans-serif';
const WORLD = 1280;

function fmtTime(s) {
  s = Math.max(0, Math.ceil(s));
  return Math.floor(s / 60) + ':' + String(s % 60).padStart(2, '0');
}

export class Hud {
  constructor(canvas, mapImage) {
    this.c = canvas;
    this.ctx = canvas.getContext('2d');
    this.map = mapImage; // canvas holding the 1024x1024 island map
    this.W = 1; this.H = 1; this.dpr = 1;
    this.notices = []; // pickup notifications
    this.prev = null;
    this.matFlash = [0, 0, 0];
    this.slotFlash = [0, 0, 0, 0, 0, 0];
    this.hitT = 0;
    this.time = 0;
    this.fps = 60;
    this.showPerf = false;
    this.perf = '';
    this.spectateHint = 0;
    this.banners = []; // "ELIMINATED name" popups for the player's own kills
    this.seenFeed = new Set();
  }

  resize(w, h, dpr) {
    this.dpr = dpr; this.W = w; this.H = h;
    this.c.width = Math.floor(w * dpr); this.c.height = Math.floor(h * dpr);
  }

  clear() {
    this.ctx.setTransform(1, 0, 0, 1, 0, 0);
    this.ctx.clearRect(0, 0, this.c.width, this.c.height);
  }

  // ---- helpers ----------------------------------------------------------------------------------------
  text(str, x, y, size, color = '#fff', align = 'left', opts = {}) {
    const ctx = this.ctx;
    ctx.save();
    ctx.font = `${opts.weight || 900} ${opts.italic === false ? '' : 'italic '}${size}px ${FONT}`;
    ctx.textAlign = align; ctx.textBaseline = opts.base || 'alphabetic';
    if (opts.shadow !== false) { ctx.lineWidth = Math.max(2, size * 0.14); ctx.strokeStyle = opts.stroke || 'rgba(8,12,40,.85)'; ctx.lineJoin = 'round'; ctx.strokeText(str, x, y); }
    ctx.fillStyle = color; ctx.fillText(str, x, y);
    ctx.restore();
  }

  skew(x, y, w, h, fill, k = -0.3, stroke) {
    const ctx = this.ctx;
    ctx.save();
    ctx.transform(1, 0, k, 1, -k * (y + h / 2), 0);
    ctx.fillStyle = fill; ctx.fillRect(x, y, w, h);
    if (stroke) { ctx.lineWidth = 2; ctx.strokeStyle = stroke; ctx.strokeRect(x, y, w, h); }
    ctx.restore();
  }

  // ---- main entry ------------------------------------------------------------------------------------------
  draw(s, dt) {
    this.time += dt;
    const ctx = this.ctx;
    this.clear();
    if (!s) return;
    ctx.setTransform(this.dpr, 0, 0, this.dpr, 0, 0);
    const W = this.W, H = this.H;
    const S = Math.max(0.62, Math.min(1.5, H / 1080));
    this.S = S;
    this.track(s, dt);
    const dead = s.dead;

    this.damageIndicators(s, W, H, S);
    if (!dead || s.spec) {
      this.compass(s, W, S);
      this.minimap(s, W, S);
      this.stormInfo(s, W, S);
      this.feed(s, W, S);
      this.toasts(s, W, S);
    }
    if (!dead) {
      this.vitals(s, W, H, S);
      this.hotbar(s, W, H, S);
      this.ammo(s, W, H, S);
      this.crosshair(s, W, H, S);
      this.prompt(s, W, H, S);
      this.heal(s, W, H, S);
      this.busPrompts(s, W, H, S);
      this.notice(s, W, H, S);
      this.bannersDraw(s, W, H, S);
    } else {
      this.spectating(s, W, H, S);
    }
    this.damageNumbers(s, W, H, S);
    if (this.showPerf) this.text(this.perf, 12, H - 14, 15 * S, '#9dffb0', 'left', { italic: false, weight: 600 });
  }

  // Detect pickups / changes between frames for the notification list.
  track(s, dt) {
    const p = this.prev;
    if (p && !s.dead && p.ph === s.ph) {
      for (let i = 0; i < 6; i++) {
        const a = p.slots[i], b = s.slots[i];
        if (b && (!a || a.t !== b.t || a.k !== b.k || a.r !== b.r || (b.t === 'c' && b.c > a.c))) {
          this.notices.push({ txt: b.t === 'c' ? `${b.n} x${b.c}` : b.n, r: b.r ?? 0, age: 0 });
          this.slotFlash[i] = 1;
        }
      }
      for (let i = 0; i < 3; i++) if (s.mats[i] > p.mats[i]) { this.matFlash[i] = 1; this.notices.push({ txt: `+${s.mats[i] - p.mats[i]} ${MAT_NAMES[i]}`, r: -1, age: 0, mat: i }); }
      for (let i = 0; i < 5; i++) if (s.ammo[i] > p.ammo[i] + 0) this.notices.push({ txt: `+${s.ammo[i] - p.ammo[i]} ${AMMO_NAMES[i]} ammo`, r: -2, age: 0, ammo: i });
    }
    // the player's own eliminations get a big centre-screen banner
    for (const f of s.feed) {
      const key = f.v + '|' + f.k + '|' + Math.floor((s.t - f.age) * 2);
      if (f.me && !this.seenFeed.has(key) && f.age < 1.5) { this.seenFeed.add(key); this.banners.push({ name: f.v, age: 0, kills: s.kills }); }
    }
    if (this.seenFeed.size > 64) this.seenFeed.clear();
    for (const b of this.banners) b.age += dt;
    this.banners = this.banners.filter((b) => b.age < 2.8).slice(-2);
    this.prev = s;
    for (const n of this.notices) n.age += dt;
    this.notices = this.notices.filter((n) => n.age < 3.2).slice(-6);
    for (let i = 0; i < 3; i++) this.matFlash[i] = Math.max(0, this.matFlash[i] - dt * 2.5);
    for (let i = 0; i < 6; i++) this.slotFlash[i] = Math.max(0, this.slotFlash[i] - dt * 2.5);
    this.hitT = s.hit.t;
  }

  // ---- vitals ---------------------------------------------------------------------------------------------
  vitals(s, W, H, S) {
    const ctx = this.ctx;
    const bw = 330 * S, bh = 22 * S, x = 36 * S, y = H - 52 * S;
    const bar = (yy, val, max, col, col2, label) => {
      this.skew(x - 3, yy - 3, bw + 6, bh + 6, 'rgba(6,10,36,.8)');
      ctx.save();
      ctx.transform(1, 0, -0.3, 1, 0.3 * (yy + bh / 2), 0);
      const g = ctx.createLinearGradient(0, yy, 0, yy + bh);
      g.addColorStop(0, col2); g.addColorStop(1, col);
      ctx.fillStyle = g; ctx.fillRect(x, yy, bw * Math.max(0, Math.min(1, val / max)), bh);
      ctx.fillStyle = 'rgba(255,255,255,.22)'; ctx.fillRect(x, yy, bw * Math.max(0, Math.min(1, val / max)), bh * 0.35);
      ctx.fillStyle = 'rgba(6,10,36,.55)';
      for (let i = 1; i < 4; i++) ctx.fillRect(x + (bw * i) / 4 - 1, yy, 2, bh);
      ctx.restore();
      this.text(String(Math.ceil(val)), x + bw + 12 * S, yy + bh - 1 * S, 26 * S, '#fff');
    };
    const lowHp = s.hp < 30;
    bar(y - 30 * S, s.sh, 100, '#2f8cff', '#6fc4ff', 'shield');
    bar(y, s.hp, 100, lowHp ? '#ff4a4a' : '#5fd13a', lowHp ? '#ff9a8a' : '#a7f07a', 'health');
    // heart / shield glyphs
    this.text('+', x - 24 * S, y + bh - 2 * S, 28 * S, '#7dff5c');
    this.text('◆', x - 26 * S, y - 30 * S + bh - 1 * S, 22 * S, '#69bcff');
  }

  // ---- hotbar -------------------------------------------------------------------------------------------------
  hotbar(s, W, H, S) {
    const ctx = this.ctx;
    const sz = 68 * S, gap = 7 * S;
    const total = 6 * sz + 5 * gap;
    const x0 = W / 2 - total / 2 - 10 * S;
    const y0 = H - sz - 22 * S;
    for (let i = 0; i < 6; i++) {
      const sel = s.sel === i && !s.build.on;
      const slot = s.slots[i];
      const grow = sel ? 1.18 : 1 + this.slotFlash[i] * 0.12;
      const w = sz * grow, h = sz * grow;
      const x = x0 + i * (sz + gap) - (w - sz) / 2, y = y0 - (h - sz) + (sel ? 0 : 0);
      ctx.save();
      const rar = slot && slot.r != null ? slot.r : slot && slot.t === 'p' ? 0 : -1;
      const base = rar >= 0 ? RARITY[rar] : '#5a6490';
      // body
      const g = ctx.createLinearGradient(0, y, 0, y + h);
      g.addColorStop(0, slot ? (rar >= 0 ? this.alpha(base, 0.5) : 'rgba(60,70,120,.55)') : 'rgba(10,16,50,.55)');
      g.addColorStop(1, slot ? this.alpha(RARITY_DARK[Math.max(0, rar)], 0.9) : 'rgba(10,16,50,.75)');
      ctx.fillStyle = g; ctx.fillRect(x, y, w, h);
      if (slot) { ctx.fillStyle = base; ctx.fillRect(x, y + h - 5 * S, w, 5 * S); }
      ctx.lineWidth = sel ? 3.5 * S : 2 * S; ctx.strokeStyle = sel ? '#fff' : 'rgba(255,255,255,.35)'; ctx.strokeRect(x, y, w, h);
      if (sel) { ctx.shadowColor = base; ctx.shadowBlur = 18 * S; ctx.strokeRect(x, y, w, h); }
      ctx.restore();
      if (slot) drawItem(ctx, slot, x + 5 * S, y + 8 * S, w - 10 * S, h - 24 * S);
      this.text(String(i + 1), x + 6 * S, y + 17 * S, 17 * S, 'rgba(255,255,255,.85)');
      if (slot && slot.t === 'w') this.text(String(slot.a), x + w - 6 * S, y + 18 * S, 17 * S, slot.a === 0 ? '#ff7d7d' : '#fff', 'right');
      if (slot && slot.t === 'c') this.text(String(slot.c), x + w - 6 * S, y + 18 * S, 19 * S, '#fff', 'right');
    }
    // materials
    const mx = x0 + total + 26 * S;
    for (let i = 0; i < 3; i++) {
      const act = s.build.on && s.build.mat === i;
      const mw = 80 * S, mh = 21 * S, my = y0 + 1 * S + i * (mh + 2 * S);
      this.skew(mx, my, mw, mh, act ? 'rgba(255,255,255,.95)' : 'rgba(8,12,44,.62)', -0.25, act ? '#fff' : 'rgba(255,255,255,.25)');
      ctx.save(); ctx.globalAlpha = 1; drawMaterial(ctx, i, mx + 6 * S, my + 1 * S, 19 * S, 19 * S); ctx.restore();
      const flash = this.matFlash[i];
      this.text(String(s.mats[i]), mx + mw - 10 * S, my + mh - 4 * S, 17 * S + flash * 5, act ? '#10163d' : (flash > 0 ? '#fff29a' : '#fff'), 'right', { stroke: act ? 'rgba(255,255,255,0)' : undefined, shadow: !act });
    }
  }

  alpha(hex, a) {
    const n = parseInt(hex.slice(1), 16);
    return `rgba(${(n >> 16) & 255},${(n >> 8) & 255},${n & 255},${a})`;
  }

  // ---- ammo / building pieces ------------------------------------------------------------------------------------
  ammo(s, W, H, S) {
    const x = W - 40 * S, y = H - 60 * S;
    if (s.build.on) {
      // piece picker
      const keys = ['Z', 'X', 'C', 'V'];
      const sz = 70 * S, gap = 8 * S;
      const x0 = W - 40 * S - 4 * sz - 3 * gap;
      for (let i = 0; i < 4; i++) {
        const sel = s.build.piece === i;
        const px = x0 + i * (sz + gap), py = H - sz - 30 * S;
        const ctx = this.ctx;
        ctx.save();
        ctx.fillStyle = sel ? 'rgba(255,255,255,.95)' : 'rgba(8,12,44,.7)'; ctx.fillRect(px, py, sz, sz);
        ctx.strokeStyle = sel ? MAT_COLORS[s.build.mat] : 'rgba(255,255,255,.3)'; ctx.lineWidth = sel ? 4 * S : 2 * S; ctx.strokeRect(px, py, sz, sz);
        ctx.restore();
        drawPiece(ctx, i, px + 4 * S, py + 4 * S, sz - 8 * S, sz - 14 * S, sel ? '#10163d' : '#e8f0ff');
        this.text(keys[i], px + 6 * S, py + 18 * S, 16 * S, sel ? '#10163d' : '#fff', 'left', { shadow: !sel });
        this.text(PIECE_NAMES[i], px + sz / 2, py + sz - 5 * S, 14 * S, sel ? '#10163d' : '#cfe0ff', 'center', { shadow: !sel });
      }
      const enough = s.mats[s.build.mat] >= s.build.cost;
      this.text(enough ? `${MAT_NAMES[s.build.mat]} ${s.build.cost}` : `Not enough ${MAT_NAMES[s.build.mat]}`, x, H - sz - 40 * S, 22 * S, enough ? MAT_COLORS[s.build.mat] : '#ff7d7d', 'right');
      return;
    }
    const w = s.wep;
    if (!w) return;
    const ctx = this.ctx;
    // weapon name + rarity underline
    this.text(w.n, x, y - 54 * S, 22 * S, '#fff', 'right');
    ctx.fillStyle = RARITY[w.r]; ctx.fillRect(x - 200 * S, y - 48 * S, 200 * S, 4 * S);
    drawWeapon(ctx, w.k, x - 330 * S, y - 66 * S, 120 * S, 60 * S, RARITY[w.r]);
    // ammo counts
    const mag = String(w.mag);
    this.text(mag, x - 112 * S, y + 14 * S, 74 * S, w.mag === 0 ? '#ff7d7d' : '#fff', 'right');
    this.text('| ' + w.res, x - 100 * S, y + 14 * S, 36 * S, 'rgba(255,255,255,.85)', 'left');
    drawAmmo(ctx, s.slots[s.sel] ? s.slots[s.sel].ak : 0, x - 38 * S, y - 28 * S, 36 * S, 36 * S);
  }

  // ---- crosshair ------------------------------------------------------------------------------------------------------
  crosshair(s, W, H, S) {
    const ctx = this.ctx;
    const cx = W / 2, cy = H / 2;
    const w = s.wep;
    if (s.build.on || s.mode === 'bus' || s.mode === 'freefall' || s.mode === 'glide') {
      if (s.mode === 'glide' || s.mode === 'freefall') return;
    }
    // sniper scope overlay
    if (w && w.scope && s.ads && s.aim > 0.6) {
      const r = Math.min(W, H) * 0.46;
      ctx.save();
      ctx.fillStyle = `rgba(0,0,0,${Math.min(1, (s.aim - 0.6) * 2.5)})`;
      ctx.beginPath(); ctx.rect(0, 0, W, H); ctx.arc(cx, cy, r, 0, Math.PI * 2, true); ctx.fill('evenodd');
      ctx.strokeStyle = 'rgba(0,0,0,.95)'; ctx.lineWidth = 3;
      ctx.beginPath(); ctx.moveTo(cx - r, cy); ctx.lineTo(cx + r, cy); ctx.moveTo(cx, cy - r); ctx.lineTo(cx, cy + r); ctx.stroke();
      ctx.strokeStyle = 'rgba(255,255,255,.25)'; ctx.lineWidth = 1.5; ctx.beginPath(); ctx.arc(cx, cy, r * 0.5, 0, Math.PI * 2); ctx.stroke();
      ctx.restore();
      return;
    }
    const ads = s.aim;
    const gap = (w ? (11 - 6 * ads) : 5) * S;
    const len = (w ? 10 : 6) * S;
    ctx.save();
    ctx.lineCap = 'butt';
    const draw = (col, lw) => {
      ctx.strokeStyle = col; ctx.lineWidth = lw;
      ctx.beginPath();
      if (w && !s.build.on) {
        ctx.moveTo(cx - gap - len, cy); ctx.lineTo(cx - gap, cy);
        ctx.moveTo(cx + gap, cy); ctx.lineTo(cx + gap + len, cy);
        ctx.moveTo(cx, cy - gap - len); ctx.lineTo(cx, cy - gap);
        ctx.moveTo(cx, cy + gap); ctx.lineTo(cx, cy + gap + len);
      } else {
        ctx.moveTo(cx - 5 * S, cy); ctx.lineTo(cx + 5 * S, cy); ctx.moveTo(cx, cy - 5 * S); ctx.lineTo(cx, cy + 5 * S);
      }
      ctx.stroke();
    };
    draw('rgba(10,14,40,.85)', 5 * S);
    draw('#fff', 2.4 * S);
    ctx.fillStyle = '#fff'; ctx.beginPath(); ctx.arc(cx, cy, 1.8 * S, 0, Math.PI * 2); ctx.fill();
    // reload ring
    if (w && w.reload > 0) {
      ctx.strokeStyle = 'rgba(8,12,40,.7)'; ctx.lineWidth = 8 * S; ctx.beginPath(); ctx.arc(cx, cy, 36 * S, 0, Math.PI * 2); ctx.stroke();
      ctx.strokeStyle = '#ffe94a'; ctx.lineWidth = 5 * S; ctx.beginPath(); ctx.arc(cx, cy, 36 * S, -Math.PI / 2, -Math.PI / 2 + Math.PI * 2 * w.reload); ctx.stroke();
      this.text('RELOADING', cx, cy + 66 * S, 20 * S, '#ffe94a', 'center');
    } else if (w && w.mag === 0 && w.res === 0) {
      this.text('OUT OF AMMO', cx, cy + 66 * S, 20 * S, '#ff7d7d', 'center');
    }
    // hit marker
    if (s.hit.t > 0) {
      const k = Math.min(1, s.hit.t / 0.22);
      const col = ['#ffffff', '#6cc4ff', '#ffd23a', '#ff3b3b'][s.hit.k] || '#fff';
      const g = (6 + (1 - k) * 8) * S, l = 11 * S;
      ctx.globalAlpha = Math.min(1, k * 1.6);
      ctx.strokeStyle = 'rgba(0,0,0,.6)'; ctx.lineWidth = 6 * S;
      const diag = () => { ctx.beginPath(); for (const [sx, sy] of [[-1, -1], [1, -1], [-1, 1], [1, 1]]) { ctx.moveTo(cx + sx * g, cy + sy * g); ctx.lineTo(cx + sx * (g + l), cy + sy * (g + l)); } ctx.stroke(); };
      diag(); ctx.strokeStyle = col; ctx.lineWidth = 3.4 * S; diag();
    }
    ctx.restore();
  }

  // ---- compass -----------------------------------------------------------------------------------------------------------
  compass(s, W, S) {
    const ctx = this.ctx;
    const heading = ((-s.yaw * 180 / Math.PI) % 360 + 360) % 360;
    const w = 420 * S, h = 30 * S, x = W / 2 - w / 2, y = 14 * S;
    ctx.save();
    ctx.beginPath(); ctx.rect(x, y, w, h); ctx.clip();
    ctx.fillStyle = 'rgba(8,12,44,.45)'; ctx.fillRect(x, y, w, h);
    const ppd = w / 140; // pixels per degree
    for (let d = -90; d <= 90; d += 5) {
      const a = Math.round((heading + d) / 5) * 5;
      const off = (a - heading);
      let diff = ((off + 540) % 360) - 180;
      const px = W / 2 + diff * ppd;
      if (px < x || px > x + w) continue;
      const am = ((a % 360) + 360) % 360;
      const major = am % 45 === 0;
      const lab = { 0: 'N', 90: 'E', 180: 'S', 270: 'W', 45: 'NE', 135: 'SE', 225: 'SW', 315: 'NW' }[am];
      if (am % 15 === 0) {
        ctx.fillStyle = major ? '#fff' : 'rgba(255,255,255,.6)';
        ctx.fillRect(px - 1, y + (major ? 3 : 8) * S, 2, (major ? 8 : 5) * S);
      }
      if (lab) this.text(lab, px, y + 26 * S, (am % 90 === 0 ? 18 : 13) * S, am === 0 ? '#ff6b6b' : '#fff', 'center', { shadow: false });
    }
    ctx.restore();
    ctx.fillStyle = '#ffe94a'; ctx.beginPath(); ctx.moveTo(W / 2, y + h + 5 * S); ctx.lineTo(W / 2 - 6 * S, y + h + 13 * S); ctx.lineTo(W / 2 + 6 * S, y + h + 13 * S); ctx.fill();
    this.text(String(Math.round(heading)).padStart(3, '0'), W / 2, y + h + 31 * S, 14 * S, '#fff', 'center');
  }

  // ---- minimap -----------------------------------------------------------------------------------------------------------------
  minimap(s, W, S) {
    const ctx = this.ctx;
    const size = 214 * S, x = W - size - 22 * S, y = 20 * S;
    const view = 300; // metres across
    const px = s.pos[0], pz = s.pos[2];
    ctx.save();
    ctx.shadowColor = 'rgba(0,0,0,.5)'; ctx.shadowBlur = 16 * S;
    ctx.fillStyle = '#2d86d8'; ctx.fillRect(x, y, size, size);
    ctx.shadowBlur = 0;
    ctx.beginPath(); ctx.rect(x, y, size, size); ctx.clip();
    const scale = size / view; // px per metre
    const toX = (wx) => x + size / 2 + (wx - px) * scale;
    const toY = (wz) => y + size / 2 + (wz - pz) * scale;
    if (this.map) {
      const k = this.map.width / WORLD;
      const sx = (px + WORLD / 2 - view / 2) * k, sy = (pz + WORLD / 2 - view / 2) * k, sw = view * k;
      ctx.imageSmoothingEnabled = true;
      try { ctx.drawImage(this.map, sx, sy, sw, sw, x, y, size, size); } catch (e) { /* source rectangle outside the image */ }
    }
    this.mapOverlays(ctx, s, toX, toY, scale, size / 2, true, S);
    // player arrow
    this.arrow(ctx, x + size / 2, y + size / 2, s.yaw, 10 * S);
    ctx.restore();
    ctx.lineWidth = 3 * S; ctx.strokeStyle = '#fff'; ctx.strokeRect(x, y, size, size);
    // stat boxes
    const by = y + size + 8 * S, bw = 100 * S, bh = 30 * S;
    this.skew(W - 22 * S - bw * 2 - 8 * S, by, bw, bh, 'rgba(8,12,44,.7)', -0.25, 'rgba(255,255,255,.3)');
    drawPerson(ctx, W - 22 * S - bw * 2 + 6 * S, by + bh / 2 + 1, 24 * S);
    this.text(String(s.alive), W - 22 * S - bw - 18 * S, by + bh - 6 * S, 24 * S, '#fff', 'right');
    this.skew(W - 22 * S - bw, by, bw, bh, 'rgba(8,12,44,.7)', -0.25, 'rgba(255,255,255,.3)');
    drawSkull(ctx, W - 22 * S - bw + 18 * S, by + bh / 2 + 1, 24 * S, '#ffd23a');
    this.text(String(s.kills), W - 22 * S - 12 * S, by + bh - 6 * S, 24 * S, '#fff', 'right');
    this.minimapBottom = by + bh;
  }

  arrow(ctx, x, y, yaw, r) {
    // facing in world (x,z): (-sin yaw, -cos yaw); on the north-up map that is screen (fx, fz)
    const fx = -Math.sin(yaw), fz = -Math.cos(yaw);
    const a = Math.atan2(fz, fx);
    ctx.save();
    ctx.translate(x, y); ctx.rotate(a);
    ctx.fillStyle = '#fff'; ctx.strokeStyle = '#0b1030'; ctx.lineWidth = 2.5;
    ctx.beginPath(); ctx.moveTo(r, 0); ctx.lineTo(-r * 0.8, r * 0.75); ctx.lineTo(-r * 0.35, 0); ctx.lineTo(-r * 0.8, -r * 0.75); ctx.closePath();
    ctx.stroke(); ctx.fill();
    ctx.restore();
  }

  /** Storm circles, bus route and so on, shared by the minimap and the full map. */
  mapOverlays(ctx, s, toX, toY, scale, half, mini, S) {
    const st = s.storm;
    if (st.on) {
      // purple storm outside the current circle
      ctx.save();
      ctx.fillStyle = 'rgba(120, 50, 230, .38)';
      ctx.beginPath();
      const big = 20000;
      ctx.rect(toX(st.cx) - big, toY(st.cz) - big, big * 2, big * 2);
      ctx.arc(toX(st.cx), toY(st.cz), st.r * scale, 0, Math.PI * 2, true);
      ctx.fill('evenodd');
      ctx.strokeStyle = 'rgba(190, 120, 255, .95)'; ctx.lineWidth = (mini ? 2.5 : 3.5) * S;
      ctx.beginPath(); ctx.arc(toX(st.cx), toY(st.cz), st.r * scale, 0, Math.PI * 2); ctx.stroke();
      // the next safe zone
      ctx.strokeStyle = '#fff'; ctx.lineWidth = (mini ? 2.5 : 3.5) * S; ctx.setLineDash([8 * S, 5 * S]);
      ctx.beginPath(); ctx.arc(toX(st.ncx), toY(st.ncz), st.nr * scale, 0, Math.PI * 2); ctx.stroke();
      ctx.restore();
    }
    if (s.bus.on) {
      const bx = toX(s.bus.x), bz = toY(s.bus.z);
      ctx.save();
      ctx.strokeStyle = 'rgba(255,255,255,.65)'; ctx.lineWidth = 2 * S; ctx.setLineDash([10 * S, 8 * S]);
      ctx.beginPath(); ctx.moveTo(bx - s.bus.dx * 2400 * scale, bz - s.bus.dz * 2400 * scale); ctx.lineTo(bx + s.bus.dx * 2400 * scale, bz + s.bus.dz * 2400 * scale); ctx.stroke();
      ctx.setLineDash([]);
      ctx.translate(bx, bz); ctx.rotate(Math.atan2(s.bus.dz, s.bus.dx));
      ctx.fillStyle = '#3fa0ff'; ctx.strokeStyle = '#fff'; ctx.lineWidth = 2;
      const r = (mini ? 8 : 12) * S;
      ctx.beginPath(); ctx.moveTo(r * 1.3, 0); ctx.lineTo(-r, r * 0.8); ctx.lineTo(-r, -r * 0.8); ctx.closePath(); ctx.fill(); ctx.stroke();
      ctx.restore();
    }
  }

  // ---- storm info -----------------------------------------------------------------------------------------------------------------
  stormInfo(s, W, S) {
    const st = s.storm;
    if (!st.on || s.ph === 0) return;
    const ctx = this.ctx;
    const size = 214 * S;
    const x = W - size - 22 * S - 0, y = (this.minimapBottom || 270 * S) + 10 * S;
    const w = size, h = 46 * S;
    const closing = st.state === 1;
    const col = closing ? '#ff9a3d' : '#b78bff';
    this.skew(x, y, w, h, 'rgba(8,12,44,.72)', -0.25, col);
    drawStorm(ctx, x + 26 * S, y + h / 2, 34 * S, col);
    this.text(closing ? 'STORM CLOSING' : 'STORM FORMS IN', x + 54 * S, y + 18 * S, 15 * S, '#cfd8ff');
    this.text(fmtTime(st.timer), x + 54 * S, y + h - 8 * S, 28 * S, '#fff');
    this.text(`PHASE ${st.phase + 1}`, x + w - 10 * S, y + h - 10 * S, 16 * S, col, 'right');
    this.stormBottom = y + h;
    if (!st.in) {
      const pulse = 0.55 + 0.45 * Math.sin(this.time * 6);
      this.text(`IN THE STORM  -${st.dps} HP/s`, W / 2, 112 * S, 30 * S, `rgba(255,120,150,${0.6 + pulse * 0.4})`, 'center');
    }
  }

  // ---- kill feed ---------------------------------------------------------------------------------------------------------------------
  feed(s, W, S) {
    const ctx = this.ctx;
    let y = (this.stormBottom || this.minimapBottom || 300 * S) + 12 * S;
    const xr = W - 22 * S;
    for (const f of s.feed.slice(-6)) {
      const a = Math.max(0, Math.min(1, (7 - f.age) / 1.2, f.age / 0.12 + 0.2));
      ctx.save(); ctx.globalAlpha = a;
      const mine = f.me, you = f.you;
      const killer = f.s ? 'The Storm' : (f.k || 'Fall');
      const verb = f.s ? '' : '';
      const line = `${killer}  ▸  ${f.v}`;
      ctx.font = `italic 900 ${19 * S}px ${FONT}`;
      const tw = ctx.measureText(line).width;
      const w = tw + 24 * S, h = 28 * S;
      this.skew(xr - w, y, w, h, mine ? 'rgba(255,210,40,.92)' : you ? 'rgba(255,70,90,.88)' : 'rgba(8,12,44,.7)', -0.25);
      this.text(line, xr - 12 * S, y + h - 7 * S, 19 * S, mine ? '#10163d' : '#fff', 'right', { shadow: !mine });
      ctx.restore();
      y += h + 5 * S;
    }
  }

  // ---- toast + notices ---------------------------------------------------------------------------------------------------------------------
  toasts(s, W, S) {
    let y = 128 * S;
    for (const t of s.toast) {
      const [txt, secs, style] = t;
      const a = Math.min(1, secs / 0.5, 1);
      const ctx = this.ctx;
      ctx.save(); ctx.globalAlpha = a;
      ctx.font = `italic 900 ${28 * S}px ${FONT}`;
      const w = ctx.measureText(txt.toUpperCase()).width + 64 * S;
      const col = style === 2 ? 'rgba(255,120,30,.92)' : style === 1 ? 'rgba(130,70,255,.92)' : style === 3 ? 'rgba(255,60,80,.92)' : 'rgba(30,140,255,.92)';
      this.skew(W / 2 - w / 2, y, w, 44 * S, col, -0.25);
      this.text(txt.toUpperCase(), W / 2, y + 32 * S, 28 * S, '#fff', 'center');
      ctx.restore();
      y += 52 * S;
    }
  }

  notice(s, W, H, S) {
    let y = H - 150 * S;
    for (const n of this.notices.slice().reverse()) {
      const a = Math.min(1, (3.2 - n.age) / 0.6, n.age / 0.1 + 0.3);
      const ctx = this.ctx;
      ctx.save(); ctx.globalAlpha = a;
      const col = n.r >= 0 ? RARITY[n.r] : n.mat != null ? MAT_COLORS[n.mat] : '#fff';
      ctx.font = `italic 900 ${20 * S}px ${FONT}`;
      const w = ctx.measureText(n.txt.toUpperCase()).width + 30 * S;
      this.skew(36 * S, y - 22 * S, w, 28 * S, 'rgba(8,12,44,.72)', -0.25);
      this.skew(36 * S, y - 22 * S, 5 * S, 28 * S, col, -0.25);
      this.text(n.txt.toUpperCase(), 50 * S, y - 2 * S, 20 * S, '#fff');
      ctx.restore();
      y -= 34 * S;
    }
  }

  bannersDraw(s, W, H, S) {
    const ctx = this.ctx;
    for (const b of this.banners) {
      const pop = Math.min(1, b.age / 0.18);
      const a = Math.min(1, (2.8 - b.age) / 0.5);
      const scale = 0.6 + 0.4 * (1 - Math.pow(1 - pop, 3)) + Math.max(0, 0.15 - b.age) * 1.5;
      ctx.save(); ctx.globalAlpha = a;
      ctx.translate(W / 2, H * 0.62); ctx.scale(scale, scale);
      this.text('ELIMINATED', 0, 0, 46 * S, '#ffe94a', 'center');
      this.text(b.name, 0, 42 * S, 34 * S, '#ffffff', 'center');
      ctx.restore();
    }
  }

  // ---- prompts ------------------------------------------------------------------------------------------------------------------------------
  prompt(s, W, H, S) {
    const p = s.prompt;
    if (!p || s.mode === 'bus' || s.mode === 'freefall' || s.mode === 'glide') return;
    const ctx = this.ctx;
    const w = 300 * S, h = 62 * S, x = W / 2 - w / 2 + 120 * S, y = H / 2 + 60 * S;
    const col = RARITY[p.rar] || '#fff';
    this.skew(x, y, w, h, 'rgba(8,12,44,.82)', -0.25);
    this.skew(x, y, 6 * S, h, col, -0.25);
    // key cap
    this.skew(x + 18 * S, y + 14 * S, 34 * S, 34 * S, '#fff', -0.15);
    this.text('E', x + 35 * S, y + 42 * S, 28 * S, '#10163d', 'center', { shadow: false });
    this.text(p.kind === 'chest' ? 'OPEN CHEST' : p.kind === 'w' ? 'PICK UP' : 'COLLECT', x + 66 * S, y + 24 * S, 15 * S, '#b9c9ff');
    this.text(p.txt + (p.cnt > 1 ? `  x${p.cnt}` : ''), x + 66 * S, y + 50 * S, 24 * S, col);
  }

  heal(s, W, H, S) {
    if (s.heal < 0) return;
    const w = 260 * S, h = 12 * S, x = W / 2 - w / 2, y = H - 130 * S;
    this.skew(x - 3, y - 3, w + 6, h + 6, 'rgba(8,12,44,.8)');
    this.skew(x, y, w * s.heal, h, '#6aff9a');
    this.text('USING ITEM', W / 2, y - 12 * S, 18 * S, '#fff', 'center');
  }

  busPrompts(s, W, H, S) {
    if (s.mode === 'bus') {
      const pulse = 0.65 + 0.35 * Math.sin(this.time * 5);
      this.text('PRESS SPACE TO JUMP FROM THE BATTLE BUS', W / 2, H * 0.2, 38 * S, `rgba(255,255,255,${pulse})`, 'center');
      this.text(`The bus leaves the island in ${fmtTime(s.bus.left)}`, W / 2, H * 0.2 + 36 * S, 20 * S, '#bcd8ff', 'center');
    } else if (s.mode === 'freefall') {
      this.text('PRESS SPACE TO OPEN GLIDER', W / 2, H * 0.2, 34 * S, '#fff', 'center');
      this.altimeter(s, W, H, S);
    } else if (s.mode === 'glide') {
      this.altimeter(s, W, H, S);
    }
  }

  altimeter(s, W, H, S) {
    const ctx = this.ctx;
    const agl = Math.max(0, s.agl || 0);
    const x = 36 * S, y = H / 2 - 120 * S, h = 240 * S;
    ctx.save();
    ctx.fillStyle = 'rgba(8,12,44,.6)'; ctx.fillRect(x, y, 16 * S, h);
    const k = Math.max(0, Math.min(1, agl / 300));
    ctx.fillStyle = k < 0.25 ? '#ff7d5a' : '#7dd3ff'; ctx.fillRect(x, y + h * (1 - k), 16 * S, h * k);
    ctx.restore();
    this.text(`${Math.round(agl)} m`, x + 26 * S, y + h * (1 - k) + 8 * S, 24 * S, '#fff');
    this.text('ALTITUDE', x - 4 * S, y - 10 * S, 14 * S, '#bcd8ff');
  }

  spectating(s, W, H, S) {
    if (!s.spec) return;
    this.text('SPECTATING', W / 2, H - 120 * S, 22 * S, '#cfe0ff', 'center');
    this.text(s.spec, W / 2, H - 80 * S, 46 * S, '#fff', 'center');
    this.text('◀  A / D  or click to switch player  ▶', W / 2, H - 46 * S, 17 * S, '#bcd0ff', 'center', { italic: false, weight: 600 });
  }

  // ---- floating numbers -------------------------------------------------------------------------------------------------------------------------------
  damageNumbers(s, W, H, S) {
    for (const d of s.dmg) {
      if (d.age > 2) continue;
      const x = (d.x * 0.5 + 0.5) * W, y = (1 - (d.y * 0.5 + 0.5)) * H;
      const a = Math.max(0, 1 - d.age / 1.0);
      const size = (d.k === 2 ? 36 : 26) * S * (1 + Math.max(0, 0.25 - d.age) * 2);
      const col = d.k === 2 ? '#ffc933' : d.k === 1 ? '#66c4ff' : '#ffffff';
      this.ctx.save(); this.ctx.globalAlpha = a;
      this.text(String(d.a), x, y, size, col, 'center');
      this.ctx.restore();
    }
  }

  damageIndicators(s, W, H, S) {
    if (!s.ind || !s.ind.length) return;
    const ctx = this.ctx;
    for (const d of s.ind) {
      ctx.save();
      ctx.translate(W / 2, H / 2); ctx.rotate(d.a);
      const r = Math.min(W, H) * 0.34;
      const g = ctx.createRadialGradient(0, -r, 0, 0, -r, 90 * S);
      g.addColorStop(0, `rgba(255,40,50,${0.85 * d.t})`); g.addColorStop(1, 'rgba(255,40,50,0)');
      ctx.fillStyle = g; ctx.beginPath(); ctx.arc(0, -r, 90 * S, 0, Math.PI * 2); ctx.fill();
      ctx.restore();
    }
  }
}

/** Draw the full-screen island map into its own canvas. */
export function drawFullMap(canvas, mapImage, s, pois) {
  const ctx = canvas.getContext('2d');
  const N = canvas.width;
  ctx.clearRect(0, 0, N, N);
  ctx.imageSmoothingEnabled = true;
  if (mapImage) ctx.drawImage(mapImage, 0, 0, N, N);
  const scale = N / WORLD;
  const toX = (x) => (x + WORLD / 2) * scale, toY = (z) => (z + WORLD / 2) * scale;
  const hud = { arrow: Hud.prototype.arrow, mapOverlays: Hud.prototype.mapOverlays };
  hud.mapOverlays(ctx, s, toX, toY, scale, N / 2, false, 1.4);
  // points of interest
  ctx.textAlign = 'center'; ctx.textBaseline = 'middle';
  for (const p of pois) {
    ctx.font = `italic 900 26px ${FONT}`;
    ctx.lineWidth = 6; ctx.strokeStyle = 'rgba(8,12,44,.9)'; ctx.lineJoin = 'round';
    ctx.strokeText(p.name.toUpperCase(), toX(p.x), toY(p.z));
    ctx.fillStyle = '#fff'; ctx.fillText(p.name.toUpperCase(), toX(p.x), toY(p.z));
  }
  // you
  const px = toX(s.pos[0]), pz = toY(s.pos[2]);
  const pulse = 0.5 + 0.5 * Math.sin(performance.now() / 250);
  ctx.strokeStyle = `rgba(255,255,255,${0.35 + 0.4 * pulse})`; ctx.lineWidth = 3;
  ctx.beginPath(); ctx.arc(px, pz, 16 + pulse * 8, 0, Math.PI * 2); ctx.stroke();
  hud.arrow(ctx, px, pz, s.yaw, 16);
  ctx.font = `italic 900 22px ${FONT}`; ctx.lineWidth = 5; ctx.strokeStyle = 'rgba(8,12,44,.9)';
  ctx.strokeText('YOU', px, pz - 32); ctx.fillStyle = '#ffe94a'; ctx.fillText('YOU', px, pz - 32);
}
