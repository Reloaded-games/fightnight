// Rooms: how players find each other without a game server.
//
// The page talks to free public MQTT brokers over WebSockets, several at once so one being down does not matter, and it
// redials any that drop. Anyone can read and write on a public broker, so:
// - a public room is a retained message under `<prefix>/rooms/<listing>` (a random id, not the room code) that the host
//   refreshes while the room is open and the broker clears when the host goes away. It carries the room code, so
//   everybody who opens "Join a game" can join, and the host's public key, so nobody else can answer for the host;
// - a private room is never published at all: its code is the only way in;
// - the topics a room's joins go through are derived from its code with a slow hash, and every message on them is
//   encrypted with a key derived from the code, so a code is never seen on the wire and nobody without it can read or
//   forge them. Each join also agrees a key of its own with the host (ECDH): the WebRTC offer and answer, which hold the
//   players' addresses, are sealed with it, so other players of the room cannot read them or answer in someone's place.
// The host answers a join with a WebRTC offer, the guest's page answers back, and from then on the two talk directly
// (net.js). Nobody copies or pastes anything: the room code (6 letters and digits) is all a friend needs.

const DEFAULT_BROKERS = ['wss://broker.emqx.io:8084/mqtt', 'wss://broker.hivemq.com:8884/mqtt', 'wss://test.mosquitto.org:8081/mqtt'];
const CODE_ALPHABET = 'ABCDEFGHJKLMNPQRSTUVWXYZ23456789'; // no 0/O or 1/I to misread
export const CODE_LENGTH = 6;
const REFRESH_MS = 8000; // a listed room is announced again this often
const STALE_MS = 25000; // and drops off the list when it has not been for this long
const RETAINED_MAX_AGE_MS = 10 * 60 * 1000; // a kept announcement older than this is a room that is long gone
const KEEPALIVE_S = 30;
const MAX_LISTED = 40;
const MAX_PAYLOAD = 16 * 1024; // nothing the room service carries is bigger
const KDF_ITERATIONS = 100000;
const KDF_SALT = 'fightnight/rooms2';
const ID_RE = /^[0-9a-f]{16}$/;

const enc = new TextEncoder();
const dec = new TextDecoder();

export function randomCode(n = CODE_LENGTH) {
  const bytes = crypto.getRandomValues(new Uint8Array(n));
  return [...bytes].map((b) => CODE_ALPHABET[b % CODE_ALPHABET.length]).join('');
}

/** A room code as typed: upper case, spaces and anything that cannot be in a code dropped. */
export function cleanCode(text) {
  return String(text || '').toUpperCase().split('').filter((c) => CODE_ALPHABET.includes(c)).join('').slice(0, CODE_LENGTH);
}

const isCode = (c) => typeof c === 'string' && c.length === CODE_LENGTH && cleanCode(c) === c;
const hex = (bytes) => [...bytes].map((b) => b.toString(16).padStart(2, '0')).join('');
function randomId() { return hex(crypto.getRandomValues(new Uint8Array(8))); }
const b64 = {
  enc(bytes) { let s = ''; for (const b of bytes) s += String.fromCharCode(b); return btoa(s); },
  dec(text) { const s = atob(text); const out = new Uint8Array(s.length); for (let i = 0; i < s.length; i++) out[i] = s.charCodeAt(i); return out; },
};

// ---- keys -------------------------------------------------------------------------------------------------------------------
/** What a room code unlocks: the topic its joins go through and the key that seals them. */
async function roomKeys(code) {
  const base = await crypto.subtle.importKey('raw', enc.encode(code), 'PBKDF2', false, ['deriveBits']);
  const bits = new Uint8Array(await crypto.subtle.deriveBits({ name: 'PBKDF2', salt: enc.encode(KDF_SALT), iterations: KDF_ITERATIONS, hash: 'SHA-256' }, base, 512));
  const key = await crypto.subtle.importKey('raw', bits.slice(32, 64), 'AES-GCM', false, ['encrypt', 'decrypt']);
  return { topic: hex(bits.slice(0, 12)), key };
}

async function seal(key, obj) {
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const ct = new Uint8Array(await crypto.subtle.encrypt({ name: 'AES-GCM', iv }, key, enc.encode(JSON.stringify(obj))));
  return b64.enc(iv) + '.' + b64.enc(ct);
}

