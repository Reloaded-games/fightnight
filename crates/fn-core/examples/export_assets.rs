//! Export the exact game geometry for the zero-cost GLB packaging tool.
//! cargo run --release -p fn-core --example export_assets -- target/asset-export 1234
use fn_core::math::*;
use fn_core::mesh::{hex, mat, MeshBuilder, MeshData, Vertex};
use fn_core::meshlib::{build_mesh, ALL_IDS};
use fn_core::rng::Rng;
use fn_core::world::{buildings::*, World, GRID_N, WORLD_HALF};
use std::{
    fs,
    io::{self, Write},
    path::Path,
};

fn write_mesh(root: &Path, name: &str, mesh: &MeshData) -> io::Result<()> {
    let mut f = fs::File::create(root.join(format!("{name}.fnmesh")))?;
    f.write_all(b"FNM1")?;
    f.write_all(&(mesh.verts.len() as u32).to_le_bytes())?;
    f.write_all(&(mesh.idx.len() as u32).to_le_bytes())?;
    for v in &mesh.verts {
        for n in v.pos.into_iter().chain(v.nrm) {
            f.write_all(&n.to_le_bytes())?;
        }
        f.write_all(&v.col)?;
        f.write_all(&v.attr)?;
    }
    for i in &mesh.idx {
        f.write_all(&i.to_le_bytes())?;
    }
    Ok(())
}

