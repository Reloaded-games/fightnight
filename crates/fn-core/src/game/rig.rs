//! Procedural character animation: turns an `Actor`'s state into world matrices for the
//! rigid body parts (see `models`), the held item and the glider. Legs and the free arm use
//! forward kinematics with run / crouch / air cycles; arms that hold an item use a two-bone IK
//! so the hands stay glued to the weapon through aiming, recoil, reloading and swapping.

use super::actor::*;
use super::items::*;
use crate::math::*;
use crate::meshlib::MeshId;
use crate::models::dims::*;
use std::f32::consts::{FRAC_PI_2, PI};

/// World matrices for every part of one character. Index 0 is the left side, 1 the right.
#[derive(Clone, Debug)]
pub struct Pose {
    pub torso: Mat4,
    pub pelvis: Mat4,
    pub head: Mat4,
    pub backpack: Mat4,
    pub arm_up: [Mat4; 2],
    pub arm_low: [Mat4; 2],
    pub hand: [Mat4; 2],
    pub leg_up: [Mat4; 2],
    pub leg_low: [Mat4; 2],
    pub boot: [Mat4; 2],
    /// The item in the right hand (grip at the model origin).
    pub item: Option<(MeshId, Mat4)>,
    pub glider: Option<Mat4>,
    /// World position of the weapon's muzzle (tracers, flashes).
    pub muzzle: Vec3,
    /// World position of the head centre.
    pub head_pos: Vec3,
}

fn rx(a: f32) -> Mat4 {
    Mat4::from_rotation_x(a)
}
fn ry(a: f32) -> Mat4 {
    Mat4::from_rotation_y(a)
}
fn rz(a: f32) -> Mat4 {
    Mat4::from_rotation_z(a)
}
fn tr(v: Vec3) -> Mat4 {
    Mat4::from_translation(v)
}

/// Matrix at `from` whose local -Y axis points at `to` (limb meshes hang along -Y). `roll_hint`
/// picks which way the limb's local +X faces.
fn limb(from: Vec3, to: Vec3, roll_hint: Vec3) -> Mat4 {
    let down = (to - from).normalize_or_zero();
    let y = if down == Vec3::ZERO { Vec3::Y } else { -down };
    let mut x = roll_hint.cross(y);
    if x.length_squared() < 1e-6 {
        x = Vec3::X.cross(y);
    }
    let x = x.normalize();
    let z = x.cross(y).normalize();
    Mat4::from_cols(x.extend(0.0), y.extend(0.0), z.extend(0.0), from.extend(1.0))
}

/// Two-bone IK. Returns the elbow position for a wrist at `target` (clamped to reach).
fn solve_elbow(sh: Vec3, target: Vec3, pole: Vec3, l1: f32, l2: f32) -> (Vec3, Vec3) {
    let d = target - sh;
    let full = d.length().max(1e-4);
    let dir = d / full;
    let dist = full.clamp(0.06, (l1 + l2) * 0.998);
    let wrist = sh + dir * dist;
    let a = (l1 * l1 - l2 * l2 + dist * dist) / (2.0 * dist);
    let h = (l1 * l1 - a * a).max(0.0).sqrt();
    let mut p = pole - dir * pole.dot(dir);
    if p.length_squared() < 1e-6 {
        p = Vec3::Y.cross(dir);
    }
    (sh + dir * a + p.normalize() * h, wrist)
}

fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Mesh and muzzle offset for a held weapon.
pub fn weapon_model(kind: WeaponKind) -> (MeshId, Vec3) {
    match kind {
        WeaponKind::Pistol => (MeshId::WpnPistol, Vec3::new(0.0, 0.045, -0.24)),
        WeaponKind::Smg => (MeshId::WpnSmg, Vec3::new(0.0, 0.03, -0.51)),
        WeaponKind::AssaultRifle => (MeshId::WpnAr, Vec3::new(0.0, 0.028, -0.78)),
        WeaponKind::Shotgun => (MeshId::WpnShotgun, Vec3::new(0.0, 0.04, -0.81)),
        WeaponKind::Sniper => (MeshId::WpnSniper, Vec3::new(0.0, 0.035, -1.07)),
        WeaponKind::RocketLauncher => (MeshId::WpnRocket, Vec3::new(0.0, 0.09, -0.79)),
    }
}

/// Where the supporting hand grabs, in weapon space.
fn foregrip(kind: WeaponKind) -> Vec3 {
    match kind {
        WeaponKind::Pistol => Vec3::new(-0.03, -0.045, 0.012),
        WeaponKind::Smg => Vec3::new(0.0, -0.045, -0.2),
        WeaponKind::AssaultRifle => Vec3::new(0.0, -0.02, -0.38),
        WeaponKind::Shotgun => Vec3::new(0.0, -0.025, -0.33),
        WeaponKind::Sniper => Vec3::new(0.0, -0.02, -0.27),
        WeaponKind::RocketLauncher => Vec3::new(0.0, -0.045, -0.3),
    }
}

