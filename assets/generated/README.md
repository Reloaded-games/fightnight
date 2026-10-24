# Fightnight free 3D model library

Original stylized battle-royale models, generated locally at zero service cost.
The delivered library contains 144 individual GLBs plus one island scene with
10,541 placed objects. The count includes reusable body parts, LODs, primitives
and brick-mode variants; there are four complete rigged outfit variants.
Bold silhouettes, saturated colors, chunky equipment and pastoral island scenery
follow the game's Fortnite-inspired art direction. These are procedural Blender
and Rust models, not assets generated through a cloud AI service or extracted
from Fortnite. All included models use the repository's MIT license.

## Contents

| Folder | Contents |
| --- | --- |
| `characters` | Scout, Ranger, Pilot and Vanguard: four palette variants of one rigged body, 16 bones, nine animation clips each |
| `character-parts` | Individual body segments, three hairstyles and four headwear options, at their runtime rig pivots |
| `weapons` | Six guns, pickaxe, ammo and rocket projectile |
| `equipment` | Healing/shield items, hinged chest parts, supply crate/parachute and glider |
| `vehicles` | Buggy, roadster, wheel and balloon bus; chassis and wheels are separate for animation |
| `environment` | Trees with distant LODs, bushes, grass, flowers, rocks, stump, hay, barrel, crate and windmill blades |
| `buildings` | 17 house, shop, cabin, farm, landmark and street-prop models, including two-story house, barn, lighthouse, dock and fence |
| `construction` | Wall, floor, ramp and roof |
| `brick` | Existing original brick-toy mode variants |
| `primitives` | Shared geometric primitives |
| `map/island.glb` | Seed 1234 island: terrain, towns, roads painted into vertex colors, water and placed vegetation/props |

`manifest.json` records every output, size and SHA-256. GLBs are self-contained
glTF 2.0 files with vertex colors and PBR material categories, in metres, Y up.
They can be imported into Blender or a glTF-capable game engine. Character
forward is -Z; buildings retain their existing entrance on +Z.

The island is an editable **static overview** sampled at 6m; the game continues
to use its 3m terrain grid. The export approximates procedural materials with
vertex colors and quantizes foliage tints to reduce size. It does not contain
gameplay collision, AI, wind animation, dynamic loot, water animation or the
game's procedural shader details. Individual character parts need the game's
rig assembly; use `characters/*.glb` for complete animated characters.

## Rebuild

Run from the repository root with Blender, Rust and Python available:

```powershell
& 'C:\Program Files\Blender Foundation\Blender 5.1\blender.exe' --background --factory-startup --python tools/assetgen/build_assets.py
python tools/assetgen/verify_assets.py
cargo run --release -p fn-core --example export_assets -- target/asset-export 1234
python tools/assetgen/package_assets.py
```

Optional standards validation and preview generation:

```powershell
npm install --prefix target/gltf-validation --no-save --package-lock=false gltf-validator
node tools/assetgen/validate_glb.mjs
& 'C:\Program Files\Blender Foundation\Blender 5.1\blender.exe' --background --factory-startup --python tools/assetgen/render_library.py -- buildings
& 'C:\Program Files\Blender Foundation\Blender 5.1\blender.exe' --background --factory-startup --python tools/assetgen/render_library.py -- island
python tools/assetgen/package_assets.py --archive-only
```

The ZIP is written to `dist-assets/fightnight-free-models.zip`. It includes this
library, the editable `source/fightnight-cartoon.blend`, and Python generator
sources. Rebuilding the entire library requires this repository for the Rust
world generators. The game's eight new Blender environment meshes are embedded
in the runtime; no model fetches or paid services are needed during play.

## Free generation options checked on 2026-10-24

| Option | Free capability and limitation | Source |
| --- | --- | --- |
| Meshy | 100 monthly credits; ten Meshy 6 Lite downloads/month. Free outputs require CC BY 4.0 attribution. Other model versions can require payment to download. | [Official free-plan guide](https://help.meshy.ai/en/articles/15696428-what-is-included-on-the-free-plan) |
| Tripo | Pricing documentation lists 200 monthly credits and H2.5-only free exports. Its pricing and commercial-use help pages disagree about free commercial rights, so no Tripo assets are included. | [Pricing](https://www.tripo3d.ai/product-doc/getting-started/pricing-credits), [commercial-use help](https://www.tripo3d.ai/help/privacy-policy/how-to-use-tripo-models-commercially) |
| Hunyuan3D 2.1 | Local image-to-3D pipeline; published requirements list 10GB VRAM for shape and 29GB for shape plus texture. Requires a substantial ML environment. Not run for this pack. | [Official repository](https://github.com/Tencent-Hunyuan/Hunyuan3D-2.1) |
| Kenney | 145 free CC0 prototype assets, including characters, buildings and vehicles; ready-made assets rather than AI generation. Not included in this original pack. | [Prototype Kit](https://kenney.nl/assets/prototype-kit) |

The locally installed Blender pipeline was used because it can generate the
whole compatible pack without accounts, download quotas or usage charges.
