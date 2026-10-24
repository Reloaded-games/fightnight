// The career leaderboard: every finished match of every player this browser has seen (the person at this keyboard, the friends
// in a multiplayer match and the bots), added up in localStorage. A match's own numbers and points come from the wasm module
// (`fn.scoreboard()`, see crates/fn-core/src/game/scoreboard.rs). A result counts once it is final (eliminated, or the match is
// decided), so a match left half-way only counts for those already out; quitting a match you are still in counts as going
// out right there. A counted result that still changes while the match goes on (a grenade its owner threw before falling goes
// off) is brought up to date.

const KEY = 'fightnight.leaderboard';
const HISTORY = 60; // recent results of people (not bots) kept for the player cards
const POLL = 0.5; // seconds between looks at the match's scoreboard
const MODES = [['all', 'All modes'], ['battle-royale', 'Battle Royale'], ['zero-build', 'Zero Build'], ['lego', 'LEGO']];
const MODE_NAMES = Object.fromEntries(MODES);
const MAP_NAMES = { chapter1: 'Chapter 1', chapter2: 'Chapter 2', island: 'Original' };

const $ = (id) => document.getElementById(id);
const esc = (t) => String(t).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
const int = (v) => Math.round(v).toLocaleString('en-US');
const pct = (v) => (v * 100).toFixed(v > 0 && v < 0.1 ? 1 : 0) + '%';
const clock = (s) => { s = Math.max(0, Math.floor(s)); const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60), x = s % 60; return h ? `${h}h ${String(m).padStart(2, '0')}m` : `${m}:${String(x).padStart(2, '0')}`; };
const ordinal = (n) => { const s = ['th', 'st', 'nd', 'rd'], v = n % 100; return n + (s[(v - 20) % 10] || s[v] || s[0]); };

// One player's totals (for all modes, and again per mode).
const blank = () => ({ matches: 0, wins: 0, top10: 0, score: 0, best: 0, kills: 0, mostKills: 0, damage: 0, headshots: 0, shots: 0, hits: 0, longest: 0, built: 0, opened: 0, time: 0, placeSum: 0, bestPlace: 0, quits: 0 });
const deaths = (s) => s.matches - s.wins;
const kd = (s) => s.kills / Math.max(1, deaths(s));

// The ranking's columns: the value it sorts on and how it is shown.
const COLUMNS = [
  { key: 'score', label: 'Score', val: (s) => s.score, fmt: int, title: 'Points from every finished match' },
  { key: 'matches', label: 'Matches', val: (s) => s.matches, fmt: int },
  { key: 'wins', label: 'Wins', val: (s) => s.wins, fmt: int, title: 'Victory Royales' },
  { key: 'winRate', label: 'Win %', val: (s) => (s.matches ? s.wins / s.matches : 0), fmt: pct },
  { key: 'kills', label: 'Elims', val: (s) => s.kills, fmt: int, title: 'Eliminations' },
  { key: 'kd', label: 'K/D', val: kd, fmt: (v) => v.toFixed(2), title: 'Eliminations per match lost' },
  { key: 'avgDamage', label: 'Avg dmg', val: (s) => (s.matches ? s.damage / s.matches : 0), fmt: int, title: 'Damage dealt per match' },
  { key: 'top10', label: 'Top 10', val: (s) => s.top10, fmt: int },
  { key: 'best', label: 'Best', val: (s) => s.best, fmt: int, title: 'Most points in one match' },
];

function fresh() { return { v: 1, players: {}, history: [] }; }

// What is stored (a fresh board if nothing usable is), or null when storage cannot be read at all.
function load() {
  try {
    const d = JSON.parse(localStorage.getItem(KEY) || 'null');
    return d && d.v === 1 && d.players && typeof d.players === 'object' && Array.isArray(d.history) ? d : fresh();
  } catch (e) {
    return null;
  }
}

function playerKey(name, bot) { return (bot ? 'b:' : 'p:') + String(name).trim().toLowerCase(); }

// The numbers of a result that can still change after it was counted.
const COUNTED = ['pts', 'kills', 'damage', 'headshots', 'shots', 'hits', 'longest', 'built', 'opened', 'won', 'place'];

