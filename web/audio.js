// WebAudio engine. Every sound effect is synthesised by the wasm module (see audio_synth.rs);
// this file turns the rendered sample buffers into positional sound.

export class GameAudio {
  constructor() {
    this.ctx = null;
    this.master = null;
    this.names = [];
    this.variants = [];
    this.isLoop = [];
    this.buffers = []; // [sfx][variant] -> AudioBuffer
    this.loops = {};
    this.active = 0;
    this.volume = 0.7;
    this.ready = false;
  }

  /** Create and resume the context synchronously: Safari only allows that inside the user gesture itself. */
  unlock() {
    const AC = window.AudioContext || window.webkitAudioContext;
    if (!AC) return;
    if (!this.ctx) {
      try { this.ctx = new AC({ latencyHint: 'interactive' }); } catch (e) { return; } // no audio device: play silently
    }
    this.resume();
  }

  /** Builds the sound bank (call `unlock()` first, from a user gesture). */
  async init(fn, onProgress) {
    if (this.ready) { this.resume(); return; }
    this.unlock();
    if (!this.ctx) return;
    const comp = this.ctx.createDynamicsCompressor();
    comp.threshold.value = -14; comp.knee.value = 18; comp.ratio.value = 4; comp.attack.value = 0.003; comp.release.value = 0.2;
    this.master = this.ctx.createGain();
    this.master.gain.value = this.curve(this.volume);
    this.master.connect(comp); comp.connect(this.ctx.destination);
    const manifest = fn.audio_manifest().split(';').map((s) => s.split(':'));
    this.names = manifest.map((m) => m[0]);
    this.variants = manifest.map((m) => +m[1]);
    this.isLoop = manifest.map((m) => m[2] === '1');
    const sr = this.ctx.sampleRate;
    const total = this.variants.reduce((a, b) => a + b, 0);
    let done = 0;
    for (let i = 0; i < this.names.length; i++) {
      this.buffers[i] = [];
      for (let v = 0; v < this.variants[i]; v++) {
        const data = fn.audio_render(i, v, sr);
        const buf = this.ctx.createBuffer(1, Math.max(1, data.length), sr);
        buf.copyToChannel(data, 0);
        this.buffers[i][v] = buf;
        done++;
      }
      if (onProgress && i % 4 === 0) { onProgress(done / total); await new Promise((r) => setTimeout(r, 0)); }
    }
    // continuous loops
    for (const [key, name] of [['bus', 'bus_loop'], ['wind', 'wind_loop'], ['glider', 'glider_loop'], ['storm', 'storm_ambient'], ['hum', 'chest_hum']]) {
      const idx = this.names.indexOf(name);
      if (idx < 0) continue;
      const src = this.ctx.createBufferSource();
      src.buffer = this.buffers[idx][0]; src.loop = true;
      const gain = this.ctx.createGain(); gain.gain.value = 0;
      const pan = this.ctx.createStereoPanner();
      src.connect(gain); gain.connect(pan); pan.connect(this.master);
      src.start();
      this.loops[key] = { src, gain, pan };
    }
    this.ready = true;
  }

  curve(v) { return Math.pow(v, 1.6) * 1.1; }

  setVolume(v) {
    this.volume = v;
    if (this.master) this.master.gain.setTargetAtTime(this.curve(v), this.ctx.currentTime, 0.05);
  }

  resume() { if (this.ctx && this.ctx.state === 'suspended') this.ctx.resume(); }

  suspend() { if (this.ctx && this.ctx.state === 'running') this.ctx.suspend(); }

  /** One-shot cues from the game: [sfx, variant, pan, gain, pitch, lowpass, delay, _] x n */
  playCues(arr) {
    if (!this.ready || this.ctx.state !== 'running') return;
    const n = arr.length / 8;
    // when many sounds arrive at once keep the loudest
    const idx = [];
    for (let i = 0; i < n; i++) idx.push(i);
    if (n > 14) idx.sort((a, b) => arr[b * 8 + 3] - arr[a * 8 + 3]);
    for (let k = 0; k < Math.min(n, 14); k++) {
      const o = idx[k] * 8;
      this.play(arr[o] | 0, arr[o + 1] | 0, arr[o + 2], arr[o + 3], arr[o + 4], arr[o + 5], arr[o + 6]);
    }
  }

  play(sfx, variant, pan = 0, gain = 1, pitch = 1, lowpass = 20000, delay = 0) {
    if (!this.ready || this.active > 56) return;
    const set = this.buffers[sfx];
    if (!set) return;
    const buf = set[variant % set.length];
    const ctx = this.ctx;
    const src = ctx.createBufferSource();
    src.buffer = buf; src.playbackRate.value = pitch;
    const g = ctx.createGain(); g.gain.value = gain;
    let node = g;
    src.connect(g);
    if (lowpass < 18000) {
      const f = ctx.createBiquadFilter(); f.type = 'lowpass'; f.frequency.value = lowpass; f.Q.value = 0.5;
      g.connect(f); node = f;
    }
    if (Math.abs(pan) > 0.01) { const p = ctx.createStereoPanner(); p.pan.value = pan; node.connect(p); node = p; }
    node.connect(this.master);
    this.active++;
    src.onended = () => { this.active--; };
    src.start(ctx.currentTime + Math.max(0, delay));
  }

  playUi(name, gain = 0.6) {
    const i = this.names.indexOf(name);
    if (i >= 0) this.play(i, 0, 0, gain, 1, 20000, 0);
  }

  /** [bus, wind, glider, storm, hum, humPan] */
  setLoops(l) {
    if (!this.ready) return;
    const t = this.ctx.currentTime;
    const set = (key, v, pan = 0) => {
      const L = this.loops[key]; if (!L) return;
      L.gain.gain.setTargetAtTime(Math.max(0, v), t, 0.12);
      L.pan.pan.setTargetAtTime(pan, t, 0.12);
    };
    set('bus', l[0]); set('wind', l[1]); set('glider', l[2]); set('storm', l[3]); set('hum', l[4], l[5]);
  }

  silenceLoops() { this.setLoops([0, 0, 0, 0, 0, 0]); }
}
