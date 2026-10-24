// Rooms end-to-end test: two browsers find each other through the room service with no codes copied back and forth.
// The host opens a room; the guest sees it in the list; the host makes it private and it leaves the list; the guest
// joins by typing its code, leaves, and joins again from the list; then they start a match together.
// A local MQTT broker (mqtt_broker.mjs) stands in for the public ones, so the test needs no internet.
//   node rooms_e2e.mjs            (needs `scripts/build.sh` first)
import { serve, launch } from './browser.mjs';
import { startBroker } from './mqtt_broker.mjs';

const broker = await startBroker();
const { srv, url } = await serve();
let failed = 0;
const check = (name, ok, extra = '') => { console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${extra ? '  ' + extra : ''}`); if (!ok) failed++; };

const A = await launch({ width: 480, height: 270 }); // the host
const B = await launch({ width: 480, height: 270 }); // the guest
const pages = { host: A.page, guest: B.page };
const logs = { host: A.logs, guest: B.logs };
const net = async (who) => JSON.parse(await pages[who].evaluate(() => window.__game.fn.net_status()));
const chips = (who) => pages[who].evaluate(() => [...document.querySelectorAll('#lobby-players .chip')].map((c) => c.textContent));
const rows = () => pages.guest.evaluate(() => [...document.querySelectorAll('#room-list .room-row')].map((r) => ({ code: r.dataset.code, text: r.textContent })));
const settles = async (who, f, arg, timeout = 30000) => { try { await pages[who].waitForFunction(f, arg, { timeout }); return true; } catch (e) { return false; } };

try {
  for (const who of ['host', 'guest']) {
    await pages[who].goto(url + 'index.html?nolock=1&quality=low&rooms=' + encodeURIComponent(broker.url));
    await pages[who].waitForFunction(() => window.__game && window.__game.state === 'menu', null, { timeout: 240000 });
  }
  check('both pages reach the menu', true);

  // ---- the host opens a room: it gets a code and is listed --------------------------------------------------------------
  await pages.host.evaluate(() => { window.__game.cfg.name = 'Hosty'; window.__game.cfg.bots = 4; window.__game.cfg.skipbus = true; });
  await pages.host.click('#seg-map button[data-v="island"]');
  await pages.host.click('#btn-multi');
  await pages.host.click('#btn-mp-host');
  check('the host gets a six-letter room code', await settles('host', () => /^[A-Z2-9]{6}$/.test(document.getElementById('room-code').textContent)));
  const code = await pages.host.textContent('#room-code');
  check('a new room is public', await pages.host.getAttribute('#lb-visibility button[data-v="public"]', 'aria-pressed') === 'true');
  check('no invite code has to be copied to open the room', !(await pages.host.isVisible('#invites .inv')));

  await pages.guest.evaluate(() => { window.__game.cfg.name = 'Gus'; });
  await pages.guest.click('#btn-multi');
  await pages.guest.click('#btn-mp-join');
  const listed = await settles('guest', (c) => !!document.querySelector(`#room-list .room-row[data-code="${c}"]`), code);
  const r0 = (await rows()).find((r) => r.code === code);
  check('the guest sees the host\'s room in the list', listed && !!r0, JSON.stringify(await rows()));
  check('the list says whose room it is, what it plays and how full it is', !!r0 && r0.text.includes('Hosty') && r0.text.includes('Original island') && r0.text.includes('1/8'), r0 && r0.text);

  // ---- private rooms leave the list; a wrong code is turned away; the right one gets in ------------------------------------
  await pages.host.click('#lb-visibility button[data-v="private"]');
  check('a private room leaves the list', await settles('guest', (c) => !document.querySelector(`#room-list .room-row[data-code="${c}"]`), code, 15000));
  await pages.guest.fill('#join-code', 'zzzzzz');
  await pages.guest.click('#btn-join-code');
  check('a code no room has is turned away with a message', await settles('guest', () => /No room with that code/.test(document.getElementById('lobby-msg').textContent), null, 30000));
  await pages.guest.fill('#join-code', ' ' + code.toLowerCase().slice(0, 3) + ' ' + code.toLowerCase().slice(3) + ' ');
  check('the code field tidies what is typed', (await pages.guest.inputValue('#join-code')) === code, await pages.guest.inputValue('#join-code'));
  await pages.guest.click('#btn-join-code');
  const inByCode = await settles('guest', () => document.querySelectorAll('#lobby-players .chip').length === 2, null, 60000);
  check('typing the private code joins the room', inByCode, (await chips('guest')).join(','));
  check('the host sees the guest arrive by itself', await settles('host', () => document.querySelectorAll('#lobby-players .chip').length === 2, null, 30000), (await chips('host')).join(','));

  // ---- the guest leaves and comes back from the list ----------------------------------------------------------------------
  await pages.guest.click('#btn-lobby-leave');
  check('the host sees the guest leave', await settles('host', () => document.querySelectorAll('#lobby-players .chip').length === 1, null, 30000), (await chips('host')).join(','));
  await pages.host.click('#lb-visibility button[data-v="public"]');
  await pages.guest.click('#btn-mp-join');
  check('the room is listed again once it is public', await settles('guest', (c) => !!document.querySelector(`#room-list .room-row[data-code="${c}"] button.join:not([disabled])`), code, 20000));
  await pages.guest.click(`#room-list .room-row[data-code="${code}"] button.join`);
  check('one click on Join gets the guest back in', await settles('guest', () => document.querySelectorAll('#lobby-players .chip').length === 2, null, 60000), (await chips('guest')).join(','));
  await settles('host', () => document.querySelectorAll('#lobby-players .chip').length === 2, null, 30000);
  check('both lobbies list both players', (await chips('host')).join(',') === 'Hosty,Gus' && (await chips('guest')).join(',') === 'Hosty,Gus', `${(await chips('host')).join(',')} / ${(await chips('guest')).join(',')}`);
  check('the listing counts the players', await settles('host', () => true) && broker.retained.size === 1 && JSON.parse([...broker.retained.values()][0].toString()).players === 2);

  // ---- they play ------------------------------------------------------------------------------------------------------------
  await pages.host.click('#btn-lobby-start');
  for (const who of ['host', 'guest']) {
    try { await pages[who].waitForFunction(() => window.__game.state === 'playing', null, { timeout: 300000 }); } catch (e) { /* reported below */ }
    check(`${who} is playing`, (await pages[who].evaluate(() => window.__game.state)) === 'playing');
  }
  const hs = await net('host'), gs = await net('guest');
  check('the match started with both players', hs.state === 'host' && hs.started && hs.names.length === 2, JSON.stringify(hs));
  check('the guest is in sync with the host', gs.state === 'guest' && !gs.closed, JSON.stringify(gs));
  check('a started match is no longer listed', await settles('host', () => true) && broker.retained.size === 0, `${broker.retained.size} listed`);

  for (const who of ['host', 'guest']) {
    const bad = logs[who].filter((l) => /error|panick|unreachable/i.test(l) && !/WebGPU is experimental|favicon|404|Failed to load resource|\bICE\b|\bstun\b/i.test(l));
    check(`no console errors on the ${who}`, bad.length === 0, bad.slice(0, 3).join(' | '));
  }
} catch (e) {
  console.error(e);
  failed++;
  for (const who of ['host', 'guest']) console.log(`--- ${who} log\n` + logs[who].slice(-15).join('\n'));
}
console.log(failed ? `\n${failed} check(s) failed` : '\nall checks passed');
await A.browser.close(); await B.browser.close(); srv.close(); await broker.close();
process.exit(failed ? 1 : 0);