fn main() -> io::Result<()> {
    let args: Vec<_> = std::env::args().collect();
    let root = Path::new(
        args.get(1)
            .map(String::as_str)
            .unwrap_or("target/asset-export"),
    );
    let seed = args
        .get(2)
        .map(|v| v.parse::<u32>().expect("seed must be a u32"))
        .unwrap_or(1234);
    fs::create_dir_all(root)?;
    let mut catalog = fs::File::create(root.join("catalog.tsv"))?;
    for &id in ALL_IDS {
        let name = format!("{id:?}");
        let category = if name.starts_with("Lego") {
            "brick"
        } else if name.starts_with("Char")
            || name.starts_with("Hair")
            || ["Cap", "Beanie", "Helmet", "Hat"].contains(&name.as_str())
        {
            "character-parts"
        } else if name.starts_with("Wpn")
            || ["Pickaxe", "Missile", "AmmoBox"].contains(&name.as_str())
        {
            "weapons"
        } else if name.starts_with("Vehicle") || name == "Bus" {
            "vehicles"
        } else if name.starts_with("Item")
            || name.starts_with("Chest")
            || name.starts_with("Supply")
            || name == "Glider"
        {
            "equipment"
        } else if name.starts_with("Piece") {
            "construction"
        } else if name.starts_with("Unit") {
            "primitives"
        } else {
            "environment"
        };
        write_mesh(root, &name, &build_mesh(id))?;
        writeln!(catalog, "{category}\t{name}")?;
    }
    let mut rng = Rng::new(seed as u64);
    let one = random_style(&mut rng, 1);
    let two = random_style(&mut rng, 2);
    let buildings = vec![
        ("House", gen_house(&mut rng, 10., 8., &one)),
        ("HouseTwoStory", gen_house(&mut rng, 10., 8., &two)),
        ("Cabin", gen_cabin(&mut rng, 9., 7.)),
        ("Shop", gen_shop(&mut rng, 12., 10.)),
        ("Barn", gen_barn(&mut rng)),
        ("Windmill", gen_windmill()),
        ("Lighthouse", gen_lighthouse()),
        ("WaterTower", gen_water_tower()),
        ("Silo", gen_silo()),
        ("GasCanopy", gen_gas_canopy()),
        ("Fountain", gen_fountain()),
        ("Well", gen_well()),
        ("LampPost", gen_lamp_post(0x394f63)),
        ("Bench", gen_bench(0.)),
        ("Planter", gen_planter(0xe68692)),
        ("Dock", gen_dock(12.)),
        ("Fence", gen_fence(6., 0xeee5ce)),
    ];
    for (name, g) in buildings {
        write_mesh(root, name, &g.mesh)?;
        writeln!(catalog, "buildings\t{name}")?;
    }
    let world = World::generate(seed);
    let mut scene = fs::File::create(root.join("island.tsv"))?;
    let mut add = |name: &str, p: Vec3, yaw: f32, scale: f32, tint: [f32; 3]| -> io::Result<()> {
        writeln!(
            scene,
            "{name}\t{}\t{}\t{}\t{yaw}\t{scale}\t{}\t{}\t{}",
            p.x, p.y, p.z, tint[0], tint[1], tint[2]
        )
    };
    // Six-metre grid for an editable overview; gameplay retains its 3m grid.
    let mut terrain = MeshData::default();
    let n = (GRID_N - 1) / 2 + 1;
    for z in 0..n {
        for x in 0..n {
            let p = Vec3::new(
                world.hm.world_x(x * 2),
                world.hm.get(x * 2, z * 2),
                world.hm.world_x(z * 2),
            );
            let mut color = world
                .painter
                .color(p.x, p.z, p.y, world.hm.slope_at(p.x, p.z));
            let splat = world.splat.sample(Vec2::new(p.x, p.z));
            color = color.lerp(hex(0xb59a72), splat[0]);
            color = color.lerp(hex(0x545968), splat[1]);
            terrain.verts.push(Vertex {
                pos: p.to_array(),
                nrm: world.hm.normal_at(p.x, p.z).to_array(),
                col: fn_core::mesh::to_srgb8(color),
                attr: [255, mat::TERRAIN, 0, 0],
            });
            if x + 1 < n && z + 1 < n {
                let i = (z * n + x) as u32;
                let row = n as u32;
                terrain
                    .idx
                    .extend_from_slice(&[i, i + row, i + row + 1, i, i + row + 1, i + 1]);
            }
        }
    }
    write_mesh(root, "IslandTerrain", &terrain)?;
    add("IslandTerrain", Vec3::ZERO, 0., 1., [1.; 3])?;
    for chunk in &world.chunk_meshes {
        let name = format!("TownChunk{}", chunk.chunk);
        write_mesh(root, &name, &chunk.mesh)?;
        add(&name, Vec3::ZERO, 0., 1., [1.; 3])?;
    }
    for props in &world.chunk_props {
        for p in props {
            add(
                &format!("{:?}", p.kind.mesh(0)),
                p.pos,
                p.yaw,
                p.scale,
                p.tint,
            )?;
        }
    }
    let mut water = MeshBuilder::new();
    water.mat(mat::GLASS).tinted(false).hex(0x348fb0);
    water.quad_flat(
        [
            Vec3::new(-WORLD_HALF, 0., -WORLD_HALF),
            Vec3::new(-WORLD_HALF, 0., WORLD_HALF),
            Vec3::new(WORLD_HALF, 0., WORLD_HALF),
            Vec3::new(WORLD_HALF, 0., -WORLD_HALF),
        ],
        [1.; 4],
    );
    for lake in &world.layout.lakes {
        let c = Vec3::new(lake.center.x, lake.level, lake.center.y);
        for i in 0..64 {
            let a = i as f32 * std::f32::consts::TAU / 64.;
            let b = (i + 1) as f32 * std::f32::consts::TAU / 64.;
            water.tri_flat(
                c,
                c + Vec3::new(b.cos(), 0., b.sin()) * lake.radius * 1.12,
                c + Vec3::new(a.cos(), 0., a.sin()) * lake.radius * 1.12,
            );
        }
    }
    write_mesh(root, "IslandWater", &water.finish())?;
    add("IslandWater", Vec3::ZERO, 0., 1., [1.; 3])?;
    fs::write(root.join("seed.txt"), seed.to_string())?;
    println!(
        "Exported {} shared models, 17 building models and island seed {seed} ({} buildings)",
        ALL_IDS.len(),
        world.buildings.len()
    );
    Ok(())
}
