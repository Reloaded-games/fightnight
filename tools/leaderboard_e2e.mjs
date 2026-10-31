// End-to-end test of the leaderboard: a won match and a quit match land on the career board with the right points, the end
// screen shows the match's points and standings (beside the result without covering it, or under it with room to scroll), a
// counted result that still changes is brought up to date, bots only show when asked, the columns sort and the board resets.
//   node leaderboard_e2e.mjs            (needs `scripts/build.sh` first)
import { serve, launch } from './browser.mjs';

const { srv, url } = await serve();
// (wide enough for the standings to sit beside the result; software rendering gets slow on bigger windows)
const { browser, page, logs } = await launch({ width: 1280, height: 720 });
let failed = 0;
const check = (name, ok, extra = '') => { console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${extra ? '  ' + extra : ''}`); if (!ok) failed++; };
const ev = (f, arg) => page.evaluate(f, arg);
const until = (f, arg, timeout = 240000) => page.waitForFunction(f, arg, { timeout });
const active = (id) => ev((id) => document.getElementById(id).classList.contains('active'), id);
const text = (sel) => ev((sel) => document.querySelector(sel)?.textContent || '', sel);
const debug = (c) => ev((c) => window.__game.fn.debug(c), c);
const stored = () => ev(() => JSON.parse(localStorage.getItem('fightnight.leaderboard') || 'null'));

const OPTS = encodeURIComponent('skipbus=1;bots=10;god=1;map=island');
await page.goto(url + 'index.html?nolock=1&opts=' + OPTS);
await until(() => window.__game && window.__game.state === 'menu');
await ev(() => { localStorage.removeItem('fightnight.leaderboard'); localStorage.removeItem('fightnight.leaderboard.view'); });
await page.fill('#opt-name', 'Ada');

// ---- an empty board ---------------------------------------------------------------------------------------------------------
await page.click('#btn-leaderboard');
check('the menu opens the leaderboard', await active('leaderboard'));
check('an empty board says how to get on it', /No finished matches yet/.test(await text('#leaderboard-body')));
check('the scoring rules are spelled out', /25 per elimination/.test(await text('#leaderboard-rules')));
await page.keyboard.press('Escape');
check('Esc closes it', !(await active('leaderboard')));

// ---- a won match ------------------------------------------------------------------------------------------------------------
await page.click('#btn-play');
await until(() => window.__game.state === 'playing');
await debug('stats 3 200 450'); // 3 eliminations, 450 damage
await debug('elim 2 1'); // bot 1 takes out bot 2: a finished bot result
await until(() => window.__game.leaderboard.board?.rows.some((r) => r.id === 2 && r.done), null, 60000);
const sb = await ev(() => JSON.parse(window.__game.fn.scoreboard()));
check('the scoreboard lists everyone in the match', sb.rows.length === 11 && sb.total === 11, `${sb.rows.length} rows`);
await debug('kill_bots');
await until(() => window.__game.state === 'over', null, 120000);
await page.waitForTimeout(300);
const points = await text('#over-points');
check('the end screen shows the points of the match', /\+220\s*points/i.test(points), points);
check('and how they add up', /75 elims/.test(points) && /45 damage/.test(points) && /50 placement/.test(points) && /50 victory/.test(points), points);
check('and the career total and rank', /Career 220 points/.test(points) && /1st of 1/.test(points), points);
const standings = await text('#over-board');
check('the match standings list the winner first', /Match standings/.test(standings) && (await ev(() => document.querySelector('#over-board tbody tr')?.classList.contains('me'))), standings.slice(0, 120));
let d = await stored();
const ada = d?.players['p:ada'];
check('the win is stored', ada && ada.all.matches === 1 && ada.all.wins === 1 && ada.all.kills === 3 && ada.all.score === 220 && ada.all.damage === 450, JSON.stringify(ada?.all));
check('under the mode it was played in', ada?.modes['battle-royale']?.score === 220);
const bots = Object.entries(d?.players || {}).filter(([k]) => k.startsWith('b:'));
check('the eliminated bot is stored apart from the people', bots.length === 1 && bots[0][1].all.matches === 1 && bots[0][1].bot === true, JSON.stringify(bots.map(([k]) => k)));
check('the recent matches keep the result', d?.history.length === 1 && d.history[0].won && d.history[0].pts === 220 && d.history[0].mode === 'battle-royale');

// ---- the board ------------------------------------------------------------------------------------------------------------------
await page.click('#btn-over-leaderboard');
check('the end screen opens the leaderboard on top', await active('leaderboard'));
let rows = await ev(() => [...document.querySelectorAll('#leaderboard-body tbody tr')].map((r) => r.textContent));
check('people only by default', rows.length === 1 && /Ada/.test(rows[0]) && /220/.test(rows[0]) && /you/i.test(rows[0]), JSON.stringify(rows));
check('the player card shows the stats', /Victory Royales/.test(await text('#leaderboard-detail')) && /Accuracy/.test(await text('#leaderboard-detail')) && /Recent matches/.test(await text('#leaderboard-detail')));
await page.click('#leaderboard-bots');
rows = await ev(() => [...document.querySelectorAll('#leaderboard-body tbody tr')].map((r) => r.textContent));
check('Show bots adds them, ranked below', rows.length === 2 && /Ada/.test(rows[0]) && /bot/i.test(rows[1]), JSON.stringify(rows));
await page.click('#leaderboard-body th[data-sort="kills"]');
check('a column header sorts by it', await ev(() => document.querySelector('#leaderboard-body th[data-sort="kills"]').classList.contains('on')));
await page.click('#leaderboard-body tbody tr:nth-child(2)');
check('clicking a row shows that player', /bot/i.test(await text('#leaderboard-detail .lb-who')));
await page.click('#leaderboard-modes button[data-v="zero-build"]');
check('a mode with no matches is empty', /Nobody has finished a match in this mode yet/.test(await text('#leaderboard-body')));
await page.click('#leaderboard-modes button[data-v="all"]');
await page.click('#leaderboard-close');

// ---- walking out of a match counts as going out there ------------------------------------------------------------------
await page.click('#btn-again');
await until(() => window.__game.state === 'playing');
await page.waitForTimeout(800);
await debug('stats 1 60 120');
await page.waitForTimeout(700);
await page.keyboard.press('Escape');
await until(() => window.__game.state === 'paused', null, 60000);
await page.click('#btn-quit');
await until(() => window.__game.state === 'menu', null, 60000);
d = await stored();
const ada2 = d?.players['p:ada']?.all;
// 11 still standing out of 11: last place, so 1 elimination (25) and 120 damage (12)
check('quitting a match you are in counts as a loss', ada2 && ada2.matches === 2 && ada2.wins === 1 && ada2.quits === 1 && ada2.kills === 4 && ada2.score === 220 + 37, JSON.stringify(ada2));
check('and is marked in the recent matches', d.history[0].quit === true && d.history[0].place === 11, JSON.stringify(d.history[0]));

// ---- an eliminated player's result is counted at once, and kept up to date ------------------------------------------------
await page.click('#btn-play');
await until(() => window.__game.state === 'playing');
await debug('kill_me');
await until(() => (JSON.parse(localStorage.getItem('fightnight.leaderboard')) || { players: {} }).players['p:ada']?.all.matches === 3, null, 120000);
check('an elimination counts before the end screen shows', true);
// a grenade thrown before falling can still score: the counted result follows
await debug('stats 2 100 300');
await until(() => (JSON.parse(localStorage.getItem('fightnight.leaderboard')) || { players: {} }).players['p:ada']?.all.kills === 6, null, 120000);
d = await stored();
const ada3 = d.players['p:ada'].all;
check('a counted result that changes is updated, not counted again', ada3.matches === 3 && ada3.kills === 6 && ada3.damage === 570 + 300 && d.history[0].kills === 2 && d.history[0].damage === 300, JSON.stringify({ all: ada3, last: d.history[0] }));
check('and its points with it', ada3.score === 257 + d.history[0].pts && d.history[0].pts === 2 * 25 + 30, JSON.stringify(d.history[0]));
await until(() => window.__game.state === 'over', null, 180000);
await page.waitForTimeout(300);
await ev(() => { document.getElementById('gpu').style.visibility = 'hidden'; }); // the layout checks below need no frames
const layout = () => ev(() => {
  const r = (id) => document.getElementById(id).getBoundingClientRect();
  const hit = (a, b) => a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom;
  const board = r('over-board');
  return { place: hit(r('over-place'), board), stats: hit(r('over-stats'), board), banner: Math.round(r('over-banner').top), board: Math.round(board.width) };
});
for (const [w, h] of [[1280, 720], [1366, 768], [1600, 900]]) {
  await page.setViewportSize({ width: w, height: h });
  const l = await layout();
  check(`at ${w}x${h} the standings sit beside the result without covering it`, !l.place && !l.stats && l.banner >= 0 && l.board > 0, JSON.stringify(l));
}
await page.setViewportSize({ width: 960, height: 540 });
const narrow = await layout();
const scrolls = await ev(() => { const o = document.getElementById('over'); return o.scrollHeight > o.clientHeight && getComputedStyle(o).overflowY === 'auto'; });
check('on a small window they go under it and the screen scrolls instead of cutting the top off', !narrow.place && narrow.banner >= 0 && scrolls, JSON.stringify(narrow));
await page.setViewportSize({ width: 1280, height: 720 });
await ev(() => { document.getElementById('gpu').style.visibility = ''; });
await page.click('#btn-menu');
await until(() => window.__game.state === 'menu');

// ---- reset ----------------------------------------------------------------------------------------------------------------------
await page.click('#btn-leaderboard');
check('the board remembers the bots choice', await ev(() => document.getElementById('leaderboard-bots').checked));
await page.click('#leaderboard-reset');
check('reset asks for a second click', /again/i.test(await text('#leaderboard-reset')) && (await stored()).history.length === 3);
await page.click('#leaderboard-reset');
check('the second click erases everything', /No finished matches yet/.test(await text('#leaderboard-body')) && Object.keys((await stored()).players).length === 0);
await page.click('#leaderboard-close');

const bad = logs.filter((l) => /error|panick/i.test(l) && !/WebGPU is experimental|favicon|404|Failed to load resource/.test(l));
check('no errors in the console', bad.length === 0, bad.join('\n'));
await browser.close(); srv.close();
console.log(failed ? `${failed} FAILED` : 'all passed');
process.exit(failed ? 1 : 0);
