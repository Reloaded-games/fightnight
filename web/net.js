// Multiplayer transport: WebRTC data channels between two browsers, set up by hand-exchanged invite codes
// (there is no server: the host makes an invite code, the guest answers with a reply code, and the two connect directly).
//
// A connection has two channels. "r" is reliable and ordered: joining, the lobby, the start of the match, changes to loot
// and buildings, one-shot events. "u" is unreliable and unordered: the player's commands going up and the snapshots coming
// down, which are replaced every few milliseconds anyway and must never queue up behind a lost packet.

const ICE_SERVERS = [{ urls: 'stun:stun.l.google.com:19302' }, { urls: 'stun:stun1.l.google.com:19302' }];
const GATHER_TIMEOUT_MS = 4000;
const CODE_PREFIX = 'FN1.';

// ---- invite codes: JSON -> deflate -> base64url ------------------------------------------------------------------------
const b64 = {
  enc(bytes) {
    let s = '';
    for (const b of bytes) s += String.fromCharCode(b);
    return btoa(s).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
  },
  dec(text) {
    const s = atob(text.replace(/-/g, '+').replace(/_/g, '/'));
    const out = new Uint8Array(s.length);
    for (let i = 0; i < s.length; i++) out[i] = s.charCodeAt(i);
    return out;
  },
};

async function pipe(bytes, stream) {
  const out = new Blob([bytes]).stream().pipeThrough(stream);
  return new Uint8Array(await new Response(out).arrayBuffer());
}

export async function packCode(obj) {
  const raw = new TextEncoder().encode(JSON.stringify(obj));
  try { return CODE_PREFIX + b64.enc(await pipe(raw, new CompressionStream('deflate-raw'))); } catch (e) { return 'FN0.' + b64.enc(raw); }
}

export async function unpackCode(code) {
  const text = String(code || '').replace(/\s+/g, '');
  if (text.startsWith('FN0.')) return JSON.parse(new TextDecoder().decode(b64.dec(text.slice(4))));
  if (!text.startsWith(CODE_PREFIX)) throw new Error('That is not an invite code.');
  const raw = await pipe(b64.dec(text.slice(CODE_PREFIX.length)), new DecompressionStream('deflate-raw'));
  return JSON.parse(new TextDecoder().decode(raw));
}

function gathered(pc) {
  return new Promise((resolve) => {
    if (pc.iceGatheringState === 'complete') return resolve();
    const done = () => { pc.removeEventListener('icegatheringstatechange', check); clearTimeout(timer); resolve(); };
    const check = () => { if (pc.iceGatheringState === 'complete') done(); };
    const timer = setTimeout(done, GATHER_TIMEOUT_MS);
    pc.addEventListener('icegatheringstatechange', check);
  });
}

// ---- one connection ---------------------------------------------------------------------------------------------------
// handlers: { open(), close(why), message(Uint8Array) }
export class Link {
  constructor(handlers) {
    this.h = handlers;
    this.pc = new RTCPeerConnection({ iceServers: ICE_SERVERS });
    this.r = null; // reliable channel
    this.u = null; // unreliable channel
    this.opened = false;
    this.closed = false;
    this.pc.addEventListener('connectionstatechange', () => {
      const s = this.pc.connectionState;
      if (s === 'failed' || s === 'closed') this.#lost('The connection failed.');
      else if (s === 'disconnected') {
        // often a blip that heals itself
        clearTimeout(this.blip);
        this.blip = setTimeout(() => { if (this.pc.connectionState !== 'connected') this.#lost('The connection was interrupted.'); }, 5000);
      }
    });
  }

  #adopt(ch) {
    ch.binaryType = 'arraybuffer';
    if (ch.label === 'r') this.r = ch; else if (ch.label === 'u') this.u = ch; else return;
    ch.addEventListener('open', () => this.#maybeOpen());
    ch.addEventListener('close', () => this.#lost('The connection closed.'));
    ch.addEventListener('message', (ev) => { if (!this.closed) this.h.message(new Uint8Array(ev.data)); });
    if (ch.readyState === 'open') this.#maybeOpen();
  }

  #maybeOpen() {
    if (this.opened || !this.r || !this.u || this.r.readyState !== 'open' || this.u.readyState !== 'open') return;
    this.opened = true;
    this.h.open();
  }

  #lost(why) {
    if (this.closed) return;
    this.closed = true;
    clearTimeout(this.blip);
    try { this.pc.close(); } catch (e) { /* already closed */ }
    this.h.close(why);
  }

  /** Host: make an invite code for one guest. */
  async createInvite() {
    this.#adopt(this.pc.createDataChannel('r', { ordered: true }));
    this.#adopt(this.pc.createDataChannel('u', { ordered: false, maxRetransmits: 0 }));
    await this.pc.setLocalDescription(await this.pc.createOffer());
    await gathered(this.pc);
    return packCode({ t: 'offer', sdp: this.pc.localDescription.sdp });
  }

  /** Guest: take the host's invite and make the reply code. */
  async acceptInvite(code) {
    const o = await unpackCode(code);
    if (o.t !== 'offer') throw new Error('That is a reply code. Paste the host\'s invite code here.');
    this.pc.addEventListener('datachannel', (ev) => this.#adopt(ev.channel));
    await this.pc.setRemoteDescription({ type: 'offer', sdp: o.sdp });
    await this.pc.setLocalDescription(await this.pc.createAnswer());
    await gathered(this.pc);
    return packCode({ t: 'answer', sdp: this.pc.localDescription.sdp });
  }

  /** Host: take the guest's reply; the connection opens by itself. */
  async acceptReply(code) {
    const o = await unpackCode(code);
    if (o.t !== 'answer') throw new Error('That is an invite code. Paste the guest\'s reply code here.');
    await this.pc.setRemoteDescription({ type: 'answer', sdp: o.sdp });
  }

  send(reliable, bytes) {
    if (this.closed || !this.opened) return;
    const ch = reliable ? this.r : this.u;
    try {
      // an unreliable packet that would only queue up behind a jammed connection is not worth sending
      if (!reliable && ch.bufferedAmount > 256 * 1024) return;
      ch.send(bytes);
    } catch (e) { /* the channel closed under us; the close handler reports it */ }
  }

  close() {
    if (this.closed) return;
    this.closed = true;
    clearTimeout(this.blip);
    try { this.pc.close(); } catch (e) { /* ignore */ }
  }
}

// ---- the page's end of the wasm network API ---------------------------------------------------------------------------

/** Send what the game has queued: repeated [peer u32][reliable u8][length u32][bytes]. `route(peer)` finds the Link. */
export function flush(fn, route) {
  const buf = fn.net_poll();
  if (!buf.length) return 0;
  const dv = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
  let at = 0, n = 0;
  while (at + 9 <= buf.length) {
    const peer = dv.getUint32(at, true), reliable = buf[at + 4] === 1, len = dv.getUint32(at + 5, true);
    at += 9;
    if (at + len > buf.length) break;
    const link = route(peer);
    if (link) link.send(reliable, buf.subarray(at, at + len));
    at += len;
    n++;
  }
  return n;
}
