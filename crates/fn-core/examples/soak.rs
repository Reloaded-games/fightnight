#![allow(clippy::manual_is_multiple_of)] // `is_multiple_of` needs a newer compiler than the minimum supported one
//! Run a bots-only match headlessly and print a timeline:
//! `cargo run -p fn-core --release --example soak -- <bots> <seconds> <storm_speed> [bus]`
use fn_core::game::*;
use fn_core::world::World;
use std::collections::BTreeMap;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let bots: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(39);
    let seconds: f32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(300.0);
    let speed: f32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(1.0);
    let bus = args.get(4).is_some_and(|s| s == "bus");
    let follow: Option<usize> = args.get(5).and_then(|s| s.parse().ok());
    let t0 = std::time::Instant::now();
    let world = World::generate(1234);
    println!("world {:?}", t0.elapsed());
    let mut g = Game::new(world, GameConfig { bots, skip_bus: !bus, seed: 5, storm_speed: speed, god_mode: true, ..Default::default() });
    let t0 = std::time::Instant::now();
    let inp = PlayerInput::default();
    let mut t = 0.0f32;
    let mut next = 0.0f32;
    let mut frames = 0u32;
    let mut worst = 0.0f64;
    let mut pickups = 0usize;
    let mut chests = 0usize;
    let mut shots: BTreeMap<String, (u32, u32)> = BTreeMap::new();
    while t < seconds && g.phase != Phase::Over {
        let f0 = std::time::Instant::now();
        g.update(1.0 / 30.0, &inp);
        worst = worst.max(f0.elapsed().as_secs_f64());
        frames += 1;
        for e in &g.events {
            match e {
                fn_core::game::events::Event::Pickup { actor, .. } if *actor != 0 => pickups += 1,
                fn_core::game::events::Event::ChestOpen { .. } => chests += 1,
                fn_core::game::events::Event::Shot { actor, weapon, hit_actor, .. } if *actor != 0 => {
                    let e = shots.entry(format!("{weapon:?}")).or_default();
                    e.0 += 1;
                    if *hit_actor {
                        e.1 += 1;
                    }
                }
                _ => {}
            }
        }
        t += 1.0 / 30.0;
        if let Some(f) = follow {
            if (t * 30.0) as u32 % 30 == 0 {
                let a = &g.actors[f];
                println!("  [{t:5.1}] bot{f} hp={:.0}/{:.0} pos=({:.1},{:.2},{:.1}) vel=({:.1},{:.1}) yaw={:.2} onground={} mode={:?} w={} {}", a.hp, a.shield, a.pos.x, a.pos.y, a.pos.z, a.vel.x, a.vel.z, a.yaw, a.on_ground, a.mode, a.inv.weapon_count(), a.brain.as_ref().map(|b| b.debug()).unwrap_or_default());
            }
        }
        if t >= next {
            next += 10.0;
            let mut goals: BTreeMap<&str, usize> = BTreeMap::new();
            let mut modes: BTreeMap<String, usize> = BTreeMap::new();
            for a in g.actors.iter().skip(1).filter(|a| a.alive) {
                *modes.entry(format!("{:?}", a.mode)).or_default() += 1;
                if let Some(b) = &a.brain {
                    *goals.entry(b.goal_name()).or_default() += 1;
                }
            }
            let kills: u32 = g.actors.iter().map(|a| a.kills).sum();
            let weapons: usize = g.actors.iter().skip(1).filter(|a| a.alive).map(|a| a.inv.weapon_count()).sum();
            let alive = g.actors.iter().skip(1).filter(|a| a.alive).count();
            let dealt: f32 = g.actors.iter().map(|a| a.damage_dealt).sum();
            println!(
                "t={t:5.0} pickups={pickups:3} chests={chests:2} alive={alive:2} kills={kills:2} dmg={dealt:5.0} weapons/bot={:.1} storm(phase {} r={:.0} {:?}) pieces={} modes={modes:?} goals={goals:?}",
                weapons as f32 / alive.max(1) as f32,
                g.storm.phase,
                g.storm.radius,
                g.storm.state,
                g.pieces.count()
            );
        }
    }
    for (k, (n, h)) in &shots {
        println!("  shots {k:<16} {n:5} hit {h:5} ({:.0}%)", *h as f32 * 100.0 / (*n).max(1) as f32);
    }
    let by_storm = g.feed.iter().filter(|f| f.storm).count();
    println!("done: t={t:.0}s phase={:?} winner={:?} feed-storm-deaths(last 7s)={by_storm}", g.phase, g.winner);
    let mut dead_by_storm = 0;
    let _ = &mut dead_by_storm;
    println!("cpu: {:?} for {frames} frames = {:.2} ms/frame avg, worst {:.1} ms", t0.elapsed(), t0.elapsed().as_secs_f64() * 1000.0 / frames.max(1) as f64, worst * 1000.0);
    let mut rows: Vec<_> = g.actors.iter().collect();
    rows.sort_by_key(|a| std::cmp::Reverse(a.kills));
    for a in rows.iter().take(6) {
        println!("  {:<16} kills {} dmg {:.0} placement {} alive {} weapons {} mats {:?}", a.name, a.kills, a.damage_dealt, a.placement, a.alive, a.inv.weapon_count(), a.inv.mats);
    }
}