pub fn consumable_model(kind: ConsumableKind) -> MeshId {
    match kind {
        ConsumableKind::Bandage => MeshId::ItemBandage,
        ConsumableKind::MedKit => MeshId::ItemMedkit,
        ConsumableKind::ShieldSmall => MeshId::ItemShieldMini,
        ConsumableKind::ShieldBig => MeshId::ItemShieldBig,
        ConsumableKind::ChugJug => MeshId::ItemChug,
    }
}

pub fn pickup_model(kind: &super::PickupKind) -> MeshId {
    match kind {
        super::PickupKind::Weapon { kind, .. } => weapon_model(*kind).0,
        super::PickupKind::Ammo { .. } => MeshId::AmmoBox,
        super::PickupKind::Consumable { kind, .. } => consumable_model(*kind),
    }
}

fn hair_mesh(style: u8) -> Option<MeshId> {
    match style {
        1 => Some(MeshId::Hair1),
        2 => Some(MeshId::Hair2),
        3 => Some(MeshId::Hair3),
        _ => None,
    }
}

pub fn hair_model(o: &Outfit) -> Option<MeshId> {
    hair_mesh(o.hair_style)
}

pub fn headgear_model(o: &Outfit) -> Option<MeshId> {
    match o.headgear {
        1 => Some(MeshId::Cap),
        2 => Some(MeshId::Beanie),
        3 => Some(MeshId::Helmet),
        4 => Some(MeshId::Hat),
        _ => None,
    }
}

struct Legs {
    thigh: [f32; 2],
    knee: [f32; 2],
    foot: [f32; 2],
}

