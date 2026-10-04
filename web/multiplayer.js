// Multiplayer front end: the "Multiplayer" screens, the invite/reply exchange, and the per-frame pump between the wasm game
// and the WebRTC links (see net.js). game.js owns the match itself; this owns the room around it.
import { Link, flush } from './net.js';

const $ = (id) => document.getElementById(id);
const POLL_MS = 250;

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
    $('lobby-title').textContent = 'Your room';
    $('lobby-host').classList.remove('hidden');
    $('lobby-guest').classList.add('hidden');
    $('btn-lobby-start').classList.remove('hidden');
    $('invites').innerHTML = '';
    invites = 0;
    msg('');
    addInvite();
    startPolling();
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
    // invites nobody answered are of no use any more
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
    $('lobby-players').innerHTML = '';
    msg('');
    startPolling();
  }

  async function guestContinue() {
    const code = $('join-invite').value.trim();
    if (!code) { msg('Paste the invite code your host sent you.', true); return; }
    msg('Making a reply code...');
    links.get(0)?.close();
    const link = new Link({
      open() {
        // the connection is up: now we can say hello
        fn.net_guest_open(ctx.name());
        msg('Connected. Waiting for the host to start the match...');
        pump();
      },
      close(why) { links.delete(0); try { fn.net_disconnected(0); } catch (e) { /* shutting down */ } msg(why || 'Disconnected from the host.', true); },
      message(bytes) { try { fn.net_receive(0, bytes); } catch (e) { console.error(e); } },
    });
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
