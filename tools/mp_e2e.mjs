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
const vehicles = async (who) => JSON.parse(await dbg(who, 'vehicles'));
const net = async (who) => JSON.parse(await pages[who].evaluate(() => window.__game.fn.net_status()));
// Native GPUs render these small viewports much faster than software WebGPU.
// Hold input for simulation time as well as frame count, just like the solo test.
const frames = async (who, n) => {
  const p = pages[who];
  const start = await p.evaluate(() => ({ frames: window.__game.frames, t: JSON.parse(window.__game.fn.debug('state')).t }));
  await p.waitForFunction(({ frames, t, native }) =>
    window.__game.frames >= frames && (!native || window.__game.state !== 'playing' || JSON.parse(window.__game.fn.debug('state')).t >= t),
  { frames: start.frames + n, t: start.t + n / 10, native: process.env.FN_FLAGS === 'native' }, { timeout: 180000 });
};
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
  await pages.host.click('#seg-mode button[data-v="lego"]');
  await pages.host.click('#btn-multi');
  await pages.host.click('#btn-mp-host');
  check('the room creator is identified as the host', (await pages.host.textContent('#lobby-title')).includes('You are the host'));
  check('the host can choose LEGO in its room', await pages.host.getAttribute('#lb-mode button[data-v="lego"]', 'aria-pressed') === 'true');
  await pages.host.waitForFunction(() => document.querySelector('#invites .inv')?.value.startsWith('FN1.'), null, { timeout: 30000 });
  const invite = await pages.host.evaluate(() => document.querySelector('#invites .inv').value);
  check('the host gets an invite code', invite.startsWith('FN1.') && invite.length > 100, `${invite.length} characters`);

  // ---- the guest answers it ---------------------------------------------------------------------------------------------------
  await pages.guest.evaluate(() => { window.__game.cfg.name = 'Gus'; });
  await pages.guest.click('#seg-mode button[data-v="zero-build"]');
  await pages.guest.click('#btn-multi');
  await pages.guest.click('#btn-mp-join');
  check('a guest cannot start the host room', !(await pages.guest.isVisible('#btn-lobby-start')));
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
  check('the host determines the mode for both players', hs.mode === 'LEGO' && gs.mode === 'LEGO', `${hs.mode} / ${gs.mode}`);
  const modeHuds = await Promise.all(['host', 'guest'].map((who) => pages[who].evaluate(() => ({ mode: window.__game.hud.gameMode, world: window.__game.hud.worldSize }))));
  check('both HUDs show the host LEGO mode despite the guest choosing Zero Build', modeHuds.every((h) => h.mode === 'lego'), JSON.stringify(modeHuds));
  check('both players use the expanded island', modeHuds.every((h) => h.world === 1920), JSON.stringify(modeHuds));

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

  // ---- the guest drives; the host owns the seat and sees the same vehicle --------------------------------------------------
  const hostFleet = await vehicles('host'), guestFleet = await vehicles('guest');
  check('both players share the same vehicle fleet', hostFleet.length > 0 && hostFleet.map((v) => v.id).join(',') === guestFleet.map((v) => v.id).join(','), `${hostFleet.length} buggies`);
  const carId = hostFleet[0].id;
  await dbg('host', 'vehicle 0 1');
  await pages.guest.waitForFunction(() => window.__game.hud?.prompt?.kind === 'vehicle' && window.__game.hud.prompt.txt.startsWith('Drive'), null, { timeout: 30000 });
  check('the guest sees the vehicle entry prompt after the host positions it', (await pages.guest.evaluate(() => window.__game.hud.prompt.kind)) === 'vehicle');
  await pages.guest.keyboard.press('KeyE');
  for (const who of ['host', 'guest']) await pages[who].waitForFunction((id) => JSON.parse(window.__game.fn.debug('vehicles')).some((v) => v.id === id && v.driver === 1), carId, { timeout: 30000 });
  check('E assigns the guest a seat on the host and its replica', (await vehicles('host')).find((v) => v.id === carId).driver === 1 && (await vehicles('guest')).find((v) => v.id === carId).driver === 1);
  await pages.guest.waitForFunction((id) => window.__game.hud?.vehicle?.id === id, carId, { timeout: 30000 });
  const drivingHud = await pages.guest.evaluate(() => ({ vehicle: window.__game.hud.vehicle, prompt: window.__game.hud.prompt }));
  check('the guest HUD shows driving controls and speed', drivingHud.vehicle.name === 'Island Buggy' && Number.isFinite(drivingHud.vehicle.speed) && drivingHud.prompt.txt === 'Exit buggy');

  const parked = (await vehicles('host')).find((v) => v.id === carId);
  await pages.guest.keyboard.down('KeyW');
  await frames('guest', 16);
  await pages.guest.keyboard.up('KeyW');
  await frames('host', 3);
  const driven = (await vehicles('host')).find((v) => v.id === carId);
  const replica = (await vehicles('guest')).find((v) => v.id === carId);
  const distance = Math.hypot(driven.pos[0] - parked.pos[0], driven.pos[2] - parked.pos[2]);
  const carSep = Math.hypot(driven.pos[0] - replica.pos[0], driven.pos[2] - replica.pos[2]);
  check('W from the guest moves the authoritative buggy', distance > 3 && driven.speed > 1, `${distance.toFixed(1)} m, ${(driven.speed * 3.6).toFixed(1)} km/h`);
  check('the host and guest agree on buggy movement', carSep < 4, `${carSep.toFixed(2)} m apart`);

  await pages.guest.keyboard.down('Space');
  await frames('guest', 14);
  await pages.guest.keyboard.up('Space');
  await frames('host', 2);
  const stopped = (await vehicles('host')).find((v) => v.id === carId);
  check('Space brakes the guest buggy on the host', Math.abs(stopped.speed) < 1, `${Math.abs(stopped.speed).toFixed(2)} m/s`);
  await pages.guest.keyboard.press('KeyE');
  for (const who of ['host', 'guest']) await pages[who].waitForFunction((id) => JSON.parse(window.__game.fn.debug('vehicles')).some((v) => v.id === id && v.driver === null), carId, { timeout: 30000 });
  await pages.guest.waitForFunction(() => window.__game.hud?.vehicle === null, null, { timeout: 30000 });
  check('E exits the buggy and releases the seat on both machines', (await vehicles('host')).find((v) => v.id === carId).driver === null && (await vehicles('guest')).find((v) => v.id === carId).driver === null);
  check('the guest can return to on-foot play after driving', (await actor('guest', 1)).mode === 'Ground');

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
    const bad = logs[who].filter((l) => /error|panick|unreachable/i.test(l) && !/WebGPU is experimental|favicon|404|Failed to load resource|\bICE\b|\bstun\b/i.test(l));
    check(`no console errors on the ${who}`, bad.length === 0, bad.slice(0, 3).join(' | '));
  }
} catch (e) {
  console.error(e);
  failed++;
  for (const who of ['host', 'guest']) console.log(`--- ${who} log\n` + logs[who].slice(-15).join('\n'));
}
if (failed) {
  for (const who of ['host', 'guest']) {
    console.log(`--- ${who}: ${await pages[who].evaluate(() => document.getElementById('err').textContent).catch(() => '?')}`);
    console.log(logs[who].filter((l) => !/WebGPU is experimental/.test(l)).slice(-12).join('\n'));
  }
}
console.log(failed ? `\n${failed} check(s) failed` : '\nall checks passed');
await A.browser.close(); await B.browser.close(); srv.close();
process.exit(failed ? 1 : 0);