/// Compute the pose of an actor.
pub fn pose(a: &Actor) -> Pose {
    let an = &a.anim;
    let mode = a.mode;
    let run = an.run.min(1.5);
    let ra = run.min(1.25);
    let crouch = an.crouch;
    let air = an.air;
    let phase = an.phase;
    let time = an.time;
    let grounded = matches!(mode, MoveMode::Ground);
    let swimming = matches!(mode, MoveMode::Swim);
    let freefall = matches!(mode, MoveMode::Freefall);
    let gliding = matches!(mode, MoveMode::Glide);
    let dead = matches!(mode, MoveMode::Dead);

    // ---- root: feet position and body heading -----------------------------------------------
    let mut root = tr(a.pos) * ry(a.body_yaw);
    if dead {
        // topple backwards
        let k = ease(a.dead_time * 2.8);
        root *= rx(1.35 * k);
    }

    // ---- body frame ------------------------------------------------------------------------------
    let sp = (phase).sin();
    let bob = if grounded { (0.5 - 0.5 * (2.0 * phase).cos()) * 0.03 * ra } else { 0.0 };
    let breathe = if grounded { (time * 1.7 + a.id as f32).sin() * 0.006 * (1.0 - ra.min(1.0)) } else { 0.0 };
    let mut pelvis_y = HIP_Y - 0.36 * crouch - 0.1 * an.land - bob + breathe;
    let mut lean = 0.05 + 0.16 * ra + 0.13 * an.sprint + 0.3 * crouch;
    let mut roll = -an.lean_side * 0.1;
    let mut pelvis_pos = Vec3::new(0.0, pelvis_y, 0.0);
    if freefall {
        let dive = ((-a.pitch - 0.1) / 1.1).clamp(0.0, 1.0);
        lean = 1.22 + 0.28 * dive;
        pelvis_y = 0.95;
        pelvis_pos = Vec3::new(0.0, pelvis_y, 0.0);
        roll = -an.lean_side * 0.25;
    } else if gliding {
        lean = 0.12 + (-a.pitch).clamp(-0.4, 0.8) * 0.35;
        roll = -an.lean_side * 0.35;
        pelvis_y = 0.95;
        pelvis_pos = Vec3::new(0.0, pelvis_y, 0.0);
    } else if swimming {
        lean = 1.05 + 0.1 * (time * 2.0).sin();
        pelvis_y = 0.82;
        pelvis_pos = Vec3::new(0.0, pelvis_y, 0.0);
        roll = (phase * 0.5).sin() * 0.12;
    }
    let hip_twist = if grounded { 0.12 * sp * ra } else { 0.0 };
    let d = angle_diff(a.body_yaw, a.yaw);
    let twist = if freefall || gliding || swimming { 0.0 } else { (d * 0.65).clamp(-0.9, 0.9) };
    let pelvis_m = tr(pelvis_pos) * ry(hip_twist);
    let torso_rel = rx(-lean) * rz(roll) * ry(twist - hip_twist * 0.7);
    let torso_m = pelvis_m * torso_rel;

    // ---- head ---------------------------------------------------------------------------------------
    let head_yaw = if freefall || gliding || swimming { 0.0 } else { (d - twist - hip_twist).clamp(-1.1, 1.1) };
    let head_pitch = if freefall { (lean - 0.2).min(1.2) + a.pitch * 0.3 } else { (a.pitch + lean * 0.8).clamp(-0.9, 0.9) * 0.8 };
    let head_m = torso_m * tr(Vec3::new(0.0, NECK_Y, 0.0)) * ry(head_yaw) * rx(head_pitch.clamp(-1.2, 1.2));

    // ---- legs ----------------------------------------------------------------------------------------------
    let mut legs = Legs { thigh: [0.0; 2], knee: [0.0; 2], foot: [0.0; 2] };
    for i in 0..2 {
        let side_phase = phase + if i == 0 { 0.0 } else { PI };
        let mut thigh = 0.0;
        let mut knee = 0.0;
        if grounded {
            let s = side_phase.sin();
            let c = side_phase.cos();
            let amp = 0.52 * ra + 0.14 * an.sprint;
            thigh = amp * s * if s > 0.0 { 0.9 } else { 0.7 } + 0.05 * ra;
            knee = 0.08 * ra + 1.25 * c.max(0.0).powf(1.35) * ra.min(1.0) + 0.1 * an.sprint;
            // crouching bends everything
            thigh = lerp(thigh, 0.95 + 0.1 * s * ra, crouch);
            knee = lerp(knee, 1.9, crouch);
        }
        // airborne: tuck the legs, one a bit more than the other
        let tuck = if i == 0 { 0.6 } else { 0.35 };
        if !grounded && !freefall && !gliding && !swimming {
            thigh = lerp(thigh, tuck + 0.2, air);
            knee = lerp(knee, 1.0 + tuck, air);
        }
        if freefall {
            // legs trail behind the torso and spread
            thigh = -0.25 - 0.3 * (1.0 - ((-a.pitch - 0.1) / 1.1).clamp(0.0, 1.0));
            knee = 0.2;
        }
        if gliding {
            thigh = 0.18 + 0.05 * (time * 1.7 + i as f32).sin();
            knee = 0.3 + 0.1 * (time * 1.3 + i as f32 * 2.0).sin();
        }
        if swimming {
            thigh = 0.25 * (time * 8.0 + i as f32 * PI).sin();
            knee = 0.3 + 0.25 * (time * 8.0 + i as f32 * PI + 1.0).sin();
        }
        legs.thigh[i] = thigh;
        legs.knee[i] = knee;
        legs.foot[i] = (-(thigh - knee)).clamp(-0.9, 0.9) * if freefall || swimming { 0.5 } else { 1.0 };
        if grounded && crouch < 0.5 {
            // toe-off at the back of the stride
            legs.foot[i] += 0.25 * (-side_phase.sin()).max(0.0) * ra;
        }
    }
    let spread = if freefall { 0.2 } else { 0.0 };
    let mut leg_up = [Mat4::IDENTITY; 2];
    let mut leg_low = [Mat4::IDENTITY; 2];
    let mut boot = [Mat4::IDENTITY; 2];
    for i in 0..2 {
        let sx = if i == 0 { -1.0 } else { 1.0 };
        let hip = pelvis_m * tr(Vec3::new(sx * HIP_X, -HIP_DROP, 0.0)) * rz(sx * spread) * rx(legs.thigh[i]);
        let knee = hip * tr(Vec3::new(0.0, -THIGH, 0.0)) * rx(-legs.knee[i]);
        let ankle = knee * tr(Vec3::new(0.0, -SHIN, 0.0)) * rx(legs.foot[i]);
        leg_up[i] = hip;
        leg_low[i] = knee;
        boot[i] = ankle;
    }

    // ---- what is in hand ---------------------------------------------------------------------------------
    let item = if dead || a.build_mode || matches!(mode, MoveMode::Freefall | MoveMode::Glide | MoveMode::Bus) { None } else { a.inv.selected_item().copied() };
    let shoulder_c = Vec3::new(0.0, SHOULDER_Y, 0.0);
    let sh_pos = |i: usize| shoulder_c + Vec3::new(if i == 0 { -SHOULDER_X } else { SHOULDER_X }, 0.0, 0.0);
    let aim_yaw_rel = d - twist - hip_twist * 0.3;
    let r_yaw = ry(aim_yaw_rel * if freefall || gliding || swimming { 0.0 } else { 1.0 });

    // targets for the hands, in torso space (None = swing freely)
    let mut right_target: Option<Vec3> = None;
    let mut left_target: Option<Vec3> = None;
    let mut held: Option<(MeshId, Mat4)> = None; // in torso space
    let mut muzzle_local = Vec3::ZERO;

    match item {
        Some(Item::Weapon { kind, .. }) if grounded || swimming => {
            let aimk = an.aim;
            let (mesh, muz) = weapon_model(kind);
            muzzle_local = muz;
            // grip offset from the shoulder centre in the yaw frame
            let big = matches!(kind, WeaponKind::Sniper | WeaponKind::RocketLauncher | WeaponKind::AssaultRifle | WeaponKind::Shotgun);
            let g_aim = if big { Vec3::new(0.12, -0.15, -0.26) } else { Vec3::new(0.14, -0.1, -0.34) };
            let g_low = Vec3::new(0.2, -0.36, -0.12);
            let mut g = g_low.lerp(g_aim, aimk);
            let aim_pitch = a.pitch + lean * 0.9;
            let low_pitch = (a.pitch * 0.25 - 0.5).clamp(-0.9, 0.3);
            let mut pitch_w = lerp(low_pitch, aim_pitch, aimk);
            // recoil kicks the weapon back and up
            g.z += 0.075 * an.recoil;
            pitch_w += 0.12 * an.recoil;
            // running with a weapon holds it across the body
            let carry = (ra * (1.0 - aimk)).min(1.0);
            g.x -= 0.03 * carry;
            // equipping dips the weapon
            let mut dip = 0.0;
            if let Action::Swap { t } = a.action {
                dip = (t / kind.def().equip_time.max(0.05)).clamp(0.0, 1.0);
            }
            g.y -= 0.32 * dip;
            pitch_w -= 0.6 * dip;
            // reload: tilt the gun and take the support hand to the magazine
            let rl = an.reload;
            let rl_tilt = if rl > 0.0 { ease((rl * 6.0).min(1.0)) * ease(((1.0 - rl) * 6.0).min(1.0)) } else { 0.0 };
            let roll_w = 0.55 * rl_tilt;
            pitch_w += 0.25 * rl_tilt;
            let mk = |g: Vec3| tr(shoulder_c) * r_yaw * rx(pitch_w) * tr(g) * rz(-roll_w);
            let mut w = mk(g);
            // left hand target
            let fg = foregrip(kind);
            let mut lt = w.transform_point3(fg);
            if rl > 0.0 {
                // hand travels to the magazine, then to a pouch at the hip and back
                let mag = w.transform_point3(Vec3::new(0.0, -0.11, -0.07));
                let pouch = w.transform_point3(Vec3::new(-0.2, -0.38, 0.1));
                let u = rl;
                let blend = if u < 0.18 {
                    mag.lerp(lt, 1.0 - ease(u / 0.18))
                } else if u < 0.38 {
                    mag.lerp(pouch, ease((u - 0.18) / 0.2))
                } else if u < 0.58 {
                    pouch.lerp(mag, ease((u - 0.38) / 0.2))
                } else if u < 0.82 {
                    mag
                } else {
                    mag.lerp(w.transform_point3(fg), ease((u - 0.82) / 0.18))
                };
                lt = blend;
            }
            // keep the support hand within reach by sliding the weapon back toward the body
            let lsh = sh_pos(0);
            let reach = (UPPER_ARM + FOREARM) * 0.96;
            let dist = (lt - lsh).length();
            if dist > reach && rl == 0.0 {
                let excess = (dist - reach).min(0.2);
                let back = w.transform_vector3(Vec3::Z);
                w = tr(back * excess) * w;
                lt = w.transform_point3(fg);
            }
            held = Some((mesh, w));
            right_target = Some(w.transform_point3(Vec3::ZERO));
            left_target = Some(lt);
        }
        Some(Item::Pickaxe) if grounded || swimming => {
            // carried low at the side; swings in an arc
            let s = an.swing;
            let u = 1.0 - s;
            let ang = if s <= 0.001 {
                0.0
            } else if u < 0.2 {
                lerp(0.0, 1.35, ease(u / 0.2))
            } else if u < 0.5 {
                lerp(1.35, -1.55, ease((u - 0.2) / 0.3))
            } else {
                lerp(-1.55, 0.0, ease((u - 0.5) / 0.5))
            };
            let g = Vec3::new(0.2, -0.22 + 0.05 * ra, -0.12 - 0.04 * ra);
            let hold_pitch = -0.35 - 0.2 * ra;
            // the arc pivots around the shoulder
            let pitch = hold_pitch - ang * 1.0;
            let w = tr(shoulder_c) * r_yaw * rx(pitch) * tr(g);
            held = Some((MeshId::Pickaxe, w * rx(-0.15)));
            right_target = Some(w.transform_point3(Vec3::ZERO));
        }
        Some(Item::Consumable { kind, .. }) if grounded => {
            let h = an.heal;
            let drink = matches!(kind, ConsumableKind::ShieldSmall | ConsumableKind::ShieldBig | ConsumableKind::ChugJug);
            let raise = if drink { ease((h * 4.0).min(1.0)) * ease(((1.0 - h) * 8.0).min(1.0)) } else { 0.0 };
            let wob = if h > 0.0 { (time * 14.0).sin() * 0.012 } else { 0.0 };
            let g = Vec3::new(0.06, -0.26 + 0.34 * raise + wob, -0.3 + 0.1 * raise);
            let w = tr(shoulder_c) * r_yaw * tr(g) * rx(-0.35 * raise);
            let model = consumable_model(kind);
            held = Some((model, w));
            right_target = Some(w.transform_point3(Vec3::new(0.0, 0.07, 0.0)));
            left_target = Some(w.transform_point3(Vec3::new(-0.1, 0.07, 0.0)));
        }
        _ => {}
    }

    // building: both hands held out in front
    if a.build_mode && grounded {
        let pulse = (an.time * 6.0).sin() * 0.01;
        right_target = Some(tr(shoulder_c).transform_point3(r_yaw.transform_point3(Vec3::new(0.2, -0.2 + pulse, -0.38))));
        left_target = Some(tr(shoulder_c).transform_point3(r_yaw.transform_point3(Vec3::new(-0.2, -0.2 - pulse, -0.38))));
    }
    // freefall: arms spread wide
    // gliding: hands up on the bar (the glider sits above, expressed in root space below)
    let glider_bar_root = Vec3::new(0.0, 2.1, 0.0);

    // ---- arms ----------------------------------------------------------------------------------------------------------
    let mut arm_up = [Mat4::IDENTITY; 2];
    let mut arm_low = [Mat4::IDENTITY; 2];
    let mut hand = [Mat4::IDENTITY; 2];
    for i in 0..2 {
        let sx = if i == 0 { -1.0 } else { 1.0 };
        let sh_t = sh_pos(i);
        let sh_w = torso_m.transform_point3(sh_t);
        let target_local = if i == 1 { right_target } else { left_target };
        if gliding {
            // grip the bar above the head
            let bar = glider_bar_root + Vec3::new(sx * 0.3, 0.0, 0.0);
            let (elbow, wrist) = solve_elbow(sh_w, bar, torso_m.transform_vector3(Vec3::new(sx * 1.0, -0.2, 0.6)), UPPER_ARM, FOREARM);
            let hint = Vec3::Z;
            arm_up[i] = limb(sh_w, elbow, hint);
            arm_low[i] = limb(elbow, wrist, hint);
            hand[i] = limb(wrist, wrist + (wrist - elbow).normalize_or_zero() * 0.1, hint);
            continue;
        }
        if let Some(t_local) = target_local {
            // hand centre is 0.045 beyond the wrist along the forearm
            let t_w = torso_m.transform_point3(t_local);
            let pole = torso_m.transform_vector3(Vec3::new(sx * 0.9, -1.0, 0.45));
            let (elbow0, _) = solve_elbow(sh_w, t_w, pole, UPPER_ARM, FOREARM);
            let fdir = (t_w - elbow0).normalize_or_zero();
            let (elbow, wrist) = solve_elbow(sh_w, t_w - fdir * 0.045, pole, UPPER_ARM, FOREARM);
            let hint = torso_m.transform_vector3(Vec3::new(sx, 0.0, 0.0)).cross(Vec3::Y).normalize_or_zero();
            let hint = if hint == Vec3::ZERO { Vec3::Z } else { hint };
            arm_up[i] = limb(sh_w, elbow, hint);
            arm_low[i] = limb(elbow, wrist, hint);
            hand[i] = limb(wrist, wrist + (wrist - elbow).normalize_or_zero(), hint);
            continue;
        }
        // free swinging arm (FK)
        let side_phase = phase + if i == 0 { PI } else { 0.0 };
        let mut up_f = 0.0; // forward swing
        let mut out = 0.0; // abduction
        let mut elbow = 0.25;
        if grounded {
            up_f = -0.7 * ra * side_phase.sin() - 0.05;
            elbow = 0.25 + 0.55 * ra;
            out = 0.08 + 0.1 * air;
            if air > 0.01 {
                up_f = lerp(up_f, -0.2 + 0.3 * (time * 3.0 + i as f32).sin(), air);
                out = lerp(out, 0.7, air);
                elbow = lerp(elbow, 0.4, air);
            }
        }
        if freefall {
            let dive = ((-a.pitch - 0.1) / 1.1).clamp(0.0, 1.0);
            up_f = lerp(0.5, -0.25, dive);
            out = lerp(1.15, 0.2, dive);
            elbow = lerp(0.5, 0.15, dive);
        }
        if swimming {
            let ph = time * 4.0 + if i == 0 { PI } else { 0.0 };
            up_f = ph.sin() * 1.7 - 0.3;
            out = 0.25;
            elbow = 0.4 + 0.5 * (ph + 1.2).sin().max(0.0);
        }
        if dead {
            up_f = 0.0;
            out = 0.8;
            elbow = 0.3;
        }
        let sh_m = torso_m * tr(sh_t) * rz(sx * out) * rx(up_f);
        let el_m = sh_m * tr(Vec3::new(0.0, -UPPER_ARM, 0.0)) * rx(elbow);
        let wr_m = el_m * tr(Vec3::new(0.0, -FOREARM, 0.0));
        arm_up[i] = sh_m;
        arm_low[i] = el_m;
        hand[i] = wr_m;
    }

    // ---- backpack, glider, held item to world --------------------------------------------------------------------------
    let backpack = torso_m;
    let glider = if gliding {
        Some(root * tr(glider_bar_root) * rz(roll * 0.6) * rx(-lean * 0.4))
    } else {
        None
    };
    let item_world = held.map(|(m, w)| (m, root * torso_m * w));
    let muzzle = item_world.map(|(_, w)| w.transform_point3(muzzle_local)).unwrap_or_else(|| root.transform_point3(Vec3::new(0.0, 1.4, -0.5)));

    let to_world = |m: Mat4| root * m;
    Pose {
        torso: to_world(torso_m),
        pelvis: to_world(pelvis_m),
        head: to_world(head_m),
        backpack: to_world(backpack),
        arm_up: [to_world(arm_up[0]), to_world(arm_up[1])],
        arm_low: [to_world(arm_low[0]), to_world(arm_low[1])],
        hand: [to_world(hand[0]), to_world(hand[1])],
        leg_up: [to_world(leg_up[0]), to_world(leg_up[1])],
        leg_low: [to_world(leg_low[0]), to_world(leg_low[1])],
        boot: [to_world(boot[0]), to_world(boot[1])],
        item: item_world,
        glider,
        muzzle,
        head_pos: to_world(head_m).transform_point3(Vec3::new(0.0, HEAD_C, 0.0)),
    }
}