/** The object sealed in `box`, or null if it was not sealed with `key` (or is not a box at all). */
async function unseal(key, box) {
  if (typeof box !== 'string' || box.length > MAX_PAYLOAD) return null;
  try {
    const [iv, ct] = box.split('.');
    const pt = await crypto.subtle.decrypt({ name: 'AES-GCM', iv: b64.dec(iv) }, key, b64.dec(ct));
    return JSON.parse(dec.decode(pt));
  } catch (e) { return null; }
}

const ECDH = { name: 'ECDH', namedCurve: 'P-256' };
async function keyPair() {
  const pair = await crypto.subtle.generateKey(ECDH, false, ['deriveBits']);
  return { priv: pair.privateKey, pub: b64.enc(new Uint8Array(await crypto.subtle.exportKey('raw', pair.publicKey))) };
}

/** The key two sides agree on from one's private key and the other's public one. */
async function sharedKey(priv, theirPub) {
  if (typeof theirPub !== 'string' || theirPub.length > 200) throw new Error('bad key');
  const pub = await crypto.subtle.importKey('raw', b64.dec(theirPub), ECDH, false, []);
  const bits = await crypto.subtle.deriveBits({ name: 'ECDH', public: pub }, priv, 256);
  const digest = await crypto.subtle.digest('SHA-256', new Uint8Array([...new Uint8Array(bits), ...enc.encode('fightnight/join')]));
  return crypto.subtle.importKey('raw', digest, 'AES-GCM', false, ['encrypt', 'decrypt']);
}

