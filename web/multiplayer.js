// Multiplayer front end: the "Multiplayer" screens, rooms (listed or private, found through the room service in rooms.js),
// the hand-copied invite/reply exchange kept for when that service cannot be reached, and the per-frame pump between the
// wasm game and the WebRTC links (see net.js). game.js owns the match itself; this owns the room around it.
import { Link, flush } from './net.js';
import { RoomService, HostedRoom, watchRooms, requestJoin, randomCode, cleanCode, newListingId, CODE_LENGTH } from './rooms.js';

const $ = (id) => document.getElementById(id);
const POLL_MS = 250;
const MAX_PLAYERS = 8;
const MODE_NAMES = { 'battle-royale': 'Battle Royale', 'zero-build': 'Zero Build', lego: 'LEGO' };
const MAP_NAMES = { chapter1: 'Chapter 1', chapter2: 'Chapter 2', island: 'Original island' };
// (tests point the page at their own broker: index.html?rooms=ws://127.0.0.1:1234)
const BROKERS = (new URLSearchParams(location.search).get('rooms') || '').split(',').map((s) => s.trim()).filter(Boolean);

export function createMultiplayer(ctx) {
  // ctx: { fn, cfg, show(id, on), click(), note(text), name(), startMatch(kind), toMenu(), setProgress(p, text) }
  const { fn } = ctx;
  let role = null; // 'host' | 'guest' | null
  const links = new Map(); // peer id -> Link (host side has one per invited player, guest side has peer 0)
  let nextPeer = 1;
  let invites = 0;
  let pollTimer = 0;
  let starting = false;
  let lastNames = '';
  let service = null; // the room service connection (rooms.js)
  let room = null; // host: the room we keep open
  let stopWatching = null; // guest: stops the room list
  let joining = false; // guest: a join is under way
  let joinSeq = 0; // guest: which join attempt is the current one
  let joinAbort = null; // guest: cancels the join under way
  const guestLinks = new Map(); // host: room-service guest id -> its Link

  const route = (peer) => links.get(peer);
  const status = () => { try { return JSON.parse(fn.net_status()); } catch (e) { return { state: 'none' }; } };
  const msg = (text, bad = false) => { const el = $('lobby-msg'); el.textContent = text || ''; el.classList.toggle('bad', !!bad); };

  function pump() { if (role) flush(fn, route); }

  function dropLinks() {
    for (const l of links.values()) l.close();
    links.clear();
  }

  // ---- screens -------------------------------------------------------------------------------------------------------
  function openMenu() {
    ctx.show('menu', false);
    ctx.show('lobby', false);
    ctx.show('mp');
  }

  function backToMain() {
    ctx.show('mp', false);
    ctx.show('lobby', false);
    ctx.show('menu');
  }

  function renderPlayers(st) {
    const names = st.names || [];
    const box = $('lobby-players');
    box.innerHTML = '';
    names.forEach((n, i) => {
      const chip = document.createElement('div');
      chip.className = 'chip' + (i === 0 ? ' host' : '') + (role === 'host' ? (i === 0 ? ' you' : '') : '');
      chip.textContent = n;
      box.appendChild(chip);
    });
  }

  // ---- hosting ---------------------------------------------------------------------------------------------------------
  function hostLobby() {
    leaveQuietly();
    role = 'host';
    starting = false;
    fn.net_host_open(ctx.name());
    ctx.show('mp', false);
    ctx.show('lobby');
    $('lobby-title').textContent = 'Your room · You are the host';
    $('lobby-host').classList.remove('hidden');
    $('lobby-guest').classList.add('hidden');
    $('btn-lobby-start').classList.remove('hidden');
    $('invites').innerHTML = '';
    invites = 0;
    $('manual-host').open = false;
    msg('You created this room. Your browser hosts the game and chooses the mode. Keep this tab open while everyone plays.');
    openRoom();
    startPolling();
  }

  /** Put the room on the room service: listed (public) or found by its code only (private). */
  function openRoom() {
    const code = randomCode();
    const listing = newListingId();
    $('room-code').textContent = code;
    service = new RoomService({ version: fn.net_version(), brokers: BROKERS });
    const svc = service;
    room = new HostedRoom(svc, {
      code,
      listing,
      info: roomInfo(),
      onJoin: (guest, name) => admit(guest, name),
      onAnswer: (guest, reply) => {
        const link = guestLinks.get(guest);
        if (link) link.acceptReply(reply).catch((e) => { console.warn('join reply', e); link.close(); });
      },
    });
    room.public = $('lb-visibility').querySelector('button.on')?.dataset.v !== 'private';
    setVisibility(room.public);
    const showService = () => {
      if (service !== svc) return;
      const st = $('room-status');
      st.textContent = svc.connected ? 'Room service connected: players can find this room.' : 'Connecting to the room service...';
      st.classList.toggle('bad', !svc.connected);
    };
    svc.onChange = showService;
    showService();
    // (the room works as soon as a broker answers, and again after any of them comes back)
    room.open().catch((e) => console.warn('room', e));
    // when the host's page goes away the broker takes the room off the list by itself (a private room is never on it)
    svc.start({ will: { topic: svc.listTopic(listing), payload: '', retain: true } })
      .then(() => { if (service === svc) msg('You created this room. Your browser hosts the game and chooses the mode. Keep this tab open while everyone plays.'); })
      .catch((e) => {
        if (service !== svc) return;
        showService();
        msg(e.message + ' Still trying. Meanwhile you can invite friends with copied codes below.', true);
        $('manual-host').open = true;
        if (!invites) addInvite();
      });
  }

  function roomInfo() {
    const st = status();
    return { host: ctx.name(), mode: ctx.cfg.mode, map: ctx.cfg.map, players: (st.names || []).length || 1, max: MAX_PLAYERS };
  }

  /** A player asked to join through the room service: make them an invite (their page answers it by itself). */
  async function admit(guest, name) {
    if (starting) throw new Error('The match has already started.');
    if ((status().names || []).length + [...guestLinks.values()].filter((l) => !l.opened && !l.closed).length >= MAX_PLAYERS) throw new Error('The room is full.');
    const peer = nextPeer++;
    let link = null;
    // whatever ends this join (it failed, the guest left, nobody finished it) frees the seat and lets them try again
    const drop = () => {
      links.delete(peer);
      if (guestLinks.get(guest) === link) guestLinks.delete(guest);
      room?.forget(guest);
    };
    link = new Link({
      open() { fn.net_connected(peer); },
      close() {
        drop();
        try { fn.net_disconnected(peer); } catch (e) { /* the page is shutting down */ }
      },
      message(bytes) { try { fn.net_receive(peer, bytes); } catch (e) { console.error(e); } },
    });
    link.drop = drop;
    links.set(peer, link);
    guestLinks.get(guest)?.close();
    guestLinks.set(guest, link);
    // a request nobody finishes (the guest closed the page) must not hold a seat for ever
    setTimeout(() => { if (!link.opened && !link.closed) { link.close(); drop(); } }, 30000);
    console.info(`room: ${name} is joining`);
    try {
      return await link.createInvite();
    } catch (e) {
      link.close();
      drop();
      throw e;
    }
  }

  function setVisibility(pub) {
    for (const b of $('lb-visibility').querySelectorAll('button')) {
      const on = (b.dataset.v === 'public') === pub;
      b.classList.toggle('on', on); b.setAttribute('aria-pressed', String(on));
    }
    $('room-vis-note').textContent = pub ? 'Listed under "Join a game": anyone can join.' : 'Not listed: only players who type the code can join.';
    room?.setPublic(pub);
  }

  function closeRoom() {
    room?.close();
    room = null;
    for (const l of [...guestLinks.values()]) if (!l.opened) { l.close(); l.drop?.(); }
    guestLinks.clear();
    const svc = service;
    service = null;
    // (a moment for the "room closed" message to go out first)
    if (svc) setTimeout(() => svc.stop(), 400);
  }

  function addInvite() {
    const peer = nextPeer++;
    invites++;
    const el = document.createElement('div');
    el.className = 'invite';
    el.innerHTML = `<header>Player ${invites + 1}<span class="st">making an invite...</span></header>
      <div class="row2">
        <div><small>Invite code (send this to your friend)</small><textarea class="inv" rows="3" readonly spellcheck="false"></textarea><div class="mp-actions"><button class="btn small ghost cp"><span>Copy invite</span></button></div></div>
        <div><small>Their reply code (paste it here)</small><textarea class="rep" rows="3" spellcheck="false" placeholder="FN1...."></textarea><div class="mp-actions"><button class="btn small go"><span>Connect</span></button></div></div>
      </div>`;
    $('invites').appendChild(el);
    const st = el.querySelector('.st'), inv = el.querySelector('.inv'), rep = el.querySelector('.rep');
    const setSt = (t, cls = '') => { st.textContent = t; st.className = 'st ' + cls; };
    const link = new Link({
      open() { setSt('connected', 'ok'); fn.net_connected(peer); },
      close(why) { setSt(why || 'disconnected', 'bad'); links.delete(peer); try { fn.net_disconnected(peer); } catch (e) { /* the page is shutting down */ } },
      message(bytes) { try { fn.net_receive(peer, bytes); } catch (e) { console.error(e); } },
    });
    links.set(peer, link);
    link.createInvite().then((code) => { inv.value = code; setSt('waiting for their reply'); }).catch((e) => setSt('could not make an invite: ' + e.message, 'bad'));
    el.querySelector('.cp').addEventListener('click', () => { ctx.click(); copy(inv.value, inv); });
    el.querySelector('.go').addEventListener('click', async () => {
      ctx.click();
      try { setSt('connecting...'); await link.acceptReply(rep.value); } catch (e) { setSt(e.message || String(e), 'bad'); }
    });
    inv.addEventListener('focus', () => inv.select());
  }

  async function startHosted() {
    if (starting) return;
    starting = true;
    // invites nobody answered are of no use any more, and the room comes off the list
    closeRoom();
    for (const [peer, l] of links) if (!l.opened) { l.close(); links.delete(peer); }
    await ctx.startMatch('host');
  }

  // ---- joining -----------------------------------------------------------------------------------------------------------
  function guestLobby() {
    leaveQuietly();
    role = 'guest';
    starting = false;
    ctx.show('mp', false);
    ctx.show('lobby');
    $('lobby-title').textContent = 'Join a game';
    $('lobby-host').classList.add('hidden');
    $('lobby-guest').classList.remove('hidden');
    $('btn-lobby-start').classList.add('hidden');
    $('join-invite').value = '';
    $('join-reply').value = '';
    $('join-code').value = '';
    $('manual-guest').open = false;
    $('join-pick').classList.remove('hidden');
    renderRoomsDisabled(false);
    $('lobby-players').innerHTML = '';
    msg('');
    findRooms();
    startPolling();
  }

  /** Connect to the room service and keep the list of open rooms up to date. */
  function findRooms() {
    joining = false;
    shownRooms = '';
    service = new RoomService({ version: fn.net_version(), brokers: BROKERS });
    const svc = service;
    let latest = [];
    const st = $('rooms-status');
    const showService = () => {
      if (service !== svc) return;
      if (!svc.connected) { st.textContent = svc.failed ? 'Cannot reach the room service. Still trying...' : 'Looking for rooms...'; st.classList.toggle('bad', !!svc.failed); }
      else st.classList.remove('bad');
      renderRooms(latest);
    };
    svc.onChange = showService;
    showService();
    // (the list fills in as soon as a broker answers, and again after any of them comes back)
    stopWatching = watchRooms(svc, (rooms) => { latest = rooms; if (service === svc) renderRooms(rooms); });
    svc.start().catch(() => {
      if (service !== svc) return;
      svc.failed = true;
      showService();
      $('manual-guest').open = true;
    });
  }

  let shownRooms = '';
  function renderRooms(rooms) {
    if (service?.connected) $('rooms-status').textContent = rooms.length ? `${rooms.length} open room${rooms.length > 1 ? 's' : ''}` : 'Connected';
    // (only redraw when something changed: a button that keeps being replaced swallows clicks)
    const key = JSON.stringify([!!service?.connected, joining, rooms.map((r) => [r.code, r.host, r.mode, r.map, r.players, r.max])]);
    if (key === shownRooms) return;
    shownRooms = key;
    const list = $('room-list');
    list.innerHTML = '';
    if (!rooms.length) {
      const e = document.createElement('div');
      e.className = 'empty';
      e.textContent = service?.connected ? 'No open rooms right now. Host one, or type the code of a private room below.' : 'Looking for rooms...';
      list.appendChild(e);
      return;
    }
    for (const r of rooms) {
      const row = document.createElement('div');
      row.className = 'room-row';
      row.dataset.code = r.code;
      const full = (r.players || 1) >= (r.max || MAX_PLAYERS);
      const who = document.createElement('div');
      who.className = 'who';
      who.textContent = `${String(r.host || 'Player').slice(0, 16)}'s room`;
      const what = document.createElement('div');
      what.className = 'what';
      what.textContent = `${MODE_NAMES[r.mode] || 'Battle Royale'} · ${MAP_NAMES[r.map] || 'Island'}`;
      const count = document.createElement('div');
      count.className = 'count';
      count.textContent = `${r.players || 1}/${r.max || MAX_PLAYERS}`;
      const go = document.createElement('button');
      go.className = 'btn small join';
      go.innerHTML = '<span>Join</span>';
      go.disabled = full || joining;
      go.addEventListener('click', () => { ctx.click(); joinRoom(r.code, r.hpub); });
      row.append(who, what, count, go);
      list.appendChild(row);
    }
  }

  /**
   * Join the room with this code: the room service carries the connection set-up, then the two pages talk directly.
   * `hpub` (from the room list) makes sure it is that room's host who answers.
   */
  async function joinRoom(code, hpub = null) {
    code = cleanCode(code);
    if (code.length !== CODE_LENGTH) { msg(`A room code has ${CODE_LENGTH} letters and digits.`, true); return; }
    if (joining) return;
    if (!service || !service.connected) { msg('Not connected to the room service yet. Try again in a moment.', true); return; }
    joining = true;
    const attempt = ++joinSeq;
    const abort = new AbortController();
    joinAbort = abort;
    const current = () => attempt === joinSeq && !abort.signal.aborted;
    renderRoomsDisabled(true);
    msg(`Joining room ${code}...`);
    if (links.get(0)) { links.get(0).close(); links.delete(0); }
    const link = guestLink(() => msg('Connected. Waiting for the host to start the match...'));
    links.set(0, link);
    try {
      await requestJoin(service, code, ctx.name(), (offer) => link.acceptInvite(offer), { hpub, signal: abort.signal });
      if (!current()) return;
      msg('Connecting to the host...');
      // the connection opens by itself now; give it a little time
      const t0 = performance.now();
      while (!link.opened && !link.closed && current() && performance.now() - t0 < 20000) await new Promise((r) => setTimeout(r, 100));
      if (!current()) return;
      if (!link.opened) throw new Error(link.closed ? 'The connection to the host failed.' : 'The host did not connect. Try again.');
      $('join-pick').classList.add('hidden');
      // the room service is no longer needed: from here on the host and this page talk directly
      stopWatching?.(); stopWatching = null;
      const svc = service; service = null; svc?.stop();
    } catch (e) {
      if (links.get(0) === link) links.delete(0);
      link.close();
      if (!current() || e.name === 'AbortError') return;
      joining = false;
      renderRoomsDisabled(false);
      msg(e.message || String(e), true);
    } finally {
      if (joinAbort === abort) joinAbort = null;
    }
  }

  /** Give up a join that is still under way (leaving the screen, or joining another way). */
  function cancelJoin() {
    joinSeq++;
    joinAbort?.abort();
    joinAbort = null;
    joining = false;
  }

  function renderRoomsDisabled(on) {
    for (const b of $('room-list').querySelectorAll('button.join')) b.disabled = on;
    $('btn-join-code').disabled = on;
  }

  /** The guest's link to the host; `onOpen` after the hello has gone out. */
  function guestLink(onOpen) {
    return new Link({
      open() {
        // the connection is up: now we can say hello
        fn.net_guest_open(ctx.name());
        onOpen();
        pump();
      },
      close(why) { links.delete(0); try { fn.net_disconnected(0); } catch (e) { /* shutting down */ } msg(why || 'Disconnected from the host.', true); },
      message(bytes) { try { fn.net_receive(0, bytes); } catch (e) { console.error(e); } },
    });
  }

  async function guestContinue() {
    const code = $('join-invite').value.trim();
    if (!code) { msg('Paste the invite code your host sent you.', true); return; }
    cancelJoin();
    renderRoomsDisabled(false);
    msg('Making a reply code...');
    links.get(0)?.close();
    const link = guestLink(() => msg('Connected. Waiting for the host to start the match...'));
    links.set(0, link);
    try {
      $('join-reply').value = await link.acceptInvite(code);
      msg('Now send the reply code to the host and wait for them to press Connect.');
    } catch (e) {
      links.delete(0);
      link.close();
      msg(e.message || String(e), true);
    }
  }

  // ---- polling the room -------------------------------------------------------------------------------------------------
  function startPolling() {
    clearInterval(pollTimer);
    pollTimer = setInterval(poll, POLL_MS);
  }

  async function poll() {
    if (!role) return;
    const st = status();
    if (st.state === 'host-lobby' || st.state === 'guest-lobby') {
      const key = JSON.stringify(st.names);
      if (key !== lastNames) { lastNames = key; renderPlayers(st); }
      if (role === 'host' && room) room.update(roomInfo());
      if (st.closed) {
        msg(st.closed, true);
        links.get(0)?.close();
        if (!starting) clearInterval(pollTimer);
      } else if (role === 'guest' && st.started && !starting) {
        starting = true;
        clearInterval(pollTimer);
        await ctx.startMatch('guest');
      }
    }
  }

  // ---- the match ---------------------------------------------------------------------------------------------------------
  /** Heavy part of starting a hosted or joined match (called by game.js with the loading screen up). */
  async function begin(kind, opts) {
    if (kind === 'host') {
      fn.net_begin_host(opts);
      pump(); // the guests start building their islands while we build ours
      await new Promise((r) => requestAnimationFrame(() => r()));
      const info = fn.net_finish_host();
      pump();
      return info;
    }
    const info = fn.net_start_guest();
    pump();
    return info;
  }

  /** Wait (with the loading screen showing why) until the host has begun the match. */
  async function waitForStart() {
    const t0 = performance.now();
    let last = t0;
    for (;;) {
      // the match only advances when it is ticked: the host's clock (and so the wait for slow players) is this time
      const now = performance.now();
      fn.net_tick(Math.min(0.1, (now - last) / 1000));
      last = now;
      pump();
      const st = status();
      if (st.state === 'host') {
        if (st.started) return true;
        ctx.setProgress(0.9, st.waiting ? `Waiting for ${st.waiting} player${st.waiting > 1 ? 's' : ''} to get ready` : 'Starting');
      } else if (st.state === 'guest') {
        if (st.closed) throw new Error(st.closed);
        if (st.synced) return true;
        ctx.setProgress(0.9, 'Waiting for the host');
      } else return false;
      if (performance.now() - t0 > 90000) throw new Error('The match did not start. Check your connection.');
      await new Promise((r) => setTimeout(r, 100));
    }
  }

  /** Connection problems during a match: returns the reason once the match is over for this player. */
  function problem() {
    if (!role) return null;
    const st = status();
    if (st.state === 'guest' && st.closed) return st.closed;
    return null;
  }

  // ---- leaving -----------------------------------------------------------------------------------------------------------
  function leaveQuietly() {
    clearInterval(pollTimer);
    cancelJoin();
    stopWatching?.(); stopWatching = null;
    closeRoom();
    if (role) {
      try { fn.net_reset(); } catch (e) { /* ignore */ }
      pump(); // goodbyes
      const old = [...links.values()];
      links.clear();
      setTimeout(() => old.forEach((l) => l.close()), 300);
    }
    role = null;
    starting = false;
    lastNames = '';
    nextPeer = 1;
  }

  function leave() {
    leaveQuietly();
    ctx.show('lobby', false);
    ctx.show('mp', false);
  }

  function copy(text, el) {
    if (!text) return;
    const done = () => ctx.note('Copied. Now send it to your friend.', 2500);
    if (navigator.clipboard && navigator.clipboard.writeText) navigator.clipboard.writeText(text).then(done, () => { el.select(); document.execCommand?.('copy'); done(); });
    else { el.select(); document.execCommand?.('copy'); done(); }
  }

  function wire() {
    $('btn-multi').addEventListener('click', () => { ctx.click(); openMenu(); });
    $('btn-mp-back').addEventListener('click', () => { ctx.click(); backToMain(); });
    $('btn-mp-host').addEventListener('click', () => { ctx.click(); hostLobby(); });
    $('btn-mp-join').addEventListener('click', () => { ctx.click(); guestLobby(); });
    $('btn-add-invite').addEventListener('click', () => { ctx.click(); if (invites < 7) addInvite(); });
    $('btn-join-continue').addEventListener('click', () => { ctx.click(); guestContinue(); });
    $('btn-join-code').addEventListener('click', () => { ctx.click(); joinRoom($('join-code').value); });
    $('join-code').addEventListener('input', () => { const el = $('join-code'); const c = cleanCode(el.value); if (c !== el.value) el.value = c; });
    $('join-code').addEventListener('keydown', (e) => { if (e.key === 'Enter') { e.preventDefault(); e.stopPropagation(); joinRoom($('join-code').value); } });
    for (const b of $('lb-visibility').querySelectorAll('button')) b.addEventListener('click', () => { ctx.click(); setVisibility(b.dataset.v === 'public'); });
    $('manual-host').addEventListener('toggle', () => { if ($('manual-host').open && role === 'host' && !invites) addInvite(); });
    $('btn-copy-reply').addEventListener('click', () => { ctx.click(); copy($('join-reply').value, $('join-reply')); });
    $('btn-lobby-start').addEventListener('click', () => { ctx.click(); startHosted(); });
    $('btn-lobby-leave').addEventListener('click', () => { ctx.click(); leave(); openMenu(); });
    $('join-reply').addEventListener('focus', () => $('join-reply').select());
  }

  return {
    wire,
    pump,
    begin,
    waitForStart,
    problem,
    leave,
    get active() { return !!role; },
    get role() { return role; },
  };
}
