// A tiny MQTT 3.1.1 broker over WebSockets for the browser tests: what the public brokers the game uses for its room
// list do (QoS 0, retained messages, last wills, + and # wildcards), on 127.0.0.1, so the tests need no internet.
//   import { startBroker } from './mqtt_broker.mjs'; const b = await startBroker(); ... b.url ... await b.close();
import http from 'node:http';
import crypto from 'node:crypto';

function varint(n) {
  const out = [];
  do { let b = n % 128; n = Math.floor(n / 128); if (n > 0) b |= 0x80; out.push(b); } while (n > 0);
  return out;
}
const packet = (type, flags, body) => Buffer.from([(type << 4) | flags, ...varint(body.length), ...body]);
const strBytes = (s) => { const b = Buffer.from(s, 'utf8'); return [b.length >> 8, b.length & 255, ...b]; };
function matches(filter, topic) {
  const f = filter.split('/'), t = topic.split('/');
  for (let i = 0; i < f.length; i++) {
    if (f[i] === '#') return true;
    if (i >= t.length || (f[i] !== '+' && f[i] !== t[i])) return false;
  }
  return f.length === t.length;
}

export function startBroker({ port = 0, log = false } = {}) {
  const clients = new Set();
  const sockets = new Set();
  const retained = new Map(); // topic -> payload Buffer
  let published = 0;

  function deliver(topic, payload, retain) {
    published++;
    if (retain) { if (payload.length) retained.set(topic, payload); else retained.delete(topic); }
    for (const c of clients) {
      if ([...c.subs].some((f) => matches(f, topic))) c.send(packet(3, 0, [...strBytes(topic), ...payload]));
    }
  }

  function handle(c, type, flags, body) {
    if (type === 1) { // CONNECT
      let at = 0;
      const rd = () => { const n = (body[at] << 8) | body[at + 1]; const s = body.subarray(at + 2, at + 2 + n); at += 2 + n; return s; };
      rd(); // protocol name
      at += 1; // level
      const cf = body[at]; at += 1;
      at += 2; // keepalive
      c.id = rd().toString('utf8');
      if (cf & 0x04) c.will = { topic: rd().toString('utf8'), payload: Buffer.from(rd()), retain: !!(cf & 0x20) };
      c.send(packet(2, 0, [0, 0]));
    } else if (type === 3) { // PUBLISH
      const n = (body[0] << 8) | body[1];
      const topic = body.subarray(2, 2 + n).toString('utf8');
      let at = 2 + n;
      if ((flags >> 1) & 3) at += 2;
      deliver(topic, Buffer.from(body.subarray(at)), !!(flags & 1));
    } else if (type === 8) { // SUBSCRIBE
      const id = [body[0], body[1]];
      let at = 2;
      const codes = [];
      const added = [];
      while (at < body.length) {
        const n = (body[at] << 8) | body[at + 1];
        const f = body.subarray(at + 2, at + 2 + n).toString('utf8');
        at += 3 + n;
        c.subs.add(f); added.push(f); codes.push(0);
      }
      c.send(packet(9, 0, [...id, ...codes]));
      for (const [topic, payload] of retained) if (added.some((f) => matches(f, topic))) c.send(packet(3, 1, [...strBytes(topic), ...payload]));
    } else if (type === 10) { // UNSUBSCRIBE
      let at = 2;
      while (at < body.length) {
        const n = (body[at] << 8) | body[at + 1];
        c.subs.delete(body.subarray(at + 2, at + 2 + n).toString('utf8'));
        at += 2 + n;
      }
      c.send(packet(11, 0, [body[0], body[1]]));
    } else if (type === 12) { // PINGREQ
      c.send(packet(13, 0, []));
    } else if (type === 14) { // DISCONNECT: a clean goodbye, no will
      c.will = null;
      c.end();
    }
  }

  const server = http.createServer((req, res) => { res.writeHead(426); res.end(); });
  server.on('upgrade', (req, socket) => {
    const key = req.headers['sec-websocket-key'];
    const accept = crypto.createHash('sha1').update(key + '258EAFA5-E914-47DA-95CA-C5AB0DC85B11').digest('base64');
    const proto = String(req.headers['sec-websocket-protocol'] || '').split(',').map((s) => s.trim()).includes('mqtt') ? 'Sec-WebSocket-Protocol: mqtt\r\n' : '';
    socket.write(`HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: ${accept}\r\n${proto}\r\n`);
    socket.setNoDelay(true);
    const c = {
      subs: new Set(), will: null, id: '', closed: false,
      send(buf) {
        if (c.closed) return;
        const n = buf.length;
        const head = n < 126 ? Buffer.from([0x82, n]) : n < 65536 ? Buffer.from([0x82, 126, n >> 8, n & 255]) : Buffer.concat([Buffer.from([0x82, 127, 0, 0, 0, 0]), Buffer.from([(n >>> 24) & 255, (n >>> 16) & 255, (n >>> 8) & 255, n & 255])]);
        socket.write(Buffer.concat([head, buf]));
      },
      end() { if (!c.closed) { try { socket.write(Buffer.from([0x88, 0])); } catch (e) { /* gone */ } socket.end(); } },
    };
    clients.add(c);
    sockets.add(socket);
    socket.on('close', () => sockets.delete(socket));
    let wsBuf = Buffer.alloc(0); // websocket frames
    let mq = Buffer.alloc(0); // the MQTT byte stream inside them
    const gone = () => {
      if (c.closed) return;
      c.closed = true;
      clients.delete(c);
      if (c.will) deliver(c.will.topic, c.will.payload, c.will.retain);
      if (log) console.log(`[broker] ${c.id} left`);
    };
    socket.on('close', gone);
    socket.on('error', gone);
    socket.on('data', (chunk) => {
      wsBuf = Buffer.concat([wsBuf, chunk]);
      for (;;) {
        if (wsBuf.length < 2) break;
        const op = wsBuf[0] & 15, masked = wsBuf[1] & 128;
        let len = wsBuf[1] & 127, at = 2;
        if (len === 126) { if (wsBuf.length < 4) break; len = wsBuf.readUInt16BE(2); at = 4; } else if (len === 127) { if (wsBuf.length < 10) break; len = Number(wsBuf.readBigUInt64BE(2)); at = 10; }
        const mask = masked ? wsBuf.subarray(at, at + 4) : null;
        if (masked) at += 4;
        if (wsBuf.length < at + len) break;
        const data = Buffer.from(wsBuf.subarray(at, at + len));
        if (mask) for (let i = 0; i < data.length; i++) data[i] ^= mask[i & 3];
        wsBuf = wsBuf.subarray(at + len);
        if (op === 8) { c.end(); gone(); return; }
        if (op === 9) { socket.write(Buffer.concat([Buffer.from([0x8a, data.length]), data])); continue; }
        if (op === 0 || op === 1 || op === 2) mq = Buffer.concat([mq, data]);
      }
      for (;;) {
        if (mq.length < 2) break;
        let len = 0, mul = 1, i = 1, ok = false;
        while (i < mq.length && i < 5) { const b = mq[i++]; len += (b & 127) * mul; mul *= 128; if (!(b & 128)) { ok = true; break; } }
        if (!ok || mq.length < i + len) break;
        const head = mq[0];
        const body = mq.subarray(i, i + len);
        mq = mq.subarray(i + len);
        try { handle(c, head >> 4, head & 15, body); } catch (e) { if (log) console.log('[broker] bad packet', e); }
      }
    });
  });
  return new Promise((resolve) => {
    server.listen(port, '127.0.0.1', () => {
      const p = server.address().port;
      resolve({
        url: `ws://127.0.0.1:${p}/mqtt`,
        retained,
        get published() { return published; },
        get clients() { return clients.size; },
        close: () => new Promise((r) => { for (const sock of sockets) sock.destroy(); server.close(() => r()); }),
      });
    });
  });
}