// ---- a minimal MQTT 3.1.1 client over a WebSocket (QoS 0, retained messages, last will) ---------------------------------
function varint(n) {
  const out = [];
  do {
    let b = n % 128;
    n = Math.floor(n / 128);
    if (n > 0) b |= 0x80;
    out.push(b);
  } while (n > 0);
  return out;
}
function str(s) {
  const b = enc.encode(s);
  if (b.length > 65535) throw new Error('string too long for MQTT');
  return [b.length >> 8, b.length & 255, ...b];
}
function packet(type, flags, body) {
  return new Uint8Array([(type << 4) | flags, ...varint(body.length), ...body]);
}
// a topic name we publish to: never empty, never a wildcard, never a NUL (a broker hangs up on any of those)
const goodTopic = (t) => typeof t === 'string' && t.length > 0 && t.length < 512 && !/[+#\u0000]/.test(t);

class MqttSocket {
  constructor(url, { clientId, will, onMessage, onClose }) {
    this.url = url;
    this.clientId = clientId;
    this.will = will;
    this.onMessage = onMessage;
    this.onClose = onClose;
    this.buf = new Uint8Array(0);
    this.ready = false;
    this.nextId = 1;
    this.lastRx = 0;
  }

  connect(timeoutMs = 8000) {
    return new Promise((resolve, reject) => {
      let settled = false;
      const fail = (why) => { if (!settled) { settled = true; reject(new Error(why)); } try { this.ws?.close(); } catch (e) { /* closed */ } };
      const timer = setTimeout(() => fail('timed out'), timeoutMs);
      try { this.ws = new WebSocket(this.url, ['mqtt']); } catch (e) { clearTimeout(timer); reject(e); return; }
      this.ws.binaryType = 'arraybuffer';
      this.ws.onopen = () => {
        let flags = 0x02; // clean session
        const payload = [...str(this.clientId)];
        if (this.will) {
          flags |= 0x04 | (this.will.retain ? 0x20 : 0);
          const w = typeof this.will.payload === 'string' ? enc.encode(this.will.payload) : this.will.payload;
          payload.push(...str(this.will.topic), w.length >> 8, w.length & 255, ...w);
        }
        const body = [...str('MQTT'), 4, flags, KEEPALIVE_S >> 8, KEEPALIVE_S & 255, ...payload];
        this.ws.send(packet(1, 0, body));
      };
      this.ws.onmessage = (ev) => {
        this.lastRx = Date.now();
        const more = new Uint8Array(ev.data);
        const all = new Uint8Array(this.buf.length + more.length);
        all.set(this.buf); all.set(more, this.buf.length);
        this.buf = all;
        this.#drain((type, flags, body) => {
          if (type === 2) { // CONNACK
            clearTimeout(timer);
            if (body[1] !== 0) { fail('refused (' + body[1] + ')'); return; }
            this.ready = true;
            settled = true;
            this.ping = setInterval(() => {
              // a connection that has said nothing for a keep-alive and a half (pings included) is dead
              if (Date.now() - this.lastRx > KEEPALIVE_S * 1500) { try { this.ws.close(); } catch (e) { /* closed */ } return; }
              this.#send(packet(12, 0, []));
            }, (KEEPALIVE_S * 1000) / 2);
            resolve(this);
          } else if (type === 3) { // PUBLISH
            const tl = (body[0] << 8) | body[1];
            const topic = dec.decode(body.subarray(2, 2 + tl));
            let at = 2 + tl;
            if ((flags >> 1) & 3) at += 2; // a packet id (we only subscribe at QoS 0, but be tolerant)
            this.onMessage?.(topic, body.subarray(at), !!(flags & 1));
          }
        });
      };
      this.ws.onerror = () => fail('could not connect');
      this.ws.onclose = () => {
        clearTimeout(timer);
        clearInterval(this.ping);
        const was = this.ready;
        this.ready = false;
        if (!settled) fail('closed');
        else if (was) this.onClose?.();
      };
    });
  }

  #drain(handle) {
    for (;;) {
      if (this.buf.length < 2) return;
      let len = 0, mul = 1, i = 1;
      for (;;) {
        if (i >= this.buf.length) return; // the length is not all here yet
        const b = this.buf[i++];
        len += (b & 127) * mul;
        mul *= 128;
        if (!(b & 128)) break;
        if (i > 4) { this.buf = new Uint8Array(0); return; }
      }
      if (this.buf.length < i + len) return;
      const head = this.buf[0];
      const body = this.buf.slice(i, i + len);
      this.buf = this.buf.slice(i + len);
      handle(head >> 4, head & 15, body);
    }
  }

  #send(bytes) {
    if (this.ws?.readyState === 1) {
      try { this.ws.send(bytes); } catch (e) { /* the socket is going away; onclose reports it */ }
    }
  }

  subscribe(filter) {
    const id = this.nextId++ & 0xffff || 1;
    this.#send(packet(8, 2, [id >> 8, id & 255, ...str(filter), 0]));
  }

  unsubscribe(filter) {
    const id = this.nextId++ & 0xffff || 1;
    this.#send(packet(10, 2, [id >> 8, id & 255, ...str(filter)]));
  }

  publish(topic, payload, retain = false) {
    if (!goodTopic(topic)) return;
    const p = typeof payload === 'string' ? enc.encode(payload) : payload;
    if (p.length > MAX_PAYLOAD) return;
    this.#send(packet(3, retain ? 1 : 0, [...str(topic), ...p]));
  }

  close() {
    clearInterval(this.ping);
    this.ready = false;
    this.#send(packet(14, 0, [])); // DISCONNECT: a clean goodbye, the will is not sent
    try { this.ws?.close(); } catch (e) { /* closed */ }
  }
}

// ---- the room service ---------------------------------------------------------------------------------------------------
/**
 * One connection to the room service: to every broker that answers, each redialled when it drops. `version` keeps
 * players of different builds apart (they could not play together anyway). `onChange` hears when the first broker
 * comes up, when they are all down, and every reconnection (subscriptions are renewed by themselves).
 */
export class RoomService {
  constructor({ version, brokers }) {
    this.prefix = `fightnight/rooms2/v${version}`;
    this.brokers = brokers && brokers.length ? brokers : DEFAULT_BROKERS;
    this.id = randomId();
    this.socks = new Map(); // url -> MqttSocket (connected)
    this.handlers = new Map(); // topic filter -> fn(topic, msg, retained)
    this.seen = new Map(); // message id -> time (the same message comes once from every broker)
    this.closed = false;
    this.will = null;
    this.timers = new Set();
    this.onChange = null;
  }

  get connected() { return [...this.socks.values()].some((s) => s.ready); }

