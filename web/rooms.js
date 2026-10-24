// Rooms: how players find each other without a game server.
//
// The page talks to free public MQTT brokers over WebSockets (several at once, so one being down does not matter):
// - a public room is a retained message under `<prefix>/rooms/<code>` that the host refreshes while the room is open
//   (and that the broker clears when the host goes away), so everybody who opens "Join a game" sees the list;
// - a private room is not listed; its code is the only way in;
// - joining sends a request to `<prefix>/room/<code>/host`; the host answers on `<prefix>/room/<code>/to/<guest>` with a
//   WebRTC offer, the guest sends its answer back the same way, and from then on the two talk directly (net.js).
// Nobody copies or pastes anything: the room code (6 letters) is all a friend needs.

const DEFAULT_BROKERS = ['wss://broker.emqx.io:8084/mqtt', 'wss://broker.hivemq.com:8884/mqtt', 'wss://test.mosquitto.org:8081/mqtt'];
const CODE_ALPHABET = 'ABCDEFGHJKLMNPQRSTUVWXYZ23456789'; // no 0/O or 1/I to misread
export const CODE_LENGTH = 6;
const REFRESH_MS = 8000; // a listed room is announced again this often
const STALE_MS = 25000; // and drops off the list when it has not been for this long
const KEEPALIVE_S = 30;
const MAX_LISTED = 40;

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

function randomId() {
  return [...crypto.getRandomValues(new Uint8Array(8))].map((b) => b.toString(16).padStart(2, '0')).join('');
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
  return [b.length >> 8, b.length & 255, ...b];
}
function packet(type, flags, body) {
  return new Uint8Array([(type << 4) | flags, ...varint(body.length), ...body]);
}

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
            this.ping = setInterval(() => this.#send(packet(12, 0, [])), (KEEPALIVE_S * 1000) / 2);
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
    const p = typeof payload === 'string' ? enc.encode(payload) : payload;
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
 * One connection to the room service (to every broker that answers). `version` keeps players of different builds apart:
 * they would not be able to play together anyway.
 */
export class RoomService {
  constructor({ version, brokers }) {
    this.prefix = `fightnight/rooms1/v${version}`;
    this.brokers = brokers && brokers.length ? brokers : DEFAULT_BROKERS;
    this.id = randomId();
    this.socks = [];
    this.handlers = new Map(); // topic filter -> fn(topic, payload, retained)
    this.seen = new Map(); // message id -> time (the same message comes once from every broker)
    this.closed = false;
    this.will = null;
  }

  get connected() { return this.socks.some((s) => s.ready); }

  /** Connect to every broker; resolves once at least one is up (rejects when none is). */
  start({ will } = {}) {
    this.will = will || null;
    const tries = this.brokers.map((url) => {
      const sock = new MqttSocket(url, {
        clientId: 'fn-' + randomId(),
        will: this.will,
        onMessage: (topic, payload, retained) => this.#deliver(topic, payload, retained),
        onClose: () => { this.socks = this.socks.filter((s) => s !== sock); this.onChange?.(); },
      });
      return sock.connect().then(() => {
        if (this.closed) { sock.close(); return; }
        this.socks.push(sock);
        for (const filter of this.handlers.keys()) sock.subscribe(filter);
        this.onChange?.();
      });
    });
    return new Promise((resolve, reject) => {
      let left = tries.length;
      if (!left) reject(new Error('no room service configured'));
      for (const t of tries) {
        t.then(() => resolve(this)).catch(() => { if (--left === 0 && !this.connected) reject(new Error('Could not reach the room service. Check your internet connection.')); });
      }
    });
  }

  #deliver(topic, payload, retained) {
    let msg = null;
    if (payload.length) {
      try { msg = JSON.parse(dec.decode(payload)); } catch (e) { return; }
      if (msg && msg.id) {
        const key = msg.id + (retained ? 'r' : '');
        if (this.seen.has(key)) return;
        this.seen.set(key, Date.now());
        if (this.seen.size > 500) for (const [k, t] of this.seen) if (Date.now() - t > 60000) this.seen.delete(k);
      }
    }
    for (const [filter, fn] of this.handlers) if (matches(filter, topic)) fn(topic, msg, retained);
  }

  on(filter, fn) {
    this.handlers.set(filter, fn);
    for (const s of this.socks) if (s.ready) s.subscribe(filter);
  }

  off(filter) {
    this.handlers.delete(filter);
    for (const s of this.socks) if (s.ready) s.unsubscribe(filter);
  }

  send(topic, obj, retain = false) {
    const body = obj ? JSON.stringify({ id: randomId(), ...obj }) : '';
    for (const s of this.socks) if (s.ready) s.publish(topic, body, retain);
  }

  stop() {
    this.closed = true;
    for (const s of this.socks) s.close();
    this.socks = [];
    this.handlers.clear();
  }

  roomTopic(code) { return `${this.prefix}/rooms/${code}`; }
  hostTopic(code) { return `${this.prefix}/room/${code}/host`; }
  guestTopic(code, guest) { return `${this.prefix}/room/${code}/to/${guest}`; }
}

function matches(filter, topic) {
  const f = filter.split('/'), t = topic.split('/');
  for (let i = 0; i < f.length; i++) {
    if (f[i] === '#') return true;
    if (i >= t.length || (f[i] !== '+' && f[i] !== t[i])) return false;
  }
  return f.length === t.length;
}

