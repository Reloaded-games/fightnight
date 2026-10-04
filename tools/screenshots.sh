#!/usr/bin/env bash
# Regenerates the README screenshots in docs/screenshots (software WebGPU, so it takes a few minutes).
# Needs `scripts/build.sh` and `npm install` in tools/ first.
set -euo pipefail
cd "$(dirname "$0")"
out=../docs/screenshots
mkdir -p "$out"
shot() { local name=$1; shift; echo "== $name"; node ui_shot.mjs "$out/$name.jpg" --w 1280 --h 720 "$@" >/dev/null; }

shot menu --state menu --wait 2500
shot bus --state menu --click "#btn-play" --wait 6000
shot town --state playing --opts "skipbus=1;bots=20;god=1" --cmd "tp -10 40|look 20 -3|bots_near 3 16|give ar epic" --wait 2000
shot lake --state playing --opts "skipbus=1;bots=12;god=1" --cmd "tp 21 -171|look 90 -8|give smg epic" --wait 2000
shot build --state playing --opts "skipbus=1;bots=12;god=1" --cmd "tp -10 40|look 20 -3|mats 500|build_demo" --keys "KeyQ:100,KeyZ:100" --wait 2500
shot storm --state playing --opts "skipbus=1;bots=12;god=1" --cmd "tp 21 -171|storm_set 21 -230 70|look 180 -2|give ar rare" --wait 2500
shot scope --state playing --quality low --opts "skipbus=1;bots=12;god=1" --cmd "tp 21 -171|look 90 0|give sniper epic" --keys "+Mouse2:9000" --wait 300
shot victory --state playing --quality low --opts "skipbus=1;bots=12;god=1" --cmd "tp -10 40|stats 7 566 1184|kill_bots" --wait 16000
shot inventory --state playing --opts "skipbus=1;bots=12;god=1" --cmd "tp -10 40|give shotgun legendary|give smg rare|give sniper epic|give ar epic" --keys "Tab:100" --wait 1500
shot map --state playing --opts "skipbus=1;bots=12;god=1" --cmd "tp -10 40" --keys "KeyM:100" --wait 1500
echo "wrote $(ls "$out" | wc -l) screenshots to docs/screenshots"