// Add a result to a player's totals; `sign` -1 takes one back out, so a result that changed can be swapped for its new numbers
// (the bests only ever go up, and so do a result's numbers).
function add(s, r, sign = 1) {
  s.matches += sign;
  s.wins += sign * (r.won ? 1 : 0);
  s.top10 += sign * (r.place > 0 && r.place <= 10 ? 1 : 0);
  s.score += sign * r.pts;
  s.kills += sign * r.kills;
  s.damage += sign * r.damage;
  s.headshots += sign * r.headshots;
  s.shots += sign * r.shots;
  s.hits += sign * r.hits;
  s.built += sign * r.built;
  s.opened += sign * r.opened;
  s.time += sign * r.time;
  s.placeSum += sign * r.place;
  s.quits += sign * (r.quit ? 1 : 0);
  if (sign > 0) {
    s.best = Math.max(s.best, r.pts);
    s.mostKills = Math.max(s.mostKills, r.kills);
    s.longest = Math.max(s.longest, r.longest);
    s.bestPlace = s.bestPlace ? Math.min(s.bestPlace, r.place) : r.place;
  }
}

export function createLeaderboard({ fn, myName, click, show }) {
  let data = load() || fresh();
  let match = null; // the match being followed: { id, recorded: actor id -> { key, mode, res } as counted, acc: seconds since the last look }
  let board = null; // the latest scoreboard of that match
  const view = { mode: 'all', bots: false, sort: 'score', sel: null, confirmReset: false };
  try { Object.assign(view, JSON.parse(localStorage.getItem(KEY + '.view') || '{}'), { sel: null, confirmReset: false }); } catch (e) { /* defaults */ }
  if (!MODE_NAMES[view.mode]) view.mode = 'all';
  if (!COLUMNS.some((c) => c.key === view.sort)) view.sort = 'score';

  function save() {
    try { localStorage.setItem(KEY, JSON.stringify(data)); } catch (e) { /* full or unavailable: the board lives on for this visit */ }
  }
  function saveView() {
    try { localStorage.setItem(KEY + '.view', JSON.stringify({ mode: view.mode, bots: view.bots, sort: view.sort })); } catch (e) { /* ignore */ }
  }

  // Whose career a line counts for. The person at this keyboard is always themselves, whatever the match called them; anybody
  // else with their name (a friend who also kept the default "You") is kept apart from them.
  function keyOf(r) {
    const mine = playerKey(myName(), false);
    if (r.me) return { key: mine, name: myName() };
    const key = playerKey(r.name, !r.human);
    return key === mine ? { key: playerKey(r.name + ' (other)', false), name: r.name + ' (other)' } : { key, name: r.name };
  }

  // Add one finished result to the player's totals (and to the recent results, for people).
  function record(r, b, quit = false) {
    const bot = !r.human;
    const { key, name } = keyOf(r);
    const p = data.players[key] || (data.players[key] = { name, bot, all: blank(), modes: {} });
    p.name = name;
    p.last = Date.now();
    const res = { ...r, quit };
    add(p.all, res);
    add(p.modes[b.mode] || (p.modes[b.mode] = blank()), res);
    if (!bot) {
      data.history.unshift({ key, m: match.id, a: r.id, t: p.last, mode: b.mode, map: b.map, difficulty: b.difficulty, total: b.total, place: r.place, won: r.won, quit, kills: r.kills, damage: r.damage, time: r.time, pts: r.pts });
      if (data.history.length > HISTORY) data.history.length = HISTORY;
    }
    match.recorded.set(r.id, { key, mode: b.mode, res });
  }

  // A counted result whose numbers changed since (a grenade or rocket its owner let go before falling found somebody; on a
  // player's copy of a multiplayer match, the host's last numbers came in late): swap the old numbers for the new ones.
  // The time survived stays what it was when the result became final (a winner's clock runs on while they dance).
  function amend(r, rec) {
    const res = { ...r, time: rec.res.time, quit: rec.res.quit };
    const p = data.players[rec.key];
    if (p) {
      for (const s of [p.all, p.modes[rec.mode]]) {
        if (!s) continue;
        add(s, rec.res, -1);
        add(s, res);
      }
      const h = data.history.find((x) => x.m === match.id && x.a === r.id);
      if (h) Object.assign(h, { place: res.place, won: res.won, kills: res.kills, damage: res.damage, pts: res.pts });
    }
    rec.res = res;
  }

  // Look at the match's scoreboard: count every result that became final since the last look, and bring the counted ones
  // up to date.
  function poll() {
    if (!match) return board;
    let b;
    try { b = JSON.parse(fn.scoreboard()); } catch (e) { return board; }
    if (!b || !Array.isArray(b.rows)) return board;
    board = b;
    // someone who left the match is played on by a bot: that result is not theirs
    const final = b.rows.filter((r) => r.done && !r.left);
    const added = final.filter((r) => !match.recorded.has(r.id));
    const changed = final.filter((r) => match.recorded.has(r.id) && COUNTED.some((k) => r[k] !== match.recorded.get(r.id).res[k]));
    if (added.length || changed.length) {
      data = load() || data; // another tab may have added matches since
      for (const r of added) record(r, b);
      for (const r of changed) amend(r, match.recorded.get(r.id));
      save();
      if ($('leaderboard').classList.contains('active')) render();
    }
    if ($('over').classList.contains('active')) renderEnd();
    return b;
  }

  // ---- the ranking ------------------------------------------------------------------------------------------------------
  function statsOf(p) { return view.mode === 'all' ? p.all : p.modes[view.mode]; }
  function ranked() {
    const col = COLUMNS.find((c) => c.key === view.sort) || COLUMNS[0];
    const list = Object.entries(data.players)
      .filter(([, p]) => (view.bots || !p.bot) && statsOf(p) && statsOf(p).matches > 0)
      .map(([key, p]) => ({ key, p, s: statsOf(p) }));
    list.sort((a, b) => col.val(b.s) - col.val(a.s) || b.s.score - a.s.score || b.s.wins - a.s.wins || a.p.name.localeCompare(b.p.name));
    return list;
  }
  // Where the player stands among people (by score, all modes): for the end screen.
  function rankOf(key) {
    const people = Object.entries(data.players).filter(([, p]) => !p.bot && p.all.matches > 0).sort((a, b) => b[1].all.score - a[1].all.score);
    return { rank: people.findIndex(([k]) => k === key) + 1, of: people.length };
  }

  function render() {
    const me = playerKey(myName(), false);
    const list = ranked();
    for (const b of $('leaderboard-modes').querySelectorAll('button')) b.classList.toggle('on', b.dataset.v === view.mode);
    $('leaderboard-bots').checked = view.bots;
    const body = $('leaderboard-body');
    if (!list.length) {
      const anyone = Object.keys(data.players).length > 0;
      body.innerHTML = `<div class="lb-empty">${anyone ? 'Nobody has finished a match in this mode yet.' : 'No finished matches yet. Play one and your results land here.'}</div>`;
      $('leaderboard-detail').innerHTML = '';
      return;
    }
    if (!view.sel || !list.some((x) => x.key === view.sel)) view.sel = (list.find((x) => x.key === me) || list[0]).key;
    const head = COLUMNS.map((c) => `<th data-sort="${c.key}" class="${c.key === view.sort ? 'on' : ''}"${c.title ? ` title="${c.title}"` : ''}>${c.label}</th>`).join('');
    const rows = list.map((x, i) => {
      const tags = (x.p.bot ? '<span class="tag">bot</span>' : '') + (x.key === me ? '<span class="tag you">you</span>' : '');
      const cells = COLUMNS.map((c) => `<td class="${c.key === 'score' ? 'score' : ''}">${c.fmt(c.val(x.s))}</td>`).join('');
      return `<tr data-key="${esc(x.key)}" class="${x.key === me ? 'me' : ''}${x.key === view.sel ? ' sel' : ''}${x.p.bot ? ' bot' : ''}"><td class="rank"><span class="rank-badge${i < 3 ? ' r' + (i + 1) : ''}">${i + 1}</span></td><td class="name"><b>${esc(x.p.name)}</b>${tags}</td>${cells}</tr>`;
    }).join('');
    body.innerHTML = `<table class="lb-table"><thead><tr><th class="rank">#</th><th class="name">Player</th>${head}</tr></thead><tbody>${rows}</tbody></table>`;
    renderDetail(list.find((x) => x.key === view.sel), list.findIndex((x) => x.key === view.sel) + 1, me);
  }

  function renderDetail(x, rank, me) {
    if (!x) { $('leaderboard-detail').innerHTML = ''; return; }
    const s = x.s;
    const tile = (v, label) => `<div class="lb-tile"><b>${v}</b><span>${label}</span></div>`;
    const tiles = [
      tile(int(s.score), 'Score'),
      tile(int(s.matches ? s.score / s.matches : 0), 'Points / match'),
      tile(int(s.best), 'Best match'),
      tile(int(s.matches), 'Matches'),
      tile(int(s.wins), 'Victory Royales'),
      tile(pct(s.matches ? s.wins / s.matches : 0), 'Win rate'),
      tile(int(s.top10), 'Top 10'),
      tile(s.bestPlace ? '#' + s.bestPlace : '-', 'Best placement'),
      tile(s.matches ? '#' + (s.placeSum / s.matches).toFixed(1) : '-', 'Avg placement'),
      tile(int(s.kills), 'Eliminations'),
      tile(kd(s).toFixed(2), 'K/D'),
      tile(int(s.mostKills), 'Most elims in a match'),
      tile(s.longest ? Math.round(s.longest) + ' m' : '-', 'Longest elimination'),
      tile(int(s.damage), 'Damage dealt'),
      tile(int(s.matches ? s.damage / s.matches : 0), 'Damage / match'),
      tile(int(s.headshots), 'Headshots'),
      tile(s.shots ? pct(s.hits / s.shots) : '-', 'Accuracy'),
      tile(clock(s.time), 'Time survived'),
      tile(clock(s.matches ? s.time / s.matches : 0), 'Avg survival'),
      tile(int(s.built), 'Pieces built'),
      tile(int(s.opened), 'Chests & boxes opened'),
    ].join('');
    // the modes side by side
    const modes = view.mode === 'all' ? MODES.slice(1).filter(([m]) => x.p.modes[m]).map(([m, label]) => {
      const t = x.p.modes[m];
      return `<div class="lb-mode"><span>${label}</span><b>${int(t.score)}</b> pts &middot; ${int(t.matches)} played &middot; ${int(t.wins)} won &middot; ${int(t.kills)} elims</div>`;
    }).join('') : '';
    const recent = x.p.bot ? [] : data.history.filter((h) => h.key === x.key && (view.mode === 'all' || h.mode === view.mode)).slice(0, 8);
    const when = (t) => new Date(t).toLocaleString(undefined, { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });
    const recentHtml = recent.length ? `<h4>Recent matches</h4><table class="lb-recent"><thead><tr><th>When</th><th>Mode</th><th>Map</th><th>Place</th><th>Elims</th><th>Damage</th><th>Survived</th><th>Points</th></tr></thead><tbody>${recent.map((h) => `<tr class="${h.won ? 'won' : ''}"><td>${esc(when(h.t))}</td><td>${esc(MODE_NAMES[h.mode] || h.mode)}</td><td>${esc(MAP_NAMES[h.map] || h.map)}</td><td>${h.won ? 'Victory' : '#' + h.place + ' / ' + h.total}${h.quit ? ' (left)' : ''}</td><td>${h.kills}</td><td>${int(h.damage)}</td><td>${clock(h.time)}</td><td class="score">+${int(h.pts)}</td></tr>`).join('')}</tbody></table>` : '';
    $('leaderboard-detail').innerHTML = `<div class="lb-who"><span class="rank-badge${rank <= 3 ? ' r' + rank : ''}">${rank}</span><h4>${esc(x.p.name)}</h4>${x.p.bot ? '<span class="tag">bot</span>' : ''}${x.key === me ? '<span class="tag you">you</span>' : ''}<small>${MODE_NAMES[view.mode]}${x.p.last ? ' &middot; last played ' + esc(new Date(x.p.last).toLocaleDateString()) : ''}</small></div><div class="lb-tiles">${tiles}</div>${modes ? `<div class="lb-modes">${modes}</div>` : ''}${recentHtml}`;
  }

  function rules(b) {
    const k = (b && b.scoring) || { elim: 25, damagePer: 10, placement: 50, victory: 50 };
    return `Points per match: ${k.elim} per elimination, 1 per ${k.damagePer} damage dealt, up to ${k.placement} for how much of the field you outlast, and ${k.victory} more for a Victory Royale. A result counts once it is final; leaving a match you are still in counts as being eliminated there.`;
  }

  // ---- the end screen: this match's points and standings -------------------------------------------------------------------
  function renderEnd() {
    const b = board;
    const pointsEl = $('over-points'), boardEl = $('over-board');
    if (!b || !b.rows) { pointsEl.innerHTML = ''; boardEl.innerHTML = ''; return; }
    const mine = b.rows.find((r) => r.me);
    if (mine) {
      const [e, d, p, v] = mine.parts;
      const parts = [[e, 'elims'], [d, 'damage'], [p, 'placement'], [v, 'victory']].filter(([n]) => n > 0).map(([n, l]) => `${int(n)} ${l}`).join(' &middot; ');
      const { key } = keyOf(mine);
      const career = data.players[key];
      const { rank, of } = rankOf(key);
      const where = career && match && match.recorded.has(mine.id) ? `Career <b>${int(career.all.score)}</b> points${rank ? ` &middot; ${ordinal(rank)} of ${of} on the leaderboard` : ''}` : 'Counts on the leaderboard once your result is final';
      pointsEl.innerHTML = `<b>+${int(mine.pts)}</b> points<small>${parts || 'no points this time'}</small><small>${where}</small>`;
    } else pointsEl.innerHTML = '';
    // the best eight, and the player's own line if it is further down
    const top = b.rows.slice(0, 8);
    if (mine && !top.includes(mine)) top.push(mine);
    const line = (r) => `<tr class="${r.me ? 'me' : ''}"><td>${b.rows.indexOf(r) + 1}</td><td class="name">${esc(r.name)}${r.human ? '' : '<span class="tag">bot</span>'}</td><td>${r.won ? 'Win' : r.place ? '#' + r.place : r.alive ? '<i>in play</i>' : '-'}</td><td>${r.kills}</td><td>${int(r.damage)}</td><td class="score">${int(r.pts)}</td></tr>`;
    boardEl.innerHTML = `<h4>Match standings</h4><table><thead><tr><th>#</th><th class="name">Player</th><th>Place</th><th>Elims</th><th>Dmg</th><th>Pts</th></tr></thead><tbody>${top.map(line).join('')}</tbody></table>`;
  }

  // ---- wiring ---------------------------------------------------------------------------------------------------------------
  function open() {
    data = load() || data;
    view.confirmReset = false;
    $('leaderboard-reset').querySelector('span').textContent = 'Reset';
    $('leaderboard-rules').textContent = rules(board);
    render();
    show('leaderboard');
  }
  function close() { show('leaderboard', false); }

  function wire() {
    $('leaderboard-modes').innerHTML = MODES.map(([v, label]) => `<button data-v="${v}">${label}</button>`).join('');
    $('leaderboard-modes').addEventListener('click', (e) => {
      const b = e.target.closest('button');
      if (!b) return;
      view.mode = b.dataset.v; view.sel = null; saveView(); click(); render();
    });
    $('leaderboard-bots').addEventListener('change', () => { view.bots = $('leaderboard-bots').checked; saveView(); click(); render(); });
    $('leaderboard-body').addEventListener('click', (e) => {
      const th = e.target.closest('th[data-sort]');
      if (th) { view.sort = th.dataset.sort; saveView(); click(); render(); return; }
      const tr = e.target.closest('tr[data-key]');
      if (tr) { view.sel = tr.dataset.key; click(); render(); }
    });
    $('leaderboard-close').addEventListener('click', () => { click(); close(); });
    $('leaderboard-reset').addEventListener('click', () => {
      click();
      if (!view.confirmReset) {
        view.confirmReset = true;
        $('leaderboard-reset').querySelector('span').textContent = 'Click again to erase everything';
        return;
      }
      data = fresh();
      save();
      view.confirmReset = false;
      view.sel = null;
      $('leaderboard-reset').querySelector('span').textContent = 'Reset';
      render();
    });
  }

  return {
    wire,
    open,
    close,
    get board() { return board; },
    get data() { return data; },
    // A new match: nothing of it is counted yet.
    beginMatch() { match = { id: Date.now().toString(36) + Math.random().toString(36).slice(2, 7), recorded: new Map(), acc: 0 }; board = null; },
    // Called every frame during a match.
    tick(dt) {
      if (!match) return;
      match.acc += dt;
      if (match.acc < POLL) return;
      match.acc = 0;
      poll();
    },
    // The end screen is about to show: count what is final and fill in the points and standings.
    showEnd() { poll(); renderEnd(); },
    // The player walks out of a match. Results that are final are counted; if the player is still in it (and the match is
    // under way), that counts as being eliminated right now.
    leave(underway) {
      if (!match) return;
      const b = poll();
      const mine = b && b.rows.find((r) => r.me);
      if (underway && b.leave && mine && !mine.done && !mine.left && !match.recorded.has(mine.id)) {
        data = load() || data;
        record({ ...mine, ...b.leave, won: false, done: true }, b, true);
        save();
      }
      match = null;
    },
  };
}