// ---- hosting a room ---------------------------------------------------------------------------------------------------------
/**
 * The host's side: keeps the room listed (when public) and answers join requests with WebRTC offers.
 * `onJoin(guestId, name)` must return a promise of an invite code (or null to turn them away);
 * `onAnswer(guestId, reply)` takes the guest's reply.
 */
export class HostedRoom {
  constructor(service, { code, info, onJoin, onAnswer }) {
    this.svc = service;
    this.code = code;
    this.info = info; // { host, mode, map, players, max }
    this.public = true;
    this.onJoin = onJoin;
    this.onAnswer = onAnswer;
    this.offers = new Map(); // guest id -> promise of the invite code
    this.timer = 0;
  }

  open() {
    this.svc.on(this.svc.hostTopic(this.code), (topic, msg) => this.#request(msg));
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
    const topic = this.svc.roomTopic(this.code);
    if (this.public && !this.closed) this.svc.send(topic, { code: this.code, t: Date.now(), ...this.info }, true);
    else this.svc.send(topic, null, true); // an empty retained message takes the room off the list
  }

  async #request(msg) {
    if (!msg || !msg.from || this.closed) return;
    const to = this.svc.guestTopic(this.code, msg.from);
    if (msg.t === 'join') {
      if (!this.offers.has(msg.from)) {
        this.offers.set(msg.from, Promise.resolve(this.onJoin(msg.from, String(msg.name || 'Player').slice(0, 16))).catch((e) => ({ refuse: e.message || String(e) })));
      }
      const offer = await this.offers.get(msg.from);
      if (!offer || offer.refuse) {
        this.offers.delete(msg.from);
        this.svc.send(to, { t: 'refused', why: (offer && offer.refuse) || 'The room is full.' });
      } else this.svc.send(to, { t: 'offer', code: offer });
    } else if (msg.t === 'answer' && this.offers.has(msg.from)) {
      this.onAnswer(msg.from, msg.code);
    }
  }

  /** Forget a guest (they left or never finished joining) so they can join again. */
  forget(guestId) { this.offers.delete(guestId); }

  close() {
    this.closed = true;
    clearInterval(this.timer);
    this.svc.off(this.svc.hostTopic(this.code));
    this.svc.send(this.svc.roomTopic(this.code), null, true);
  }
}

// ---- the room list and joining ---------------------------------------------------------------------------------------------
/** Watch the list of public rooms: `onList(rooms)` gets the open ones, newest first, whenever it changes. */
export function watchRooms(service, onList) {
  const rooms = new Map(); // code -> { info, at }
  const filter = `${service.prefix}/rooms/+`;
  const emit = () => {
    const now = Date.now();
    const live = [...rooms.values()].filter((r) => now - r.at < STALE_MS).map((r) => r.info);
    live.sort((a, b) => (b.t || 0) - (a.t || 0));
    onList(live.slice(0, MAX_LISTED));
  };
  service.on(filter, (topic, msg) => {
    const code = topic.split('/').pop();
    if (!msg) rooms.delete(code);
    else {
      // an announcement kept by the broker from long ago is a room that is gone (its host never said goodbye)
      // (anyone can publish here: keep only what a room announcement can hold)
      const age = Math.abs(Date.now() - (Number(msg.t) || 0));
      const info = {
        code,
        t: Number(msg.t) || 0,
        host: String(msg.host || 'Player').slice(0, 16),
        mode: String(msg.mode || ''),
        map: String(msg.map || ''),
        players: Math.max(1, Math.min(99, Math.floor(Number(msg.players)) || 1)),
        max: Math.max(1, Math.min(99, Math.floor(Number(msg.max)) || 8)),
      };
      if (age < STALE_MS && cleanCode(code) === code && code.length === CODE_LENGTH) rooms.set(code, { info, at: Date.now() - Math.min(age, STALE_MS) });
      else rooms.delete(code);
    }
    emit();
  });
  const timer = setInterval(emit, 2000);
  return () => { clearInterval(timer); service.off(filter); };
}

/**
 * Ask to join the room `code`. `onOffer(inviteCode)` must return a promise of the reply code (net.js Link.acceptInvite).
 * Resolves when the reply has been sent; rejects if the room does not answer or turns us away.
 */
export function requestJoin(service, code, name, onOffer, timeoutMs = 12000) {
  return new Promise((resolve, reject) => {
    const mine = service.guestTopic(code, service.id);
    let done = false;
    let answered = false;
    const finish = (err) => {
      if (done) return;
      done = true;
      clearInterval(retry);
      clearTimeout(timer);
      if (err) { service.off(mine); reject(err); } else resolve();
    };
    service.on(mine, async (topic, msg) => {
      if (!msg || done) return;
      if (msg.t === 'refused') finish(new Error(msg.why || 'The host turned the request down.'));
      else if (msg.t === 'offer' && !answered) {
        answered = true;
        clearInterval(retry);
        try {
          const reply = await onOffer(msg.code);
          service.send(service.hostTopic(code), { t: 'answer', from: service.id, code: reply });
          finish();
        } catch (e) { finish(e); }
      }
    });
    const ask = () => service.send(service.hostTopic(code), { t: 'join', from: service.id, name });
    ask();
    // (a message can be lost on the way: ask again until the host answers)
    const retry = setInterval(ask, 3000);
    const timer = setTimeout(() => finish(new Error('No room with that code answered. Check the code, and that the host is still in the room.')), timeoutMs);
  });
}