  /**
   * Connect to every broker. Resolves once one is up; rejects when none answered at first (they are still tried again
   * in the background until stop(): onChange says when one comes up).
   */
  start({ will } = {}) {
    this.will = will || null;
    return new Promise((resolve, reject) => {
      let left = this.brokers.length;
      if (!left) { reject(new Error('no room service configured')); return; }
      for (const url of this.brokers) {
        this.#dial(url, 0, (ok) => {
          if (ok) resolve(this);
          else if (--left === 0 && !this.connected) reject(new Error('Could not reach the room service. Check your internet connection.'));
        });
      }
    });
  }

  #dial(url, attempt, first) {
    if (this.closed) return;
    const sock = new MqttSocket(url, {
      clientId: 'fn-' + randomId(),
      will: this.will,
      onMessage: (topic, payload, retained) => this.#deliver(topic, payload, retained),
      onClose: () => {
        if (this.socks.get(url) === sock) this.socks.delete(url);
        this.onChange?.();
        this.#later(() => this.#dial(url, 0), 1000);
      },
    });
    sock.connect().then(() => {
      if (this.closed) { sock.close(); return; }
      this.socks.set(url, sock);
      for (const filter of this.handlers.keys()) sock.subscribe(filter);
      first?.(true);
      this.onChange?.();
    }).catch(() => {
      first?.(false);
      // try again later, less and less often
      this.#later(() => this.#dial(url, attempt + 1), Math.min(30000, 2000 * 2 ** attempt));
    });
  }

  #later(fn, ms) {
    if (this.closed) return;
    const t = setTimeout(() => { this.timers.delete(t); fn(); }, ms);
    this.timers.add(t);
  }

  #deliver(topic, payload, retained) {
    if (payload.length > MAX_PAYLOAD) return;
    let msg = null;
    if (payload.length) {
      try { msg = JSON.parse(dec.decode(payload)); } catch (e) { return; }
      if (!msg || typeof msg !== 'object') return;
      if (typeof msg.id === 'string') {
        const key = msg.id.slice(0, 40) + (retained ? 'r' : '');
        if (this.seen.has(key)) return;
        this.seen.set(key, Date.now());
        if (this.seen.size > 500) for (const [k, t] of this.seen) if (Date.now() - t > 60000) this.seen.delete(k);
      }
    }
    for (const [filter, fn] of this.handlers) if (matches(filter, topic)) fn(topic, msg, retained);
  }

  on(filter, fn) {
    this.handlers.set(filter, fn);
    for (const s of this.socks.values()) if (s.ready) s.subscribe(filter);
  }

  off(filter) {
    this.handlers.delete(filter);
    for (const s of this.socks.values()) if (s.ready) s.unsubscribe(filter);
  }

  send(topic, obj, retain = false) {
    const body = obj ? JSON.stringify({ id: randomId(), ...obj }) : '';
    for (const s of this.socks.values()) if (s.ready) s.publish(topic, body, retain);
  }

  stop() {
    this.closed = true;
    for (const t of this.timers) clearTimeout(t);
    this.timers.clear();
    for (const s of this.socks.values()) s.close();
    this.socks.clear();
    this.handlers.clear();
  }

  listTopic(listing) { return `${this.prefix}/rooms/${listing}`; }
  hostTopic(roomTopic) { return `${this.prefix}/room/${roomTopic}/host`; }
  guestTopic(roomTopic, guest) { return `${this.prefix}/room/${roomTopic}/to/${guest}`; }
}

function matches(filter, topic) {
  const f = filter.split('/'), t = topic.split('/');
  for (let i = 0; i < f.length; i++) {
    if (f[i] === '#') return true;
    if (i >= t.length || (f[i] !== '+' && f[i] !== t[i])) return false;
  }
  return f.length === t.length;
}

/** A random id for a room's listing: the topic it is announced under says nothing about its code. */
export function newListingId() { return randomId(); }

// ---- hosting a room ---------------------------------------------------------------------------------------------------------
/**
 * The host's side: keeps the room listed (when public) and answers join requests with WebRTC offers.
 * `onJoin(guestId, name)` must return a promise of an invite code (or throw to turn them away);
 * `onAnswer(guestId, reply)` takes the guest's reply. Call `await room.open()` once the service is started.
 */
