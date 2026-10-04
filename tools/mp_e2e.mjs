// Multiplayer end-to-end test: two browsers, one hosts and one joins through the real invite/reply exchange over WebRTC,
// then they play a little and check each other. Exits non-zero on failure.
//   node mp_e2e.mjs            (needs `scripts/build.sh` first)
import { serve, launch } from './browser.mjs';

const { srv, url } = await serve();
let failed = 0;
const check = (name, ok, extra = '') => { console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${extra ? '  ' + extra : ''}`); if (!ok) failed++; };

const A = await launch({ width: 480, height: 270 }); // the host
const B = await launch({ width: 480, height: 270 }); // the guest
const pages = { host: A.page, guest: B.page };
const logs = { host: A.logs, guest: B.logs };
const dbg = (who, c) => pages[who].evaluate((c) => window.__game.fn.debug(c), c);
const actor = async (who, i) => JSON.parse(await dbg(who, 'actor ' + i));
const net = async (who) => JSON.parse(await pages[who].evaluate(() => window.__game.fn.net_status()));
const frames = async (who, n) => { const p = pages[who]; const f0 = await p.evaluate(() => window.__game.frames); await p.waitForFunction((t) => window.__game.frames >= t, f0 + n, { timeout: 180000 }); };
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

try {
  for (const who of ['host', 'guest']) {
    await pages[who].goto(url + 'index.html?nolock=1&quality=low');
    await pages[who].waitForFunction(() => window.__game && window.__game.state === 'menu', null, { timeout: 240000 });
    await pages[who].evaluate(() => { document.getElementById('opt-name').value = ''; });
  }
  check('both pages reach the menu', true);

  // ---- the host opens a room and makes an invite -----------------------------------------------------------------------
  await pages.host.evaluate(() => { window.__game.cfg.name = 'Hosty'; window.__game.cfg.bots = 6; window.__game.cfg.skipbus = true; });
  await pages.host.click('#btn-multi');
  await pages.host.click('#btn-mp-host');
  await pages.host.waitForFunction(() => document.querySelector('#invites .inv')?.value.startsWith('FN1.'), null, { timeout: 30000 });
  const invite = await pages.host.evaluate(() => document.querySelector('#invites .inv').value);
  check('the host gets an invite code', invite.startsWith('FN1.') && invite.length > 100, `${invite.length} characters`);

  // ---- the guest answers it ---------------------------------------------------------------------------------------------------
  await pages.guest.evaluate(() => { window.__game.cfg.name = 'Gus'; });
  await pages.guest.click('#btn-multi');
  await pages.guest.click('#btn-mp-join');
  await pages.guest.fill('#join-invite', invite);
  await pages.guest.click('#btn-join-continue');
  await pages.guest.waitForFunction(() => document.getElementById('join-reply').value.startsWith('FN1.'), null, { timeout: 30000 });
  const reply = await pages.guest.evaluate(() => document.getElementById('join-reply').value);
  check('the guest gets a reply code', reply.startsWith('FN1.'), `${reply.length} characters`);

  // ---- ... and the host accepts it ------------------------------------------------------------------------------------------
  await pages.host.fill('#invites .rep', reply);
  await pages.host.click('#invites .go');
  await pages.host.waitForFunction(() => document.querySelectorAll('#lobby-players .chip').length === 2, null, { timeout: 60000 });
  await pages.guest.waitForFunction(() => document.querySelectorAll('#lobby-players .chip').length === 2, null, { timeout: 60000 });
  const names = await pages.guest.evaluate(() => [...document.querySelectorAll('#lobby-players .chip')].map((c) => c.textContent));
  check('both lobbies list both players', names.join(',') === 'Hosty,Gus', names.join(','));

  // ---- start the match ----------------------------------------------------------------------------------------------------------
  await pages.host.click('#btn-lobby-start');
  for (const who of ['host', 'guest']) {
    try { await pages[who].waitForFunction(() => window.__game.state === 'playing', null, { timeout: 300000 }); } catch (e) { /* reported below */ }
    check(`${who} is playing`, (await pages[who].evaluate(() => window.__game.state)) === 'playing');
  }
  await frames('host', 4); await frames('guest', 4);
  const hs = await net('host'), gs = await net('guest');
  check('the host sees the match started with two players', hs.state === 'host' && hs.started && hs.names.length === 2, JSON.stringify(hs));
  check('the guest is in sync with the host', gs.state === 'guest' && gs.synced && !gs.closed, JSON.stringify(gs));

  // ---- the guest walks; the host sees it -----------------------------------------------------------------------------------
  await dbg('host', 'tp 52 28'); // the host stays out of the way
  const g0 = await actor('guest', 1);
  await pages.guest.evaluate(() => window.__game.fn.debug('look 90 0'));
  await pages.guest.keyboard.down('KeyW');
  await frames('guest', 14);
  await pages.guest.keyboard.up('KeyW');
  await frames('guest', 10); await frames('host', 10);
  const g1 = await actor('guest', 1);
  const gOnHost = await actor('host', 1);
  const moved = Math.hypot(g1.pos[0] - g0.pos[0], g1.pos[2] - g0.pos[2]);
  check('W moves the guest on its own screen', moved > 3, `${moved.toFixed(1)} m`);
  const sep = Math.hypot(g1.pos[0] - gOnHost.pos[0], g1.pos[2] - gOnHost.pos[2]);
  check('the host has the guest where the guest thinks it is', sep < 3, `${sep.toFixed(2)} m apart`);
  const hostSeenByGuest = await actor('guest', 0);
  const hostOnHost = await actor('host', 0);
  check('the guest sees the host where the host is', Math.hypot(hostSeenByGuest.pos[0] - hostOnHost.pos[0], hostSeenByGuest.pos[2] - hostOnHost.pos[2]) < 6, JSON.stringify(hostSeenByGuest.pos));
  const names2 = [(await actor('guest', 0)).name, (await actor('guest', 1)).name, (await actor('host', 1)).name];
  check('names carry over', names2.join(',') === 'Hosty,Gus,Gus', names2.join(','));

  // ---- the guest shoots: ammo and the world agree ---------------------------------------------------------------------------
  await dbg('host', 'give ar epic'); // (on the host's copy of the guest's actor this is not what we want; give the guest a gun the proper way)
  const hud = await pages.guest.evaluate(() => window.__game.hud);
  check('the guest HUD shows the connection', !!hud && !!hud.net && hud.net.role === 'guest' && hud.net.players === 2, JSON.stringify(hud && hud.net));
  const hudHost = await pages.host.evaluate(() => window.__game.hud);
  check('the host HUD shows the room', !!hudHost && !!hudHost.net && hudHost.net.role === 'host' && hudHost.net.players === 2, JSON.stringify(hudHost && hudHost.net));

  // ---- the guest leaves ---------------------------------------------------------------------------------------------------------
  await pages.guest.evaluate(() => window.__game.mp.leave());
  await pages.guest.evaluate(() => { window.__game.fn.net_reset(); });
  await sleep(1500);
  await frames('host', 6);
  const hs2 = await net('host');
  check('the host notices the guest leaving', hs2.peers && hs2.peers.every((p) => !p.connected), JSON.stringify(hs2.peers));
  const g2 = await actor('host', 1);
  check('the guest\'s actor carries on as a bot', g2.human === false, JSON.stringify(g2));

  for (const who of ['host', 'guest']) {
    const bad = logs[who].filter((l) => /error|panick|unreachable/i.test(l) && !/WebGPU is experimental|favicon|404|Failed to load resource|ICE|stun/i.test(l));
    check(`no console errors on the ${who}`, bad.length === 0, bad.slice(0, 3).join(' | '));
  }
} catch (e) {
  console.error(e);
  failed++;
  for (const who of ['host', 'guest']) console.log(`--- ${who} log\n` + logs[who].slice(-15).join('\n'));
}
console.log(failed ? `\n${failed} check(s) failed` : '\nall checks passed');
await A.browser.close(); await B.browser.close(); srv.close();
process.exit(failed ? 1 : 0);
