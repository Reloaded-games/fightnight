# FIGHT NIGHT

A complete, playable **Fortnite-style battle royale that runs in your browser**, written in
**Rust → WebAssembly** and rendered with **WebGPU**. Everything you see and hear is generated in code:
the island, towns, trees, characters, weapons, UI icons and every sound effect. There are no image,
model or audio files.

* Drop from the **Battle Bus** (hot-air balloon and all), skydive and glide onto the island.
* **Third-person shoulder camera**, smooth movement, sprint / crouch / jump / swim, **aim down sights**,
  weapon bloom and recoil, **reloading**, headshots and damage falloff.
* **39 enemy bots** (adjustable) that land, loot, heal, build cover, fight and run from the storm.
* **Loot**: chests, floor weapons with rarity beams (common → legendary), ammo, bandages, medkits, shield
  potions, Chug Jugs. 5 hotbar slots + pickaxe, health + shield, materials.
* **Building**: walls, floors, ramps and roofs on a Fortnite-style grid in wood, stone and metal — ramp-rush
  up cliffs, box yourself in, and watch bullets and rockets chew through your pieces.
* **Harvesting**: hit trees and rocks with the pickaxe for materials (trees really fall over).
* **Shrinking storm** in 7 phases that forces everyone together, with a minimap, full map and storm timer.
* A colourful island with grassy hills, forests, lakes, a snowy mountain, beaches and nine named towns.
* Fortnite-flavoured lighting (cascaded shadow maps, bloom, ACES grading, soft sky and clouds), procedural
  character animation (run/crouch/air/swim/glide cycles, IK-held weapons, reload and swing animations, a
  dance emote and a victory dance) and a Canvas2D HUD in the same visual language (compass, minimap, skewed
  health/shield bars, rarity-coloured hotbar, kill feed, hit markers, damage numbers, victory screen with
  confetti).

## Screenshots

| | |
| --- | --- |
| ![Main menu](docs/screenshots/menu.jpg) | ![The Battle Bus over the island](docs/screenshots/bus.jpg) |
| ![A firefight in Maple Meadows](docs/screenshots/town.jpg) | ![Lazy Lagoon](docs/screenshots/lake.jpg) |
| ![Building a ramp](docs/screenshots/build.jpg) | ![Standing next to the storm wall](docs/screenshots/storm.jpg) |
| ![Inventory](docs/screenshots/inventory.jpg) | ![Island map](docs/screenshots/map.jpg) |

(Regenerate them with `tools/screenshots.sh`.)

## Play

You need a browser with WebGPU (Chrome / Edge 113+, Safari 18+, recent Firefox) and a GPU.

```bash
# one-time setup
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129   # must match the wasm-bindgen crate version

# build the game into ./dist and serve it
scripts/build.sh                       # release build (use `scripts/build.sh dev` for a fast debug build)
python3 -m http.server -d dist 8080    # or any static file server
# open http://localhost:8080
```

`dist/` is a plain static site, so any static host works (WebGPU needs HTTPS or `localhost`). The repository
ships a CI workflow (tests, clippy, wasm build) and a manual **Deploy to GitHub Pages** workflow: enable
*Settings → Pages → Source: GitHub Actions*, then run it from the Actions tab. Building needs Rust 1.87 or newer.

### Controls

| Key / mouse | Action |
| --- | --- |
| `W A S D` / mouse | Move / look |
| `Shift` (auto-sprint on by default) | Sprint |
| `Space` | Jump · leave the bus · open the glider |
| `Ctrl` or `F` | Crouch (browsers reserve Ctrl+W and Ctrl+1–6, so `F` is safer; fullscreen captures them too) |
| Left mouse | Fire · swing the pickaxe · place the selected piece |
| Right mouse | Aim down sights |
| `R` | Reload |
| `E` | Pick up / open chest |
| `1`–`6` / wheel | Select slot (1 is the pickaxe) |
| `G` | Drop the selected item |
| `Q` | Toggle build mode |
| `Z` `X` `C` `V` | Wall · Floor · Ramp · Roof |
| `T` | Cycle building material (wood / stone / metal) |
| `B` | Dance emote (any other action ends it; the winner dances automatically) |
| `Tab` | Inventory |
| `M` | Map |
| `Esc` | Pause |
| `F3` | Performance overlay |

Tips: hold forward and click with a ramp selected to ramp-rush up a cliff; walls block bullets, so build
cover before you heal; the next safe circle is the dashed white ring on the map.

## How it is put together

```
crates/
  fn-core/     platform-independent game: math, noise, world generation, simulation, bots, models,
               procedural audio synthesis, input mapping, shadow cascades, HUD/audio mapping. Compiles natively,
               so almost everything is unit tested without a browser.
    src/world/   island: terrain + biomes, lakes, roads, towns and buildings, props, colliders, nav grid, minimap
    src/game/    actors, movement/physics, combat, loot, building pieces, storm + bus, bot AI, rig (animation),
                 scene (draw lists), fx (particles), hud (JSON snapshot), audio_map (positional cues)
    src/models.rs, meshlib.rs   every mesh in the game (characters, weapons, items, trees, building pieces...)
    src/audio_synth.rs          all sound effects synthesised from oscillators and noise
  fn-shaders/  WGSL shaders (+ a naga validation test so a shader typo fails `cargo test`)
  fn-web/      the wasm module: wgpu WebGPU renderer, input, app loop, and the exports used by the page
web/           index.html, CSS and the JS for menus, HUD drawing (Canvas2D), icons and WebAudio playback
scripts/       build script (cargo → wasm-bindgen → dist/)
tools/         headless-Chromium helpers: screenshots and the end-to-end tests
```

Rendering notes: reverse-Z infinite projection into an HDR (RGBA16F) MSAA target, 3-cascade shadow maps,
instanced meshes batched per model (a few hundred draw calls), CPU-culled props and grass, procedural
terrain/sky/water/storm shaders, bloom + FXAA + ACES composite. Quality presets (Low / Medium / High) trade
resolution scale, MSAA, shadow resolution, bloom and draw distance.

The simulation runs at a fixed ≤1/60 s step shared by the player and bots (bots only produce an `Intent`,
exactly like the keyboard does), so the same rules apply to everyone.

## Tests

```bash
cargo test --release                       # ~170 unit/integration tests (simulation, bots, models, input, shaders, ...)
cargo test --release -- --ignored monkey   # long randomised "monkey" soak test over many seeds
cargo run --release -p fn-core --example soak -- 39 700 1 bus   # a bots-only match with a timeline
cd tools && npm install && npm test       # real-browser tests (needs `scripts/build.sh` first):
                                           #   e2e.mjs      gameplay: move, shoot, reload, build, harvest, chests, kills...
                                           #   e2e_flow.mjs UI state machine with the real pointer lock: pause, win, spectate...
```

`tools/` also has `ui_shot.mjs` / `game_shot.mjs` for scripted screenshots (the WebGPU frame is read back
explicitly because headless Chromium cannot screenshot a WebGPU canvas). The page exposes a small debug
console (`window.__game.fn.debug("tp 52 28")`, `give ar epic`, `chest_here`, `build_demo`, `ff 120`, ...) that
the harnesses use.

## Licence

MIT.