export class HostedRoom {
  constructor(service, { code, listing, info, onJoin, onAnswer }) {
    this.svc = service;
    this.code = code;
    this.listing = listing;
    this.info = info; // { host, mode, map, players, max }
    this.public = true;
    this.listed = false;
    this.onJoin = onJoin;
    this.onAnswer = onAnswer;
    this.joins = new Map(); // guest id -> { key (shared with that guest), offer (promise of the invite code), answered }
    this.timer = 0;
    this.closed = false;
  }

  async open() {
    this.keys = await roomKeys(this.code);
    this.pair = await keyPair();
    if (this.closed) return;
    this.svc.on(this.svc.hostTopic(this.keys.topic), (topic, msg) => { this.#request(msg).catch((e) => console.warn('room request', e)); });
    const prev = this.svc.onChange;
    // a broker that comes back has forgotten nothing it keeps, but say it again at once anyway
    this.svc.onChange = () => { prev?.(); if (this.svc.connected) this.#announce(); };
    this.#announce();
    this.timer = setInterval(() => this.#announce(), REFRESH_MS);
  }

  setPublic(on) {
    this.public = !!on;
    this.#announce();
  }

  update(info) {
    const before = JSON.stringify(this.info);
    this.info = { ...this.info, ...info };
    if (JSON.stringify(this.info) !== before) this.#announce();
  }

  #announce() {
    if (!this.keys) return;
    const topic = this.svc.listTopic(this.listing);
    if (this.public && !this.closed) {
      this.svc.send(topic, { code: this.code, hpub: this.pair.pub, t: Date.now(), ...this.info }, true);
      this.listed = true;
    } else if (this.listed) {
      // an empty kept message takes the room off the list (a private room that was never listed sends nothing at all)
      this.svc.send(topic, null, true);
      this.listed = false;
    }
  }

  async #request(env) {
    if (!env || this.closed || !this.keys) return;
    const msg = await unseal(this.keys.key, env.box);
    if (!msg || typeof msg.from !== 'string' || !ID_RE.test(msg.from)) return;
    const to = this.svc.guestTopic(this.keys.topic, msg.from);
    const reply = async (j, obj) => this.svc.send(to, { box: await seal(this.keys.key, { hpub: this.pair.pub, data: await seal(j.key, obj) }) });
    if (msg.t === 'join') {
      let j = this.joins.get(msg.from);
      if (!j) {
        let key;
        try { key = await sharedKey(this.pair.priv, msg.gpub); } catch (e) { return; }
        j = { key, gpub: msg.gpub, answered: false };
        j.offer = Promise.resolve().then(() => this.onJoin(msg.from, String(msg.name || 'Player').slice(0, 16))).catch((e) => ({ refuse: e.message || String(e) }));
        this.joins.set(msg.from, j);
      } else if (j.gpub !== msg.gpub) return; // somebody else using that guest's id
      const offer = await j.offer;
      if (!offer || offer.refuse) {
        this.joins.delete(msg.from);
        await reply(j, { t: 'refused', why: (offer && offer.refuse) || 'The room is full.' });
      } else await reply(j, { t: 'offer', code: offer });
    } else if (msg.t === 'answer') {
      const j = this.joins.get(msg.from);
      if (!j || j.answered) return;
      const inner = await unseal(j.key, msg.data);
      if (!inner || typeof inner.code !== 'string') return; // not from the guest who asked (only they share j.key)
      j.answered = true;
      this.onAnswer(msg.from, inner.code);
    }
  }

  /** Forget a guest (they left or never finished joining) so they can join again. */
  forget(guestId) { this.joins.delete(guestId); }

  close() {
    this.closed = true;
    clearInterval(this.timer);
    if (this.keys) this.svc.off(this.svc.hostTopic(this.keys.topic));
    this.#announce();
  }
}

// ---- the room list and joining ---------------------------------------------------------------------------------------------
/** Watch the list of public rooms: `onList(rooms)` gets the open ones whenever it changes. */
export function watchRooms(service, onList) {
  const rooms = new Map(); // listing -> { info, at, first }
  const filter = `${service.prefix}/rooms/+`;
  const emit = () => {
    const now = Date.now();
    for (const [k, r] of rooms) if (now - r.at > STALE_MS) rooms.delete(k);
    const live = [...rooms.values()].sort((a, b) => a.first - b.first).map((r) => r.info);
    onList(live.slice(0, MAX_LISTED));
  };
  service.on(filter, (topic, msg, retained) => {
    const listing = topic.split('/').pop();
    if (!msg) { rooms.delete(listing); emit(); return; }
    // (anyone can publish here: keep only what a room announcement can hold)
    if (!isCode(msg.code) || typeof msg.hpub !== 'string' || msg.hpub.length > 200) return;
    // a kept announcement from long ago is a room whose host never said goodbye; a live one is fresh whatever the
    // clocks of the two machines say
    if (retained && Math.abs(Date.now() - (Number(msg.t) || 0)) > RETAINED_MAX_AGE_MS) { rooms.delete(listing); emit(); return; }
    const info = {
      code: msg.code,
      hpub: msg.hpub,
      host: String(msg.host || 'Player').slice(0, 16),
      mode: String(msg.mode || '').slice(0, 20),
      map: String(msg.map || '').slice(0, 20),
      players: Math.max(1, Math.min(99, Math.floor(Number(msg.players)) || 1)),
      max: Math.max(1, Math.min(99, Math.floor(Number(msg.max)) || 8)),
    };
    const old = rooms.get(listing);
    rooms.set(listing, { info, at: Date.now(), first: old ? old.first : Date.now() });
    emit();
  });
  const timer = setInterval(emit, 2000);
  return () => { clearInterval(timer); service.off(filter); };
}

/**
 * Ask to join the room `code`. `onOffer(inviteCode)` must return a promise of the reply code (net.js Link.acceptInvite).
 * `hpub` is the host's key from the room list, when the room was picked there: then only that host can answer.
 * Resolves when the reply has been sent; rejects if the room does not answer, turns us away, or `signal` aborts.
 */
export async function requestJoin(service, code, name, onOffer, { hpub = null, timeoutMs = 15000, signal = null } = {}) {
  if (!isCode(code)) throw new Error(`A room code has ${CODE_LENGTH} letters and digits.`);
  const keys = await roomKeys(code);
  const mine = await keyPair();
  const myTopic = service.guestTopic(keys.topic, service.id);
  return new Promise((resolve, reject) => {
    let done = false;
    let answered = false;
    let retry = 0, timer = 0;
    const finish = (err) => {
      if (done) return;
      done = true;
      clearInterval(retry);
      clearTimeout(timer);
      service.off(myTopic);
      signal?.removeEventListener('abort', onAbort);
      if (err) reject(err); else resolve();
    };
    const onAbort = () => finish(Object.assign(new Error('cancelled'), { name: 'AbortError' }));
    if (signal?.aborted) { onAbort(); return; }
    signal?.addEventListener('abort', onAbort);
    service.on(myTopic, async (topic, env) => {
      if (!env || done || answered) return;
      const outer = await unseal(keys.key, env.box);
      if (!outer || typeof outer.hpub !== 'string' || (hpub && outer.hpub !== hpub)) return;
      let key;
      try { key = await sharedKey(mine.priv, outer.hpub); } catch (e) { return; }
      const msg = await unseal(key, outer.data);
      if (!msg || done || answered) return;
      if (msg.t === 'refused') finish(new Error(String(msg.why || 'The host turned the request down.').slice(0, 120)));
      else if (msg.t === 'offer' && typeof msg.code === 'string') {
        answered = true;
        clearInterval(retry);
        clearTimeout(timer); // (making the answer takes a few seconds of its own)
        try {
          const reply = await onOffer(msg.code);
          if (done) return;
          const box = await seal(keys.key, { t: 'answer', from: service.id, data: await seal(key, { code: reply }) });
          service.send(service.hostTopic(keys.topic), { box });
          finish();
        } catch (e) { finish(e); }
      }
    });
    const ask = async () => {
      const box = await seal(keys.key, { t: 'join', from: service.id, name: String(name || 'Player').slice(0, 16), gpub: mine.pub });
      if (!done) service.send(service.hostTopic(keys.topic), { box });
    };
    ask();
    // (a message can be lost on the way: ask again until the host answers)
    retry = setInterval(ask, 3000);
    timer = setTimeout(() => finish(new Error('No room with that code answered. Check the code, and that the host is still in the room.')), timeoutMs);
  });
}

/** For the tests (tools/rooms_unit.mjs): what someone who knows a room's code can make. */
export const __test = { roomKeys, seal };