// ---------------------------------------------------------------------------------------
// Build pieces
// ---------------------------------------------------------------------------------------

use super::pieces::{PieceKey, LEVEL_H, TILE};

/// Instance transform for a build piece (see `models::piece_*` for the local frame).
pub fn piece_transform(key: &PieceKey, base_y: f32) -> Mat4 {
    let y0 = super::pieces::level_y(base_y, key.level);
    let (cx, cz) = (key.x as f32 * TILE, key.z as f32 * TILE);
    match key.kind {
        PieceKind::Wall => {
            if key.dir == 0 {
                tr(Vec3::new(cx, y0, cz))
            } else {
                tr(Vec3::new(cx, y0, cz)) * ry(-FRAC_PI_2)
            }
        }
        PieceKind::Floor | PieceKind::Roof => tr(Vec3::new(cx, y0, cz)),
        PieceKind::Ramp => {
            let center = Vec3::new(cx + TILE * 0.5, y0, cz + TILE * 0.5);
            let ang = match key.dir & 3 {
                0 => 0.0,
                1 => -FRAC_PI_2,
                2 => PI,
                _ => FRAC_PI_2,
            };
            tr(center) * ry(ang) * tr(Vec3::new(-TILE * 0.5, 0.0, -TILE * 0.5))
        }
    }
}

