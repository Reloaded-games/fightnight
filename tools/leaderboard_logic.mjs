// The leaderboard's bookkeeping without a browser: web/leaderboard.js driven with a fake page and a scripted scoreboard. Two
// people both called "You" in one multiplayer match, a winner whose last numbers arrive late, and another tab writing meanwhile.
//   node leaderboard_logic.mjs
const store = new Map();
globalThis.localStorage = { getItem: (k) => (store.has(k) ? store.get(k) : null), setItem: (k, v) => store.set(k, String(v)), removeItem: (k) => store.delete(k) };
const el = () => ({ classList: { contains: () => false, toggle() {} }, querySelectorAll: () => [], querySelector: () => ({ textContent: '' }), addEventListener() {}, innerHTML: '', textContent: '', checked: false });
globalThis.document = { getElementById: el };
const { createLeaderboard } = await import('../web/leaderboard.js');
let board;
const row = (o) => ({ human: true, me: false, left: false, alive: false, done: true, place: 2, won: false, kills: 0, damage: 0, time: 100, headshots: 0, shots: 0, hits: 0, longest: 0, built: 0, opened: 0, pts: 0, parts: [0, 0, 0, 0], ...o });
const fn = { scoreboard: () => JSON.stringify(board) };
const lb = createLeaderboard({ fn, myName: () => 'You', click() {}, show() {} });
let fails = 0;
const check = (n, ok, x = '') => { console.log(`${ok ? 'PASS' : 'FAIL'}  ${n}${x ? '  ' + x : ''}`); if (!ok) fails++; };
board = { over: false, total: 3, alive: 2, mode: 'battle-royale', map: 'island', difficulty: 'normal', scoring: { elim: 25, damagePer: 10, placement: 50, victory: 50 }, leave: null,
  rows: [row({ id: 0, name: 'You', me: true, place: 3, kills: 1, pts: 25 }), row({ id: 1, name: 'You', alive: true, done: false, place: 0 }), row({ id: 2, name: 'Bot', human: false, alive: true, done: false, place: 0 })] };
lb.beginMatch();
lb.tick(1);
let d = JSON.parse(store.get('fightnight.leaderboard'));
check('the local player is counted under their own key', d.players['p:you']?.all.matches === 1 && d.players['p:you'].all.kills === 1);
// the friend also called "You" wins; first the line arrives without the last elimination, then with it
board.over = true; board.alive = 1;
board.rows[1] = row({ id: 1, name: 'You', alive: true, done: true, place: 1, won: true, kills: 2, pts: 150 });
lb.tick(1);
d = JSON.parse(store.get('fightnight.leaderboard'));
check('a friend with the same name is kept apart', d.players['p:you'].all.matches === 1 && d.players['p:you (other)']?.all.matches === 1 && d.players['p:you (other)'].name === 'You (other)', Object.keys(d.players).join(','));
// another tab adds a match meanwhile (the stored board changes under us)
d.players['p:zed'] = { name: 'Zed', bot: false, all: { matches: 1, wins: 0, top10: 1, score: 5, best: 5, kills: 0, mostKills: 0, damage: 0, headshots: 0, shots: 0, hits: 0, longest: 0, built: 0, opened: 0, time: 1, placeSum: 3, bestPlace: 3, quits: 0 }, modes: {} };
store.set('fightnight.leaderboard', JSON.stringify(d));
board.rows[1] = row({ id: 1, name: 'You', alive: true, done: true, place: 1, won: true, kills: 3, damage: 120, pts: 187, time: 140 });
board.rows[2] = row({ id: 2, name: 'Bot', human: false, place: 2, pts: 49 });
lb.tick(1);
d = JSON.parse(store.get('fightnight.leaderboard'));
const o = d.players['p:you (other)'].all;
check('the late numbers replace the early ones', o.matches === 1 && o.wins === 1 && o.kills === 3 && o.damage === 120 && o.score === 187 && o.best === 187 && o.time === 100, JSON.stringify(o));
check('the history entry follows', d.history.find((h) => h.key === 'p:you (other)')?.pts === 187);
check('the other tab\'s match survives', d.players['p:zed']?.all.matches === 1);
check('the bot is counted once it is out', d.players['b:bot']?.all.matches === 1);
// nothing changed: no further writes
const before = store.get('fightnight.leaderboard');
lb.tick(1);
check('an unchanged board writes nothing', store.get('fightnight.leaderboard') === before);
console.log(fails ? `${fails} FAILED` : 'all passed');
process.exit(fails ? 1 : 0);
