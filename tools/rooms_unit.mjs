// Room service tests without a browser (Node 22): web/rooms.js against the local broker (mqtt_broker.mjs), including the
// things a stranger on a public broker could try. Fast; exits non-zero on failure.
//   node rooms_unit.mjs
import { startBroker } from './mqtt_broker.mjs';
import { RoomService, HostedRoom, watchRooms, requestJoin, randomCode, newListingId, __test } from '../web/rooms.js';

let failed = 0;
const check = (name, ok, extra = '') => { console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${extra ? '  ' + extra : ''}`); if (!ok) failed++; };
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const until = async (f, ms = 5000) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { if (await f()) return true; await sleep(50); } return false; };

let broker = await startBroker();
const port = Number(new URL(broker.url).port);
const svc = () => new RoomService({ version: 99, brokers: [broker.url] });

// a stranger who reads everything the game sends
const spy = svc();
await spy.start();
const heard = [];
spy.on('fightnight/#', (topic, msg) => heard.push(topic + ' ' + JSON.stringify(msg)));

async function host({ code = randomCode(), pub = true, onJoin } = {}) {
  const s = svc();
  const listing = newListingId();
  const answers = [];
  const room = new HostedRoom(s, { code, listing, info: { host: 'Hosty', mode: 'lego', map: 'island', players: 1, max: 8 },
    onJoin: onJoin || (async (g) => 'FN1.offer-for-' + g), onAnswer: (g, reply) => answers.push(reply) });
  room.public = pub;
  await s.start({ will: { topic: s.listTopic(listing), payload: '', retain: true } });
  await room.open();
  return { s, room, code, listing, answers };
}

try {
  // ---- a public room is listed, with its code and its host's key ----------------------------------------------------------
  const guest = svc();
  await guest.start();
  let list = [];
  const stop = watchRooms(guest, (r) => { list = r; });
  const pubRoom = await host();
  check('a public room is listed', await until(() => list.some((r) => r.code === pubRoom.code)), JSON.stringify(list.map((r) => r.code)));
  const entry = list.find((r) => r.code === pubRoom.code);
  check('the listing names the host and carries their key', !!entry && entry.host === 'Hosty' && typeof entry.hpub === 'string' && entry.hpub.length > 40);

  // ---- joining from the list, pinned to that host ---------------------------------------------------------------------------
  let offer = null;
  await requestJoin(guest, pubRoom.code, 'Gus', async (o) => { offer = o; return 'FN1.reply'; }, { hpub: entry.hpub });
  check('the guest gets the host\'s offer', offer === 'FN1.offer-for-' + guest.id, offer);
  check('the host gets the guest\'s answer', await until(() => pubRoom.answers.includes('FN1.reply')), JSON.stringify(pubRoom.answers));

  // ---- a private room never shows its code on the wire ------------------------------------------------------------------------
  heard.length = 0;
  const priv = await host({ pub: false });
  const g2 = svc();
  await g2.start();
  let offer2 = null;
  await requestJoin(g2, priv.code, 'Pat', async (o) => { offer2 = o; return 'FN1.reply2'; });
  check('a private room can be joined by its code', offer2 === 'FN1.offer-for-' + g2.id && await until(() => priv.answers.includes('FN1.reply2')));
  await sleep(9000); // (past a refresh)
  check('a private room is never listed', !list.some((r) => r.code === priv.code));
  const leaks = heard.filter((h) => h.includes(priv.code));
  check('nothing on the wire contains a private room\'s code', leaks.length === 0, leaks.slice(0, 2).join(' | '));
  check('nobody can read the offers and answers', !heard.some((h) => h.includes('FN1.')), `${heard.length} messages heard`);
  check('a public room\'s offers and answers are sealed too', !heard.some((h) => h.includes('offer-for')));

  // ---- strangers cannot break or hijack a room ------------------------------------------------------------------------------
  const before = pubRoom.s.connected;
  for (const from of ['x/#', '+', 'a\u0000b', 'é'.repeat(40000)]) spy.send(pubRoom.s.hostTopic('whatever'), { box: 'garbage', from });
  // (a stranger who knows the public code still cannot make the host publish to a topic of their choosing)
  const strangerKeysJoin = svc();
  await strangerKeysJoin.start();
  const k = await __test.roomKeys(pubRoom.code);
  for (const from of ['x/#', '+', 'a\u0000b', '../host']) {
    strangerKeysJoin.send(strangerKeysJoin.hostTopic(k.topic), { box: await __test.seal(k.key, { t: 'join', from, name: 'Eve', gpub: entry.hpub }) });
  }
  await sleep(800);
  check('forged requests do not get the host thrown off the broker', before && pubRoom.s.connected && broker.protocolErrors === 0, `${broker.protocolErrors} protocol errors`);
  // a stranger who knows the public code answers a joining guest in the host's place
  const victim = svc();
  await victim.start();
  const impostor = await host({ code: pubRoom.code, onJoin: async () => { throw new Error('The room is full.'); } });
  let got = null;
  try { await requestJoin(victim, pubRoom.code, 'Vic', async (o) => { got = o; return 'FN1.r'; }, { hpub: entry.hpub }); } catch (e) { got = 'refused: ' + e.message; }
  check('with the host\'s key from the list, only the real host can answer', got === 'FN1.offer-for-' + victim.id, got);
  impostor.room.close(); impostor.s.stop();

  // ---- wrong codes and timeouts ---------------------------------------------------------------------------------------------
  let err = '';
  try { await requestJoin(guest, 'ZZZZZZ', 'Gus', async () => 'x', { timeoutMs: 1500 }); } catch (e) { err = e.message; }
  check('a code no room has is turned away', /No room with that code/.test(err), err);

  // ---- clocks: a live room shows whatever the two clocks say; a long-gone kept one does not ---------------------------------
  const skewed = newListingId();
  spy.send(spy.listTopic(skewed), { code: 'SKEW22', hpub: entry.hpub, host: 'Late', t: Date.now() - 90000, players: 1, max: 8 });
  check('a live room whose host\'s clock is 90 s behind is listed', await until(() => list.some((r) => r.code === 'SKEW22')));
  const gone = newListingId();
  spy.send(spy.listTopic(gone), { code: 'GONE22', hpub: entry.hpub, host: 'Gone', t: Date.now() - 3600000, players: 1, max: 8 }, true);
  const late = svc();
  await late.start();
  let list2 = [];
  const stop2 = watchRooms(late, (r) => { list2 = r; });
  await sleep(800);
  check('a kept announcement from an hour ago is not listed', !list2.some((r) => r.code === 'GONE22') && list2.some((r) => r.code === pubRoom.code));
  stop2(); late.stop();

  // ---- the host leaves without a goodbye: the broker unlists the room ---------------------------------------------------------
  const crash = await host();
  check('a room is listed before its host goes away', await until(() => list.some((r) => r.code === crash.code)));
  for (const s of crash.s.socks.values()) s.ws.close(); // an abrupt close, as when the page dies
  crash.s.closed = true; // (and it does not dial back)
  check('the room leaves the list when the host goes away', await until(() => !list.some((r) => r.code === crash.code)));

  // ---- a broker restart: everybody dials back and the room is listed again ----------------------------------------------------
  await broker.close();
  check('everybody notices the broker going away', await until(() => !pubRoom.s.connected && !guest.connected, 5000));
  broker = await startBroker({ port });
  check('the host and the guest dial back by themselves', await until(() => pubRoom.s.connected && guest.connected, 15000));
  check('the room is listed again after the broker restart', await until(() => list.some((r) => r.code === pubRoom.code), 12000));
  let offer3 = null;
  const g3 = svc();
  await g3.start();
  await requestJoin(g3, pubRoom.code, 'Rae', async (o) => { offer3 = o; return 'FN1.reply3'; });
  check('joining works again after the restart', offer3 === 'FN1.offer-for-' + g3.id);

  stop();
  for (const s of [guest, g2, g3, victim, strangerKeysJoin, spy]) s.stop();
  for (const h of [pubRoom, priv]) { h.room.close(); h.s.stop(); }
} catch (e) {
  console.error(e);
  failed++;
}
await broker.close();
console.log(failed ? `\n${failed} check(s) failed` : '\nall checks passed');
process.exit(failed ? 1 : 0);