pub fn piece_mesh(kind: PieceKind) -> MeshId {
    match kind {
        PieceKind::Wall => MeshId::PieceWall,
        PieceKind::Floor => MeshId::PieceFloor,
        PieceKind::Ramp => MeshId::PieceRamp,
        PieceKind::Roof => MeshId::PieceRoof,
    }
}

pub fn level_height() -> f32 {
    LEVEL_H
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::actor::Outfit;
    use crate::game::pieces::shape_of;
    use crate::meshlib::build_mesh;
    use crate::rng::Rng;
    use std::f32::consts::TAU;

    fn actor() -> Actor {
        let mut rng = Rng::new(3);
        let mut a = Actor::new(1, "t", false, Outfit::random(&mut rng));
        a.mode = MoveMode::Ground;
        a.on_ground = true;
        a.pos = Vec3::new(10.0, 5.0, -4.0);
        a
    }

    fn finite(p: &Pose) -> bool {
        let all = [p.torso, p.pelvis, p.head, p.backpack, p.arm_up[0], p.arm_up[1], p.arm_low[0], p.arm_low[1], p.hand[0], p.hand[1], p.leg_up[0], p.leg_up[1], p.leg_low[0], p.leg_low[1], p.boot[0], p.boot[1]];
        all.iter().all(|m| m.to_cols_array().iter().all(|v| v.is_finite())) && p.muzzle.is_finite()
    }

    fn lowest_boot_y(p: &Pose) -> f32 {
        let boot = build_mesh(MeshId::CharBoot);
        let mut lo = f32::MAX;
        for m in p.boot {
            for v in &boot.verts {
                lo = lo.min(m.transform_point3(Vec3::from(v.pos)).y);
            }
        }
        lo
    }

    #[test]
    fn standing_pose_has_feet_on_the_ground_and_head_at_full_height() {
        let a = actor();
        let p = pose(&a);
        assert!(finite(&p));
        let lo = lowest_boot_y(&p) - a.pos.y;
        assert!(lo.abs() < 0.04, "boots rest on the ground: {lo}");
        let head = p.head_pos.y - a.pos.y;
        assert!((1.6..1.7).contains(&head), "head centre height {head}");
    }

    #[test]
    fn crouching_lowers_the_body_and_keeps_feet_planted() {
        let mut a = actor();
        a.anim.crouch = 1.0;
        let p = pose(&a);
        let head = p.head_pos.y - a.pos.y;
        assert!(head < 1.35 && head > 1.0, "crouched head height {head}");
        let lo = lowest_boot_y(&p) - a.pos.y;
        assert!(lo.abs() < 0.12, "crouched boots stay near the ground: {lo}");
    }

    #[test]
    fn run_cycle_swings_legs_in_opposition_and_never_sinks_into_the_ground() {
        let mut a = actor();
        a.anim.run = 1.0;
        let mut min_lo = f32::MAX;
        let mut max_dz = 0.0f32;
        for k in 0..64 {
            a.anim.phase = k as f32 / 64.0 * TAU;
            let p = pose(&a);
            assert!(finite(&p));
            let l = p.boot[0].transform_point3(Vec3::ZERO);
            let r = p.boot[1].transform_point3(Vec3::ZERO);
            max_dz = max_dz.max((l.z - r.z).abs());
            min_lo = min_lo.min(lowest_boot_y(&p) - a.pos.y);
        }
        assert!(max_dz > 0.45, "feet should pass each other with a long stride: {max_dz}");
        assert!(min_lo > -0.12, "feet shouldn't dig into the ground: {min_lo}");
    }

    #[test]
    fn hands_stay_on_the_weapon_while_aiming_for_every_gun() {
        for kind in WeaponKind::ALL {
            let mut a = actor();
            a.inv.add_weapon(kind, Rarity::Common, 10);
            a.inv.selected = 1;
            a.anim.aim = 1.0;
            a.yaw = 0.4;
            a.body_yaw = 0.4;
            a.pitch = 0.1;
            let p = pose(&a);
            assert!(finite(&p), "{kind:?}");
            let (mesh, w) = p.item.expect("weapon in hand");
            assert_eq!(mesh, weapon_model(kind).0);
            let grip = w.transform_point3(Vec3::ZERO);
            let rh = p.hand[1].transform_point3(Vec3::new(0.0, -0.045, 0.0));
            assert!(rh.distance(grip) < 0.08, "{kind:?} right hand {:.3} from the grip", rh.distance(grip));
            let lh = p.hand[0].transform_point3(Vec3::new(0.0, -0.045, 0.0));
            let fg = w.transform_point3(foregrip(kind));
            assert!(lh.distance(fg) < 0.11, "{kind:?} left hand {:.3} from the foregrip", lh.distance(fg));
            // the barrel points where the actor is aiming
            let dir = (p.muzzle - grip).normalize();
            let want = look_dir(a.yaw, a.pitch);
            assert!(dir.dot(want) > 0.97, "{kind:?} barrel direction off by {:.2}", dir.dot(want));
        }
    }

    #[test]
    fn muzzle_sits_near_the_simulated_shot_origin() {
        // the simulation spawns tracers at eye + right*0.30 - up*0.32 + dir*0.9
        let mut a = actor();
        a.inv.add_weapon(WeaponKind::AssaultRifle, Rarity::Common, 30);
        a.inv.selected = 1;
        a.anim.aim = 1.0;
        a.yaw = 1.0;
        a.body_yaw = 1.0;
        a.pitch = 0.0;
        let p = pose(&a);
        let dir = look_dir(a.yaw, a.pitch);
        let sim = a.eye_pos() + yaw_right(a.yaw) * 0.30 - Vec3::Y * 0.32 + dir * 0.9;
        assert!(p.muzzle.distance(sim) < 0.6, "visual muzzle {:?} vs sim {:?}", p.muzzle, sim);
    }

    #[test]
    fn reloading_moves_the_support_hand_away_from_the_foregrip() {
        let mut a = actor();
        a.inv.add_weapon(WeaponKind::AssaultRifle, Rarity::Common, 5);
        a.inv.selected = 1;
        a.anim.aim = 0.0;
        a.anim.reload = 0.0;
        let p0 = pose(&a);
        let h0 = p0.hand[0].transform_point3(Vec3::ZERO);
        let mut far = 0.0f32;
        for k in 1..20 {
            a.anim.reload = k as f32 / 20.0;
            let p1 = pose(&a);
            assert!(finite(&p1));
            far = far.max(h0.distance(p1.hand[0].transform_point3(Vec3::ZERO)));
        }
        assert!(far > 0.3, "the left hand should travel during a reload: {far}");
    }

    #[test]
    fn reload_phases_keep_both_hands_below_the_shoulders() {
        for kind in WeaponKind::ALL {
            let mut a = actor();
            a.inv.add_weapon(kind, Rarity::Common, 2);
            a.inv.selected = 1;
            a.anim.aim = 0.0;
            for k in 0..=40 {
                a.anim.reload = k as f32 / 40.0;
                let p = pose(&a);
                assert!(finite(&p));
                let shoulder_y = p.torso.transform_point3(Vec3::new(0.0, SHOULDER_Y, 0.0)).y;
                for (i, h) in p.hand.iter().enumerate() {
                    let hy = h.transform_point3(Vec3::ZERO).y;
                    assert!(hy < shoulder_y + 0.05, "{kind:?} hand {i} at reload {:.2} is {:.2} m above the shoulders", a.anim.reload, hy - shoulder_y);
                }
            }
        }
    }

    #[test]
    fn every_mode_and_item_produces_a_finite_pose() {
        let mut a = actor();
        a.inv.add_weapon(WeaponKind::Sniper, Rarity::Rare, 1);
        a.inv.add_consumable(ConsumableKind::ChugJug, 1);
        for mode in [MoveMode::Ground, MoveMode::Freefall, MoveMode::Glide, MoveMode::Swim, MoveMode::Dead, MoveMode::Bus] {
            for sel in 0..3 {
                for build in [false, true] {
                    a.mode = mode;
                    a.inv.selected = sel;
                    a.build_mode = build;
                    a.anim.swing = 0.6;
                    a.anim.heal = 0.5;
                    a.dead_time = 0.4;
                    a.pitch = -1.0;
                    let p = pose(&a);
                    assert!(finite(&p), "{mode:?} sel {sel} build {build}");
                    if mode == MoveMode::Glide {
                        assert!(p.glider.is_some());
                    }
                }
            }
        }
    }

    #[test]
    fn piece_meshes_land_exactly_on_their_collision_shapes() {
        use crate::game::actor::PieceKind;
        for (kind, dir) in [(PieceKind::Wall, 0u8), (PieceKind::Wall, 1), (PieceKind::Floor, 0), (PieceKind::Roof, 0), (PieceKind::Ramp, 0), (PieceKind::Ramp, 1), (PieceKind::Ramp, 2), (PieceKind::Ramp, 3)] {
            let key = PieceKey { kind, x: 3, z: -2, level: 1, dir };
            let base = 7.5;
            let m = piece_transform(&key, base);
            let mesh = build_mesh(piece_mesh(kind));
            let mut bb = Aabb::EMPTY;
            for v in &mesh.verts {
                bb.extend(m.transform_point3(Vec3::from(v.pos)));
            }
            let shape = shape_of(&key, base).aabb();
            let tol = 0.2;
            assert!(bb.min.x > shape.min.x - tol && bb.max.x < shape.max.x + tol, "{kind:?}/{dir} x: mesh {:?} shape {:?}", (bb.min.x, bb.max.x), (shape.min.x, shape.max.x));
            assert!(bb.min.z > shape.min.z - tol && bb.max.z < shape.max.z + tol, "{kind:?}/{dir} z: mesh {:?} shape {:?}", (bb.min.z, bb.max.z), (shape.min.z, shape.max.z));
            assert!(bb.min.y > shape.min.y - tol && bb.max.y < shape.max.y + 0.3, "{kind:?}/{dir} y: mesh {:?} shape {:?}", (bb.min.y, bb.max.y), (shape.min.y, shape.max.y));
            // and the mesh covers most of the shape's footprint
            assert!(bb.max.x - bb.min.x > (shape.max.x - shape.min.x) * 0.9 - 0.3);
            assert!(bb.max.z - bb.min.z > (shape.max.z - shape.min.z) * 0.9 - 0.3);
        }
    }
}
