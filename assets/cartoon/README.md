# Fightnight cartoon asset pack

Original models authored locally in Blender 5.1, using the supplied screenshots
as an art direction reference. No Fortnite meshes or textures are included.
Generation cost: zero; no API key, cloud credit or paid add-on is required.
All assets are covered by this repository's MIT license.

- `fightnight-cartoon.blend`: editable character, skeleton, actions and prop library.
- `scout.glb`, `ranger.glb`, `pilot.glb`, `vanguard.glb`: four outfit palettes,
  each with a weighted skeleton and Idle, Walk, Sprint, Jump, Crouch, Reload and Victory clips.
- `items.glb`: named weapon, consumable and chest assemblies.
- `foliage.glb`: named trees, bush and curved grass assemblies.
- `meshes/*.fnmesh`: 34 meshes baked to the existing GPU vertex layout.
- `locomotion.fnmotion`: cyclic keyframes shared by the Blender walk/sprint actions
  and the live gameplay rig. Runtime weapon IK, physics and action blends remain active.
- `manifest.json`: generator version, triangle counts and byte budgets.

The solo menu offers four outfit palettes that share one body mesh and differ in clothing palettes. The game
also supports three hair meshes. Each runtime mesh stays under 5,000 vertices;
existing distant tree LODs keep the wider island affordable to render.

Rebuild from the repository root:

```powershell
& 'C:\Program Files\Blender Foundation\Blender 5.1\blender.exe' --background --factory-startup --python tools/assetgen/build_assets.py
python tools/assetgen/verify_assets.py
scripts/build.ps1
```

FNM1: magic + little-endian u32 vertex/index counts, then 32-byte vertices
(position 3xf32, normal 3xf32, sRGB/tint 4xu8, AO/material/wind/specular 4xu8)
and u32 indices. Coordinates are metres, Y up, forward -Z. FNA1 stores a count
followed by six f32 channels per cyclic sample. Both formats are embedded into
WASM, avoiding extra model downloads during play. GLBs use standard glTF 2.0.
