//! # `audio_synth` - procedural sound effects for FightNight
//!
//! FightNight ships **no audio asset files**.  Every sound effect is synthesized at
//! start-up into a mono `Vec<f32>` (samples in `[-1, 1]`, peak <= 0.98) which other code
//! uploads to WebAudio.  This module only produces sample data; it is `std`-only,
//! single-threaded and compiles natively and to `wasm32-unknown-unknown` (no file IO, no
//! clocks, no external crates, no `rand`).
//!
//! ```ignore
//! use fn_core::audio_synth::{synth, Sfx};
//! for &sfx in Sfx::ALL {
//!     for v in 0..sfx.variants() {
//!         let samples = synth(sfx, 48_000, v);   // deterministic: same inputs => same samples
//!         // upload `samples` as an AudioBuffer; set `loop = true` if `sfx.is_loop()`
//!     }
//! }
//! ```
//!
//! ## Design
//!
//! * **Layering.**  Almost every sound is built the way a sound designer would build it in a
//!   DAW: a very short bright *transient* (what makes it "pop"), a mid *body*, a low *thump*
//!   (exponentially falling sine, saturated so it is audible on laptop speakers) and a short
//!   airy *tail* or a small reverb.  Layers are rendered into their own buffers, shaped with
//!   the chainable `Fx` helpers (filters, envelopes, soft clip) and mixed with `mix`.  Gunshots
//!   additionally use a noise "blast" whose low-pass sweeps from bright to dark, and a pitched
//!   mid "snap", which is what gives them body instead of sounding like a white-noise burst.
//! * **Mechanical sounds** (clicks, clanks, knocks, tinks, footsteps) use *modal synthesis*: a
//!   tiny filtered-noise strike plus a handful of exponentially damped sinusoids (`ring`)
//!   whose frequencies and decays pick the material - wood is low and short, metal higher
//!   and longer, glass very high and shimmering.
//! * **Noise** comes from a seeded xorshift64* RNG (white), Paul Kellet's filter (pink) and a
//!   leaky integrator (brown).  Filters are RBJ biquads (f64 state, so low cut-offs stay
//!   clean) and one-pole sections; sweeping filters are retuned every 8 samples.
//! * **Tonal / musical sounds** (chimes, jingles, horns, hums) use polyBLEP saws and squares
//!   (band-limited cheaply, no harsh aliasing), additive inharmonic bells built from damped
//!   resonators and a Freeverb-style Schroeder reverb for tails.  A polynomial sine keeps the
//!   oscillators platform independent.
//! * **Loops** (`Sfx::is_loop`) are seamless by construction: tonal components use
//!   frequencies that are whole multiples of `1 / loop_length` (exactly periodic), and noise
//!   components are rendered longer than the loop and folded back with an equal-power
//!   cross-fade (`LoopSpec`).  The finished loop is finally rotated to the point where
//!   `last -> first` is the smoothest, so the seam is inaudible even to a sample-exact test.
//! * **Mastering.**  Everything is soft-clipped ("tanh style") where it adds punch, given a
//!   0.3 ms fade-in and an end fade to exactly zero (one-shots), DC-free (loops) and
//!   peak-normalised to a per-sound loudness (gunshots/explosion ~0.9, impacts ~0.7,
//!   footsteps ~0.35, UI ~0.4, ambient loops ~0.35-0.5, jingles ~0.6).
//! * **Variants.**  `variant` re-seeds the noise and nudges pitch (+-8 %) and brightness
//!   (+-12 %) through small deterministic tables, so repeated footsteps/shots do not sound
//!   machine-gunned.  Variant 0 is the nominal sound; any `u32` is accepted.
//! * **Cost.**  Rendering every sound and variant at 48 kHz (97 buffers, ~53 s of audio) takes
//!   about 0.3 s natively in release and ~0.5 s in WebAssembly, which is fine for a start-up
//!   job.  The output only depends on `(sfx, sample_rate, variant)`.

// Recipe helpers take many knobs (frequency, decay, gain, ...) by design.
#![allow(clippy::too_many_arguments)]

use std::f32::consts::{FRAC_PI_2, PI, TAU};

// ============================================================================
// 1. Public API
// ============================================================================

/// Every sound effect of the game.  The discriminants are stable (`repr(u8)`) and equal the
/// index of the variant in [`Sfx::ALL`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Sfx {
    // --- weapons -----------------------------------------------------------
    ShotAr,
    ShotSmg,
    ShotPistol,
    ShotShotgun,
    ShotSniper,
    RocketFire,
    Explosion,
    // --- weapon handling ---------------------------------------------------
    ReloadMag,
    ReloadShell,
    PumpAction,
    EmptyClick,
    WeaponSwap,
    // --- movement ----------------------------------------------------------
    StepGrass,
    StepSand,
    StepStone,
    StepWood,
    StepMetal,
    StepWater,
    Jump,
    Land,
    // --- pickups / loot ----------------------------------------------------
    PickupWeapon,
    PickupAmmo,
    PickupHeal,
    ChestOpen,
    ChestHum,
    // --- combat feedback ---------------------------------------------------
    HitMarker,
    HitHead,
    HitShield,
    PlayerHurt,
    Kill,
    // --- building / harvesting ---------------------------------------------
    BuildWood,
    BuildStone,
    BuildMetal,
    PieceDestroy,
    HarvestHit,
    HarvestBreak,
    TreeFall,
    // --- healing -----------------------------------------------------------
    HealBandage,
    HealPotion,
    ShieldUse,
    // --- storm -------------------------------------------------------------
    StormAmbient,
    StormWarning,
    StormDamage,
    // --- bus / air ---------------------------------------------------------
    BusLoop,
    WindLoop,
    GliderDeploy,
    GliderLoop,
    // --- UI / jingles ------------------------------------------------------
    UiClick,
    UiHover,
    UiBack,
    Victory,
    Defeat,
    Elimination,
    DropIn,
}

impl Sfx {
    /// Every variant, in declaration order (`ALL[i] as usize == i`).
    pub const ALL: &'static [Sfx] = &[
        Sfx::ShotAr,
        Sfx::ShotSmg,
        Sfx::ShotPistol,
        Sfx::ShotShotgun,
        Sfx::ShotSniper,
        Sfx::RocketFire,
        Sfx::Explosion,
        Sfx::ReloadMag,
        Sfx::ReloadShell,
        Sfx::PumpAction,
        Sfx::EmptyClick,
        Sfx::WeaponSwap,
        Sfx::StepGrass,
        Sfx::StepSand,
        Sfx::StepStone,
        Sfx::StepWood,
        Sfx::StepMetal,
        Sfx::StepWater,
        Sfx::Jump,
        Sfx::Land,
        Sfx::PickupWeapon,
        Sfx::PickupAmmo,
        Sfx::PickupHeal,
        Sfx::ChestOpen,
        Sfx::ChestHum,
        Sfx::HitMarker,
        Sfx::HitHead,
        Sfx::HitShield,
        Sfx::PlayerHurt,
        Sfx::Kill,
        Sfx::BuildWood,
        Sfx::BuildStone,
        Sfx::BuildMetal,
        Sfx::PieceDestroy,
        Sfx::HarvestHit,
        Sfx::HarvestBreak,
        Sfx::TreeFall,
        Sfx::HealBandage,
        Sfx::HealPotion,
        Sfx::ShieldUse,
        Sfx::StormAmbient,
        Sfx::StormWarning,
        Sfx::StormDamage,
        Sfx::BusLoop,
        Sfx::WindLoop,
        Sfx::GliderDeploy,
        Sfx::GliderLoop,
        Sfx::UiClick,
        Sfx::UiHover,
        Sfx::UiBack,
        Sfx::Victory,
        Sfx::Defeat,
        Sfx::Elimination,
        Sfx::DropIn,
    ];

    /// Stable snake_case identifier, e.g. `"shot_ar"`.
    pub fn name(self) -> &'static str {
        match self {
            Sfx::ShotAr => "shot_ar",
            Sfx::ShotSmg => "shot_smg",
            Sfx::ShotPistol => "shot_pistol",
            Sfx::ShotShotgun => "shot_shotgun",
            Sfx::ShotSniper => "shot_sniper",
            Sfx::RocketFire => "rocket_fire",
            Sfx::Explosion => "explosion",
            Sfx::ReloadMag => "reload_mag",
            Sfx::ReloadShell => "reload_shell",
            Sfx::PumpAction => "pump_action",
            Sfx::EmptyClick => "empty_click",
            Sfx::WeaponSwap => "weapon_swap",
            Sfx::StepGrass => "step_grass",
            Sfx::StepSand => "step_sand",
            Sfx::StepStone => "step_stone",
            Sfx::StepWood => "step_wood",
            Sfx::StepMetal => "step_metal",
            Sfx::StepWater => "step_water",
            Sfx::Jump => "jump",
            Sfx::Land => "land",
            Sfx::PickupWeapon => "pickup_weapon",
            Sfx::PickupAmmo => "pickup_ammo",
            Sfx::PickupHeal => "pickup_heal",
            Sfx::ChestOpen => "chest_open",
            Sfx::ChestHum => "chest_hum",
            Sfx::HitMarker => "hit_marker",
            Sfx::HitHead => "hit_head",
            Sfx::HitShield => "hit_shield",
            Sfx::PlayerHurt => "player_hurt",
            Sfx::Kill => "kill",
            Sfx::BuildWood => "build_wood",
            Sfx::BuildStone => "build_stone",
            Sfx::BuildMetal => "build_metal",
            Sfx::PieceDestroy => "piece_destroy",
            Sfx::HarvestHit => "harvest_hit",
            Sfx::HarvestBreak => "harvest_break",
            Sfx::TreeFall => "tree_fall",
            Sfx::HealBandage => "heal_bandage",
            Sfx::HealPotion => "heal_potion",
            Sfx::ShieldUse => "shield_use",
            Sfx::StormAmbient => "storm_ambient",
            Sfx::StormWarning => "storm_warning",
            Sfx::StormDamage => "storm_damage",
            Sfx::BusLoop => "bus_loop",
            Sfx::WindLoop => "wind_loop",
            Sfx::GliderDeploy => "glider_deploy",
            Sfx::GliderLoop => "glider_loop",
            Sfx::UiClick => "ui_click",
            Sfx::UiHover => "ui_hover",
            Sfx::UiBack => "ui_back",
            Sfx::Victory => "victory",
            Sfx::Defeat => "defeat",
            Sfx::Elimination => "elimination",
            Sfx::DropIn => "drop_in",
        }
    }

    /// `true` if the buffer is meant to be looped; such buffers are seamless
    /// (`last -> first` is as smooth as any two neighbouring samples).
    pub fn is_loop(self) -> bool {
        matches!(
            self,
            Sfx::ChestHum | Sfx::StormAmbient | Sfx::BusLoop | Sfx::WindLoop | Sfx::GliderLoop
        )
    }

    /// How many distinct variants are worth pre-rendering (`variant` in `0..variants()`).
    pub fn variants(self) -> u32 {
        match self {
            Sfx::StepGrass
            | Sfx::StepSand
            | Sfx::StepStone
            | Sfx::StepWood
            | Sfx::StepMetal
            | Sfx::StepWater => 4,
            Sfx::ShotAr | Sfx::ShotSmg | Sfx::HarvestHit => 4,
            Sfx::ShotPistol
            | Sfx::ShotShotgun
            | Sfx::Land
            | Sfx::HitMarker
            | Sfx::BuildWood
            | Sfx::BuildStone
            | Sfx::BuildMetal
            | Sfx::PlayerHurt => 3,
            _ => 1,
        }
    }
}

/// Synthesizes one sound effect.
///
/// Returns mono samples in `[-1, 1]` (peak <= 0.98) at `sample_rate` (44100 or 48000 are the
/// intended rates; anything from 8 kHz to 192 kHz works).  `variant` (`0..sfx.variants()`)
/// deterministically changes the noise seed and slightly shifts pitch and timbre; any `u32`
/// is accepted.  The result depends only on `(sfx, sample_rate, variant)`.
pub fn synth(sfx: Sfx, sample_rate: u32, variant: u32) -> Vec<f32> {
    let sr = sample_rate.clamp(8_000, 192_000);
    let mut c = Ctx::new(sfx, sr, variant);
    let mut buf = match sfx {
        // weapons
        Sfx::ShotAr => shot_ar(&mut c),
        Sfx::ShotSmg => shot_smg(&mut c),
        Sfx::ShotPistol => shot_pistol(&mut c),
        Sfx::ShotShotgun => shot_shotgun(&mut c),
        Sfx::ShotSniper => shot_sniper(&mut c),
        Sfx::RocketFire => rocket_fire(&mut c),
        Sfx::Explosion => explosion(&mut c),
        // handling
        Sfx::ReloadMag => reload_mag(&mut c),
        Sfx::ReloadShell => reload_shell(&mut c),
        Sfx::PumpAction => pump_action(&mut c),
        Sfx::EmptyClick => empty_click(&mut c),
        Sfx::WeaponSwap => weapon_swap(&mut c),
        // movement
        Sfx::StepGrass => step_grass(&mut c),
        Sfx::StepSand => step_sand(&mut c),
        Sfx::StepStone => step_stone(&mut c),
        Sfx::StepWood => step_wood(&mut c),
        Sfx::StepMetal => step_metal(&mut c),
        Sfx::StepWater => step_water(&mut c),
        Sfx::Jump => jump(&mut c),
        Sfx::Land => land(&mut c),
        // pickups
        Sfx::PickupWeapon => pickup_weapon(&mut c),
        Sfx::PickupAmmo => pickup_ammo(&mut c),
        Sfx::PickupHeal => pickup_heal(&mut c),
        Sfx::ChestOpen => chest_open(&mut c),
        Sfx::ChestHum => chest_hum(&mut c),
        // feedback
        Sfx::HitMarker => hit_marker(&mut c),
        Sfx::HitHead => hit_head(&mut c),
        Sfx::HitShield => hit_shield(&mut c),
        Sfx::PlayerHurt => player_hurt(&mut c),
        Sfx::Kill => kill(&mut c),
        // building / harvesting
        Sfx::BuildWood => build_wood(&mut c),
        Sfx::BuildStone => build_stone(&mut c),
        Sfx::BuildMetal => build_metal(&mut c),
        Sfx::PieceDestroy => piece_destroy(&mut c),
        Sfx::HarvestHit => harvest_hit(&mut c),
        Sfx::HarvestBreak => harvest_break(&mut c),
        Sfx::TreeFall => tree_fall(&mut c),
        // healing
        Sfx::HealBandage => heal_bandage(&mut c),
        Sfx::HealPotion => heal_potion(&mut c),
        Sfx::ShieldUse => shield_use(&mut c),
        // storm
        Sfx::StormAmbient => storm_ambient(&mut c),
        Sfx::StormWarning => storm_warning(&mut c),
        Sfx::StormDamage => storm_damage(&mut c),
        // bus / air
        Sfx::BusLoop => bus_loop(&mut c),
        Sfx::WindLoop => wind_loop(&mut c),
        Sfx::GliderDeploy => glider_deploy(&mut c),
        Sfx::GliderLoop => glider_loop(&mut c),
        // UI / jingles
        Sfx::UiClick => ui_click(&mut c),
        Sfx::UiHover => ui_hover(&mut c),
        Sfx::UiBack => ui_back(&mut c),
        Sfx::Victory => victory(&mut c),
        Sfx::Defeat => defeat(&mut c),
        Sfx::Elimination => elimination(&mut c),
        Sfx::DropIn => drop_in(&mut c),
    };
    master(sfx, c.sr, &mut buf);
    buf
}

// ============================================================================
// 2. DSP toolkit
// ============================================================================

// ---------------------------------------------------------------------------
// 2.1 Random numbers and the per-sound synthesis context
// ---------------------------------------------------------------------------

/// Tiny deterministic xorshift64* generator (seeded through splitmix64).
#[derive(Clone)]
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Rng {
        let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        Rng(if z == 0 { 0x1234_5678_9ABC_DEF1 } else { z })
    }
    #[inline]
    fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 32) as u32
    }
    /// Uniform in `[0, 1)`.
    #[inline]
    fn f(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 * (1.0 / 16_777_216.0)
    }
    /// Uniform in `[-1, 1)`.
    #[inline]
    fn bi(&mut self) -> f32 {
        self.f() * 2.0 - 1.0
    }
    /// Uniform in `[a, b)`.
    #[inline]
    fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.f()
    }
    /// `true` with probability `p`.
    #[inline]
    fn chance(&mut self, p: f32) -> bool {
        self.f() < p
    }
    /// Uniform index in `0..n`.
    #[inline]
    fn below(&mut self, n: usize) -> usize {
        (((self.next_u32() as u64) * (n.max(1) as u64)) >> 32) as usize
    }
}

/// Per-variant pitch multipliers (variant 0 is nominal).
const PITCH_TAB: [f32; 8] = [1.0, 0.945, 1.06, 0.975, 1.035, 0.92, 1.08, 0.99];
/// Per-variant brightness multipliers (scale filter cut-offs).
const BRIGHT_TAB: [f32; 8] = [1.0, 1.1, 0.92, 1.05, 0.88, 1.12, 0.96, 1.02];

/// Everything a sound recipe needs: sample rate, RNG and the variant nudges.
struct Ctx {
    sr: f32,
    rng: Rng,
    /// Pitch multiplier for this variant (~0.92..1.08); scale tonal frequencies by it.
    p: f32,
    /// Brightness multiplier for this variant (~0.88..1.12); scale filter cut-offs by it.
    b: f32,
}

impl Ctx {
    fn new(sfx: Sfx, sr: u32, variant: u32) -> Ctx {
        let seed = ((sfx as u64) + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ ((variant as u64) + 1).wrapping_mul(0xD1B5_4A32_D192_ED03)
            ^ 0x0F16_4701_7777_0001;
        let mut rng = Rng::new(seed);
        let (p, b) = if (variant as usize) < PITCH_TAB.len() {
            (PITCH_TAB[variant as usize], BRIGHT_TAB[variant as usize])
        } else {
            (rng.range(0.92, 1.08), rng.range(0.88, 1.12))
        };
        Ctx { sr: sr as f32, rng, p, b }
    }

    /// Seconds -> sample count (at least 1).
    fn n(&self, secs: f32) -> usize {
        ((secs * self.sr).round() as usize).max(1)
    }
    /// Zero-filled buffer of `secs` seconds.
    fn silence(&self, secs: f32) -> Vec<f32> {
        vec![0.0; self.n(secs)]
    }
    /// White noise in `[-1, 1)`.
    fn white(&mut self, secs: f32) -> Vec<f32> {
        let n = self.n(secs);
        (0..n).map(|_| self.rng.bi()).collect()
    }
    /// Pink (-3 dB/oct) noise, Paul Kellet's refined filter.
    fn pink(&mut self, secs: f32) -> Vec<f32> {
        let n = self.n(secs);
        let mut k = [0.0f32; 7];
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            let w = self.rng.bi();
            k[0] = 0.99886 * k[0] + w * 0.055_517_9;
            k[1] = 0.99332 * k[1] + w * 0.075_075_9;
            k[2] = 0.969 * k[2] + w * 0.153_852;
            k[3] = 0.8665 * k[3] + w * 0.310_485_6;
            k[4] = 0.55 * k[4] + w * 0.532_952_2;
            k[5] = -0.7616 * k[5] - w * 0.016_898;
            let y = k[0] + k[1] + k[2] + k[3] + k[4] + k[5] + k[6] + w * 0.5362;
            k[6] = w * 0.115926;
            out.push(y * 0.3);
        }
        out
    }
    /// Brown (-6 dB/oct) noise: a leaky integral of white noise.
    fn brown(&mut self, secs: f32) -> Vec<f32> {
        let n = self.n(secs);
        let mut y = 0.0f32;
        (0..n)
            .map(|_| {
                y = (y + 0.02 * self.rng.bi()) / 1.02;
                y * 3.5
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// 2.2 Small math helpers
// ---------------------------------------------------------------------------

/// Sine of a phase given in *cycles* (`sin(2*pi*p)`), polynomial approximation (error
/// < 4e-6) that is identical on every platform.  Any finite `p` works.
#[inline]
fn sin_cyc(p: f32) -> f32 {
    let mut x = (p - p.floor()) * TAU; // [0, 2pi)
    if x > PI {
        x -= TAU; // [-pi, pi]
    }
    // fold to [-pi/2, pi/2]
    if x > FRAC_PI_2 {
        x = PI - x;
    } else if x < -FRAC_PI_2 {
        x = -PI - x;
    }
    let x2 = x * x;
    x * (1.0 + x2 * (-1.666_666_7e-1 + x2 * (8.333_333e-3 + x2 * (-1.984_127e-4 + x2 * 2.755_731_9e-6))))
}

/// Cosine of a phase in cycles.
#[inline]
fn cos_cyc(p: f32) -> f32 {
    sin_cyc(p + 0.25)
}

/// C1-continuous "tanh-style" soft clipper: `x(27 + x^2)/(27 + 9x^2)` hits exactly +-1 with
/// zero slope at |x| = 3.
#[inline]
fn soft(x: f32) -> f32 {
    if x >= 3.0 {
        1.0
    } else if x <= -3.0 {
        -1.0
    } else {
        let x2 = x * x;
        x * (27.0 + x2) / (27.0 + 9.0 * x2)
    }
}

#[inline]
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Hermite smoothstep from 0 (at `a`) to 1 (at `b`).
#[inline]
fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Geometric interpolation `a -> b` (`s` in 0..1); ideal for pitch and filter sweeps.
#[inline]
fn geo(a: f32, b: f32, s: f32) -> f32 {
    a * (b / a).powf(s.clamp(0.0, 1.0))
}

/// MIDI note number -> Hz.
#[inline]
fn hz(midi: f32) -> f32 {
    440.0 * ((midi - 69.0) / 12.0).exp2()
}

/// Frequency ratio of `c` cents.
#[inline]
fn cents(c: f32) -> f32 {
    (c / 1200.0).exp2()
}

/// `exp(-t / tau)`.
#[inline]
fn decay(t: f32, tau: f32) -> f32 {
    (-t / tau).exp()
}

/// ADSR evaluated analytically at time `t`; the note is released at `gate` and the release
/// phase (squared ramp) reaches exactly zero `rel` seconds later.
fn adsr(t: f32, a: f32, d: f32, s: f32, gate: f32, rel: f32) -> f32 {
    let level = |t: f32| -> f32 {
        if t < a {
            t / a.max(1e-6)
        } else {
            s + (1.0 - s) * decay(t - a, d / 3.0)
        }
    };
    if t < gate {
        level(t)
    } else {
        let r = 1.0 - (t - gate) / rel.max(1e-6);
        if r <= 0.0 {
            0.0
        } else {
            level(gate) * r * r
        }
    }
}

// ---------------------------------------------------------------------------
// 2.3 Filters
// ---------------------------------------------------------------------------

/// Biquad responses.  The toolkit deliberately offers the full RBJ set (tested in the unit
/// tests) even where the current recipes only need some of it.
#[allow(dead_code)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Lp,
    Hp,
    Bp,
    Notch,
    Peak,
    LowShelf,
    HighShelf,
}

/// RBJ "Audio EQ Cookbook" biquad in transposed direct form II.  Coefficients and state are
/// `f64` so that low cut-offs (60 Hz at 48 kHz) stay clean.
#[derive(Clone)]
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    z1: f64,
    z2: f64,
}

impl Biquad {
    fn new(kind: Kind, sr: f32, f: f32, q: f32, gain_db: f32) -> Biquad {
        let mut b = Biquad { b0: 1.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0, z1: 0.0, z2: 0.0 };
        b.set(kind, sr, f, q, gain_db);
        b
    }

    /// (Re)computes the coefficients without touching the filter state, so it can be used
    /// for sweeps.  `gain_db` only matters for `Peak` and the shelves.
    fn set(&mut self, kind: Kind, sr: f32, f: f32, q: f32, gain_db: f32) {
        let sr = sr as f64;
        let f = (f as f64).clamp(10.0, sr * 0.49);
        let q = (q as f64).max(0.05);
        let w0 = std::f64::consts::TAU * f / sr;
        let (sw, cw) = w0.sin_cos();
        let alpha = sw / (2.0 * q);
        let (b0, b1, b2, a0, a1, a2) = match kind {
            Kind::Lp => ((1.0 - cw) * 0.5, 1.0 - cw, (1.0 - cw) * 0.5, 1.0 + alpha, -2.0 * cw, 1.0 - alpha),
            Kind::Hp => ((1.0 + cw) * 0.5, -(1.0 + cw), (1.0 + cw) * 0.5, 1.0 + alpha, -2.0 * cw, 1.0 - alpha),
            // constant 0 dB peak gain band-pass
            Kind::Bp => (alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * cw, 1.0 - alpha),
            Kind::Notch => (1.0, -2.0 * cw, 1.0, 1.0 + alpha, -2.0 * cw, 1.0 - alpha),
            Kind::Peak => {
                let a = 10f64.powf(gain_db as f64 / 40.0);
                (1.0 + alpha * a, -2.0 * cw, 1.0 - alpha * a, 1.0 + alpha / a, -2.0 * cw, 1.0 - alpha / a)
            }
            Kind::LowShelf => {
                let a = 10f64.powf(gain_db as f64 / 40.0);
                let t = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) - (a - 1.0) * cw + t),
                    2.0 * a * ((a - 1.0) - (a + 1.0) * cw),
                    a * ((a + 1.0) - (a - 1.0) * cw - t),
                    (a + 1.0) + (a - 1.0) * cw + t,
                    -2.0 * ((a - 1.0) + (a + 1.0) * cw),
                    (a + 1.0) + (a - 1.0) * cw - t,
                )
            }
            Kind::HighShelf => {
                let a = 10f64.powf(gain_db as f64 / 40.0);
                let t = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) + (a - 1.0) * cw + t),
                    -2.0 * a * ((a - 1.0) + (a + 1.0) * cw),
                    a * ((a + 1.0) + (a - 1.0) * cw - t),
                    (a + 1.0) - (a - 1.0) * cw + t,
                    2.0 * ((a - 1.0) - (a + 1.0) * cw),
                    (a + 1.0) - (a - 1.0) * cw - t,
                )
            }
        };
        let inv = 1.0 / a0;
        self.b0 = b0 * inv;
        self.b1 = b1 * inv;
        self.b2 = b2 * inv;
        self.a1 = a1 * inv;
        self.a2 = a2 * inv;
    }

    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let x = x as f64;
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y as f32
    }
}

fn run_biquad(buf: &mut [f32], mut bq: Biquad) {
    for x in buf.iter_mut() {
        *x = bq.tick(*x);
    }
}

/// Q of a 2-pole Butterworth section (maximally flat).
const BUTTER_Q: f32 = std::f32::consts::FRAC_1_SQRT_2;

/// How often (in samples) sweeping filters are retuned.
const SWEEP_STEP: usize = 8;

/// Chainable offline effects on a rendered layer.  All methods consume and return the
/// buffer so recipes read like a signal chain: `c.white(0.05).bp(sr, 3000.0, 0.8).env(sr, 1e-4, 6e-3)`.
#[allow(dead_code)] // full toolkit; not every effect is used by a recipe
trait Fx: Sized {
    /// 2-pole Butterworth low-pass.
    fn lp(self, sr: f32, f: f32) -> Self;
    /// Resonant 2-pole low-pass.
    fn lpq(self, sr: f32, f: f32, q: f32) -> Self;
    /// 2-pole Butterworth high-pass.
    fn hp(self, sr: f32, f: f32) -> Self;
    /// Band-pass with 0 dB peak gain; `q` = centre / bandwidth.
    fn bp(self, sr: f32, f: f32, q: f32) -> Self;
    /// Notch.
    fn notch(self, sr: f32, f: f32, q: f32) -> Self;
    /// Peaking EQ.
    fn peak(self, sr: f32, f: f32, q: f32, db: f32) -> Self;
    /// Low shelf.
    fn low_shelf(self, sr: f32, f: f32, db: f32) -> Self;
    /// High shelf.
    fn high_shelf(self, sr: f32, f: f32, db: f32) -> Self;
    /// 1-pole low-pass (6 dB/oct).
    fn lp1(self, sr: f32, f: f32) -> Self;
    /// 1-pole high-pass (6 dB/oct).
    fn hp1(self, sr: f32, f: f32) -> Self;
    /// Time-varying biquad: `f(t)` gives the cut-off / centre in Hz at time `t` seconds.
    fn sweep(self, sr: f32, kind: Kind, q: f32, f: impl Fn(f32) -> f32) -> Self;
    /// Linear attack of `att` seconds times an exponential decay with time constant `tau`.
    fn env(self, sr: f32, att: f32, tau: f32) -> Self;
    /// Multiplies by `g(t)` (t in seconds).
    fn env_fn(self, sr: f32, g: impl Fn(f32) -> f32) -> Self;
    /// Constant gain.
    fn amp(self, g: f32) -> Self;
    /// Soft clip with pre-gain `drive` (adds odd harmonics, rounds off peaks).
    fn clip(self, drive: f32) -> Self;
    /// Fades the last `secs` seconds to exactly zero (raised-cosine).
    fn fade_out(self, sr: f32, secs: f32) -> Self;
    /// Fades the first `secs` seconds in (raised-cosine).
    fn fade_in(self, sr: f32, secs: f32) -> Self;
    /// Time reversal (for risers / reverse swells).
    fn rev(self) -> Self;
    /// Scales so the absolute peak equals `peak` (no-op for silence).
    fn norm(self, peak: f32) -> Self;
}

impl Fx for Vec<f32> {
    fn lp(mut self, sr: f32, f: f32) -> Self {
        run_biquad(&mut self, Biquad::new(Kind::Lp, sr, f, BUTTER_Q, 0.0));
        self
    }
    fn lpq(mut self, sr: f32, f: f32, q: f32) -> Self {
        run_biquad(&mut self, Biquad::new(Kind::Lp, sr, f, q, 0.0));
        self
    }
    fn hp(mut self, sr: f32, f: f32) -> Self {
        run_biquad(&mut self, Biquad::new(Kind::Hp, sr, f, BUTTER_Q, 0.0));
        self
    }
    fn bp(mut self, sr: f32, f: f32, q: f32) -> Self {
        run_biquad(&mut self, Biquad::new(Kind::Bp, sr, f, q, 0.0));
        self
    }
    fn notch(mut self, sr: f32, f: f32, q: f32) -> Self {
        run_biquad(&mut self, Biquad::new(Kind::Notch, sr, f, q, 0.0));
        self
    }
    fn peak(mut self, sr: f32, f: f32, q: f32, db: f32) -> Self {
        run_biquad(&mut self, Biquad::new(Kind::Peak, sr, f, q, db));
        self
    }
    fn low_shelf(mut self, sr: f32, f: f32, db: f32) -> Self {
        run_biquad(&mut self, Biquad::new(Kind::LowShelf, sr, f, BUTTER_Q, db));
        self
    }
    fn high_shelf(mut self, sr: f32, f: f32, db: f32) -> Self {
        run_biquad(&mut self, Biquad::new(Kind::HighShelf, sr, f, BUTTER_Q, db));
        self
    }
    fn lp1(mut self, sr: f32, f: f32) -> Self {
        let a = 1.0 - (-TAU * f.min(sr * 0.49) / sr).exp();
        let mut y = 0.0f32;
        for x in self.iter_mut() {
            y += a * (*x - y);
            *x = y;
        }
        self
    }
    fn hp1(mut self, sr: f32, f: f32) -> Self {
        let a = 1.0 - (-TAU * f.min(sr * 0.49) / sr).exp();
        let mut y = 0.0f32;
        for x in self.iter_mut() {
            y += a * (*x - y);
            *x -= y;
        }
        self
    }
    fn sweep(mut self, sr: f32, kind: Kind, q: f32, f: impl Fn(f32) -> f32) -> Self {
        let mut bq = Biquad::new(kind, sr, f(0.0), q, 0.0);
        for (i, x) in self.iter_mut().enumerate() {
            if i % SWEEP_STEP == SWEEP_STEP - 1 {
                bq.set(kind, sr, f(i as f32 / sr), q, 0.0);
            }
            *x = bq.tick(*x);
        }
        self
    }
    fn env(mut self, sr: f32, att: f32, tau: f32) -> Self {
        let k = (-1.0 / (tau * sr)).exp();
        let att_n = (att * sr).max(1.0);
        let mut e = 1.0f32;
        for (i, x) in self.iter_mut().enumerate() {
            let a = (i as f32 / att_n).min(1.0);
            *x *= a * e;
            e *= k;
        }
        self
    }
    fn env_fn(mut self, sr: f32, g: impl Fn(f32) -> f32) -> Self {
        for (i, x) in self.iter_mut().enumerate() {
            *x *= g(i as f32 / sr);
        }
        self
    }
    fn amp(mut self, g: f32) -> Self {
        for x in self.iter_mut() {
            *x *= g;
        }
        self
    }
    fn clip(mut self, drive: f32) -> Self {
        for x in self.iter_mut() {
            *x = soft(*x * drive);
        }
        self
    }
    fn fade_out(mut self, sr: f32, secs: f32) -> Self {
        let n = self.len();
        let m = ((secs * sr) as usize).clamp(1, n);
        for (j, x) in self[n - m..].iter_mut().enumerate() {
            // j = 0 is the first faded sample, j = m-1 the last of the buffer (-> 0)
            let t = (j + 1) as f32 / m as f32;
            *x *= 0.5 + 0.5 * cos_cyc(0.5 * t);
        }
        if let Some(l) = self.last_mut() {
            *l = 0.0;
        }
        self
    }
    fn fade_in(mut self, sr: f32, secs: f32) -> Self {
        let n = self.len();
        let m = ((secs * sr) as usize).clamp(1, n);
        for (j, x) in self[..m].iter_mut().enumerate() {
            let t = j as f32 / m as f32;
            *x *= 0.5 - 0.5 * cos_cyc(0.5 * t);
        }
        self
    }
    fn rev(mut self) -> Self {
        self.reverse();
        self
    }
    fn norm(mut self, peak: f32) -> Self {
        let p = self.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        if p > 1e-9 {
            let g = peak / p;
            for x in self.iter_mut() {
                *x *= g;
            }
        }
        self
    }
}

// ---------------------------------------------------------------------------
// 2.4 Mixing
// ---------------------------------------------------------------------------

/// Adds `src * gain` into `dst` starting at sample `at`.  Anything that would fall outside
/// `dst` is dropped (the destination buffer defines the sound's length).  A layer that ends
/// on its own (is not cut off by `dst`) gets a <= 128-sample raised-cosine fade-out so a
/// not-quite-decayed layer can never end in a click.
fn mix(dst: &mut [f32], src: &[f32], at: usize, gain: f32) {
    if at >= dst.len() || src.is_empty() {
        return;
    }
    let m = src.len().min(dst.len() - at);
    let fade = if m == src.len() { (m / 8).min(128) } else { 0 };
    for (i, (d, s)) in dst[at..at + m].iter_mut().zip(&src[..m]).enumerate() {
        let mut g = gain;
        if fade > 0 && i + fade >= m {
            let r = (m - i) as f32 / fade as f32; // 1 -> 0 over the last `fade` samples
            g *= 0.5 - 0.5 * cos_cyc(0.5 * r);
        }
        *d += *s * g;
    }
}

/// [`mix`] with the offset in seconds.
fn mix_at(dst: &mut [f32], sr: f32, src: &[f32], at_secs: f32, gain: f32) {
    mix(dst, src, (at_secs.max(0.0) * sr).round() as usize, gain);
}

/// Adds `src * gain` into `dst` with wrap-around (for loops).
fn mix_wrap(dst: &mut [f32], src: &[f32], at: usize, gain: f32) {
    let n = dst.len();
    if n == 0 {
        return;
    }
    for (i, s) in src.iter().enumerate() {
        dst[(at + i) % n] += *s * gain;
    }
}

// ---------------------------------------------------------------------------
// 2.5 Oscillators
// ---------------------------------------------------------------------------

#[allow(dead_code)] // `Pulse` is part of the toolkit
#[derive(Clone, Copy, Debug)]
enum Wave {
    Sine,
    Tri,
    /// polyBLEP band-limited saw.
    Saw,
    /// polyBLEP band-limited square.
    Square,
    /// polyBLEP pulse with the given width (0..1).
    Pulse(f32),
}

#[inline]
fn poly_blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let x = t / dt;
        x + x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + x + x + 1.0
    } else {
        0.0
    }
}

/// Phase-accumulating oscillator (phase in cycles).
#[derive(Clone, Copy)]
struct Osc {
    ph: f32,
}

impl Osc {
    fn new(ph: f32) -> Osc {
        Osc { ph }
    }
    #[inline]
    fn tick(&mut self, wave: Wave, freq: f32, sr: f32) -> f32 {
        let dt = (freq / sr).clamp(1e-6, 0.45);
        let p = self.ph;
        let y = match wave {
            Wave::Sine => sin_cyc(p),
            Wave::Tri => {
                let t = p + 0.25;
                let t = t - t.floor();
                1.0 - 4.0 * (t - 0.5).abs()
            }
            Wave::Saw => 2.0 * p - 1.0 - poly_blep(p, dt),
            Wave::Square => {
                let q = if p + 0.5 >= 1.0 { p - 0.5 } else { p + 0.5 };
                (if p < 0.5 { 1.0 } else { -1.0 }) + poly_blep(p, dt) - poly_blep(q, dt)
            }
            Wave::Pulse(w) => {
                let q = if p + 1.0 - w >= 1.0 { p - w } else { p + 1.0 - w };
                (if p < w { 1.0 } else { -1.0 }) + poly_blep(p, dt) - poly_blep(q, dt)
            }
        };
        self.ph += dt;
        if self.ph >= 1.0 {
            self.ph -= 1.0;
        }
        y
    }
}

/// Renders `dur` seconds of `wave` whose instantaneous frequency is `f(t)`.
fn osc_buf(sr: f32, dur: f32, wave: Wave, f: impl Fn(f32) -> f32) -> Vec<f32> {
    let n = ((dur * sr).round() as usize).max(1);
    let mut o = Osc::new(0.0);
    (0..n).map(|i| o.tick(wave, f(i as f32 / sr), sr)).collect()
}

/// Sine "thump": pitch falls exponentially `f0 -> f1` (time constant `tau_f`) while the
/// amplitude decays with `tau_a`.  Starts at the cosine peak (after a 0.6 ms de-click ramp)
/// for maximum punch.
fn thump(sr: f32, dur: f32, f0: f32, f1: f32, tau_f: f32, tau_a: f32) -> Vec<f32> {
    thump_from(sr, dur, f0, f1, tau_f, tau_a, 0.25)
}

/// [`thump`] with an explicit start phase (cycles): 0.25 starts at the peak (punchy), 0.0 at
/// the zero crossing (a gentle, soft onset).
fn thump_from(sr: f32, dur: f32, f0: f32, f1: f32, tau_f: f32, tau_a: f32, ph0: f32) -> Vec<f32> {
    let n = ((dur * sr).round() as usize).max(1);
    let kf = (-1.0 / (tau_f * sr)).exp();
    let ka = (-1.0 / (tau_a * sr)).exp();
    let att = (0.0006 * sr).max(1.0);
    let (mut df, mut a, mut ph) = (f0 - f1, 1.0f32, ph0);
    (0..n)
        .map(|i| {
            let f = f1 + df;
            df *= kf;
            ph += f / sr;
            if ph >= 1.0 {
                ph -= 1.0;
            }
            let y = sin_cyc(ph) * a * (i as f32 / att).min(1.0);
            a *= ka;
            y
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 2.6 Resonators ("ringing clicks") and bells
// ---------------------------------------------------------------------------

/// Exact damped sinusoid generated by repeated complex rotation: no transcendental calls per
/// sample.  `tick()` returns `amp * exp(-n/(tau*sr)) * sin(w*n + phase)`.
struct Damped {
    re: f32,
    im: f32,
    cr: f32,
    ci: f32,
}

impl Damped {
    fn new(sr: f32, freq: f32, tau: f32, amp: f32, phase: f32) -> Damped {
        let w = TAU * freq / sr;
        let d = (-1.0 / (tau * sr)).exp();
        let (s, c) = (phase.sin(), phase.cos());
        Damped { re: amp * c, im: amp * s, cr: d * w.cos(), ci: d * w.sin() }
    }
    #[inline]
    fn tick(&mut self) -> f32 {
        let out = self.im;
        let re = self.re * self.cr - self.im * self.ci;
        let im = self.re * self.ci + self.im * self.cr;
        self.re = re;
        self.im = im;
        out
    }
}

/// Adds a damped sine ("ringing click" mode) of `freq` Hz with decay time constant `tau`
/// seconds to `dst` starting at sample `at`.  Modes above ~0.45 * sr are skipped.
fn ring(dst: &mut [f32], sr: f32, at: usize, freq: f32, tau: f32, amp: f32) {
    if at >= dst.len() || freq >= 0.45 * sr || freq <= 1.0 || amp == 0.0 {
        return;
    }
    let len = ((tau * sr * 11.5) as usize).clamp(1, dst.len() - at);
    let mut d = Damped::new(sr, freq, tau, amp, 0.0);
    for x in dst[at..at + len].iter_mut() {
        *x += d.tick();
    }
}

/// Modal strike: sums several `(freq Hz, tau s, amp)` modes (see [`ring`]) at `at` seconds.
/// `pitch` scales every frequency and `decay_scale` every decay time.
fn modal(dst: &mut [f32], sr: f32, at: f32, modes: &[(f32, f32, f32)], pitch: f32, decay_scale: f32, gain: f32) {
    let at = (at.max(0.0) * sr).round() as usize;
    for &(f, tau, a) in modes {
        ring(dst, sr, at, f * pitch, tau * decay_scale, a * gain);
    }
}

/// Inharmonic bell/glass partials `(ratio, amplitude, decay multiplier)`.
const BELL: [(f32, f32, f32); 6] = [
    (1.0, 1.0, 1.0),
    (2.0, 0.42, 0.62),
    (2.76, 0.30, 0.45),
    (4.07, 0.16, 0.30),
    (5.4, 0.11, 0.22),
    (8.93, 0.05, 0.12),
];

/// A struck bell/glass tone at `f` Hz: the fundamental rings for `tau` seconds, higher
/// partials die faster.  `bright` (0..1.5) scales the upper partials.
fn bell(dst: &mut [f32], sr: f32, at: f32, f: f32, amp: f32, tau: f32, bright: f32) {
    let at = (at.max(0.0) * sr).round() as usize;
    for (i, &(r, a, d)) in BELL.iter().enumerate() {
        let a = if i == 0 { a } else { a * bright };
        ring(dst, sr, at, f * r, tau * d, amp * a);
    }
}

/// A softer, more harmonic chime/harp pluck: partials 1, 2, 3, 4 with fast-dying upper ones.
fn pluck(dst: &mut [f32], sr: f32, at: f32, f: f32, amp: f32, tau: f32) {
    let at = (at.max(0.0) * sr).round() as usize;
    ring(dst, sr, at, f, tau, amp);
    ring(dst, sr, at, f * 2.0, tau * 0.55, amp * 0.35);
    ring(dst, sr, at, f * 3.0, tau * 0.3, amp * 0.15);
    ring(dst, sr, at, f * 4.01, tau * 0.18, amp * 0.07);
}

// ---------------------------------------------------------------------------
// 2.7 Reverb (Freeverb-style Schroeder network)
// ---------------------------------------------------------------------------

struct Comb {
    buf: Vec<f32>,
    i: usize,
    fb: f32,
    damp: f32,
    lp: f32,
}

impl Comb {
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let y = self.buf[self.i];
        self.lp = y + (self.lp - y) * self.damp;
        self.buf[self.i] = x + self.lp * self.fb;
        self.i += 1;
        if self.i == self.buf.len() {
            self.i = 0;
        }
        y
    }
}

struct Allpass {
    buf: Vec<f32>,
    i: usize,
}

impl Allpass {
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let b = self.buf[self.i];
        let y = b - x;
        self.buf[self.i] = x + b * 0.5;
        self.i += 1;
        if self.i == self.buf.len() {
            self.i = 0;
        }
        y
    }
}

/// 8 damped parallel feedback combs into 4 series all-passes.  Comb gains are chosen from the
/// requested `rt60` so every comb decays at the same rate.
struct Reverb {
    combs: Vec<Comb>,
    aps: Vec<Allpass>,
    norm: f32,
}

impl Reverb {
    fn new(sr: f32, rt60: f32, damp: f32, size: f32) -> Reverb {
        const COMBS: [f32; 8] = [1116.0, 1188.0, 1277.0, 1356.0, 1422.0, 1491.0, 1557.0, 1617.0];
        const APS: [f32; 4] = [556.0, 441.0, 341.0, 225.0];
        let scale = sr / 44_100.0 * size;
        let mut var = 0.0f32;
        let combs = COMBS
            .iter()
            .map(|&l| {
                let len = ((l * scale).round() as usize).max(8);
                let fb = 0.001f32.powf(len as f32 / sr / rt60.max(0.02)).min(0.985);
                var += 1.0 / (1.0 - fb * fb);
                Comb { buf: vec![0.0; len], i: 0, fb, damp, lp: 0.0 }
            })
            .collect();
        let aps = APS
            .iter()
            .map(|&l| Allpass { buf: vec![0.0; ((l * scale).round() as usize).max(4)], i: 0 })
            .collect();
        Reverb { combs, aps, norm: 1.0 / var.sqrt() }
    }
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let mut s = 0.0;
        for c in self.combs.iter_mut() {
            s += c.tick(x);
        }
        s *= self.norm;
        for a in self.aps.iter_mut() {
            s = a.tick(s);
        }
        s
    }
}

/// Reverb send settings.
#[derive(Clone, Copy)]
struct Room {
    /// Time for the tail to decay by 60 dB, seconds.
    rt60: f32,
    /// High-frequency damping inside the feedback loops (0 = bright, 0.9 = dark).
    damp: f32,
    /// Scales all delay lengths (0.5 = small room, 1.0 = Freeverb default, 1.5 = hall).
    size: f32,
    /// Reverb level relative to a steady input of the same RMS.
    wet: f32,
    /// Extra seconds appended to let the reverb ring out.
    tail: f32,
    /// Pre-delay, seconds.
    pre: f32,
    /// High-pass (Hz) on the reverb send: keeps low thumps from exciting comb resonances.
    hp: f32,
}

/// Returns `dry` plus a reverb tail (`Room::tail` seconds longer than `dry`).  When a tail is
/// appended, the dry signal gets a 20 ms fade-out first so a still-sounding layer can never
/// end in a click that the reverb would then ring on.
fn reverb(dry: &[f32], sr: f32, r: Room) -> Vec<f32> {
    let faded;
    let dry = if r.tail > 0.0 {
        faded = dry.to_vec().fade_out(sr, 0.02);
        &faded[..]
    } else {
        dry
    };
    let n = dry.len() + (r.tail * sr) as usize;
    let pre_n = (r.pre * sr) as usize;
    let mut rv = Reverb::new(sr, r.rt60, r.damp, r.size);
    let mut hp = Biquad::new(Kind::Hp, sr, r.hp.max(20.0), BUTTER_Q, 0.0);
    let mut out = vec![0.0f32; n];
    for (i, o) in out.iter_mut().enumerate() {
        let d = if i < dry.len() { dry[i] } else { 0.0 };
        let mut x = if i >= pre_n && i - pre_n < dry.len() { dry[i - pre_n] } else { 0.0 };
        if r.hp > 0.0 {
            x = hp.tick(x);
        }
        *o = d + rv.tick(x) * r.wet;
    }
    out
}

// ---------------------------------------------------------------------------
// 2.8 Seamless loops
// ---------------------------------------------------------------------------

/// Helper for building seamless loops.
///
/// * Periodic (tonal) layers: render `period + warm` samples, call [`LoopSpec::take`].  Use
///   [`LoopSpec::snap`] so every frequency has an integer number of cycles per loop and
///   [`LoopSpec::cyc`] as the time base of LFOs (`k` cycles per loop = exactly periodic).
/// * Noise layers: render [`LoopSpec::total`] samples (modulated only by functions that are
///   periodic in the loop length), then call [`LoopSpec::fold`], which cross-fades the extra
///   tail into the head with equal-power curves.
///
/// `warm` samples at the front let filters settle; they are discarded.
struct LoopSpec {
    sr: f32,
    n: usize,
    xf: usize,
    warm: usize,
}

impl LoopSpec {
    fn new(sr: f32, secs: f32, xf_secs: f32) -> LoopSpec {
        LoopSpec {
            sr,
            n: ((secs * sr).round() as usize).max(16),
            xf: ((xf_secs * sr).round() as usize).max(2),
            warm: (0.25 * sr) as usize,
        }
    }
    /// Samples a noise layer must have (warm-up + loop + cross-fade tail).
    fn total(&self) -> usize {
        self.warm + self.n + self.xf
    }
    /// Exact loop length in seconds.
    fn period(&self) -> f32 {
        self.n as f32 / self.sr
    }
    /// Loop position in cycles for buffer index `i` of a [`LoopSpec::total`]-long buffer
    /// (0 at the first kept sample, so negative during warm-up).
    fn cyc(&self, i: usize) -> f32 {
        (i as f32 - self.warm as f32) / self.n as f32
    }
    /// Loop position in cycles for a time `t` seconds into a `total()`-long buffer.
    fn cyc_t(&self, t: f32) -> f32 {
        (t * self.sr - self.warm as f32) / self.n as f32
    }
    /// Nearest frequency (Hz) with a whole number of cycles per loop.
    #[allow(dead_code)]
    fn snap(&self, f: f32) -> f32 {
        let t = self.period();
        (f * t).round().max(1.0) / t
    }
    /// Integer cycles per loop for the frequency nearest to `f`.
    fn cycles(&self, f: f32) -> f32 {
        (f * self.period()).round().max(1.0)
    }
    /// Phase (cycles, `0..1`) at buffer index `i` of an oscillator that makes `k` cycles per
    /// loop (`k` must be a whole number for the result to be exactly periodic).  Computed in
    /// `f64` so long loops keep sample-exact periodicity.
    fn ph(&self, i: usize, k: f32) -> f32 {
        let x = (i as f64 - self.warm as f64) / self.n as f64;
        (x * k as f64).rem_euclid(1.0) as f32
    }
    /// Sine LFO with `k` cycles per loop and a phase offset (in cycles) at index `i`.
    fn lfo(&self, i: usize, k: f32, phase: f32) -> f32 {
        sin_cyc(self.ph(i, k) + phase)
    }
    /// [`LoopSpec::ph`] for a buffer that starts exactly at the loop start (no warm-up).
    fn ph_l(&self, i: usize, k: f32) -> f32 {
        self.ph(i + self.warm, k)
    }
    /// [`LoopSpec::lfo`] for a buffer that starts exactly at the loop start (no warm-up).
    fn lfo_l(&self, i: usize, k: f32, phase: f32) -> f32 {
        self.lfo(i + self.warm, k, phase)
    }
    /// Pads/truncates a layer to exactly [`LoopSpec::total`] samples.
    fn fit(&self, mut raw: Vec<f32>) -> Vec<f32> {
        raw.resize(self.total(), 0.0);
        raw
    }
    /// Keeps the loop-length part of a periodic layer.
    fn take(&self, raw: &[f32]) -> Vec<f32> {
        raw[self.warm..self.warm + self.n].to_vec()
    }
    /// Folds a `total()`-long noise layer into a seamless `n`-long loop.
    fn fold(&self, raw: &[f32]) -> Vec<f32> {
        debug_assert!(raw.len() >= self.total());
        let base = self.warm;
        let mut out = raw[base..base + self.n].to_vec();
        for i in 0..self.xf {
            let th = (i as f32 + 0.5) / self.xf as f32 * 0.25; // quarter cycle
            out[i] = raw[base + self.n + i] * cos_cyc(th) + raw[base + i] * sin_cyc(th);
        }
        out
    }
}

/// Removes the DC offset and rotates a (conceptually circular) loop so that the seam
/// `last -> first` falls where the signal is smoothest: tiny jump, matching slope.
/// Rotating a seamless loop keeps it seamless, so this only ever improves the seam.
fn polish_loop(buf: &mut [f32]) {
    let n = buf.len();
    if n < 8 {
        return;
    }
    let mean = buf.iter().sum::<f32>() / n as f32;
    for x in buf.iter_mut() {
        *x -= mean;
    }
    // cost of putting the seam before sample r: jump + curvature mismatch
    let at = |i: isize| buf[i.rem_euclid(n as isize) as usize];
    let mut best = (f32::MAX, 0usize);
    // Rotation costs nothing but the audible start phase, so search the whole circle
    // (coarsely stepped for speed) among points whose level is small.
    let step = (n / 4096).max(1);
    let mut r = 0usize;
    while r < n {
        let i = r as isize;
        let jump = (at(i) - at(i - 1)).abs();
        let curv = ((at(i + 1) - at(i)) - (at(i - 1) - at(i - 2))).abs();
        let cost = jump + 0.5 * curv;
        if cost < best.0 {
            best = (cost, r);
        }
        r += step;
    }
    // refine around the best coarse hit
    if step > 1 {
        let lo = best.1.saturating_sub(step);
        let hi = (best.1 + step).min(n - 1);
        for r in lo..=hi {
            let i = r as isize;
            let jump = (at(i) - at(i - 1)).abs();
            let curv = ((at(i + 1) - at(i)) - (at(i - 1) - at(i - 2))).abs();
            let cost = jump + 0.5 * curv;
            if cost < best.0 {
                best = (cost, r);
            }
        }
    }
    buf.rotate_left(best.1);
}

// ============================================================================
// 3. Building blocks shared by several sounds
// ============================================================================

/// Very short high-passed noise tick: the "strike" of a mechanical click.
fn tick_noise(c: &mut Ctx, secs: f32, hp: f32, tau: f32) -> Vec<f32> {
    let sr = c.sr;
    c.white(secs).hp(sr, hp).env(sr, 0.00003, tau)
}

/// Mechanical click at `at` seconds: noise strike + ringing `modes` (`(Hz, tau, amp)`),
/// jittered slightly per call so repeated clicks are not identical.
fn mclick(c: &mut Ctx, dst: &mut [f32], at: f32, gain: f32, tick_hp: f32, modes: &[(f32, f32, f32)]) {
    let sr = c.sr;
    let t = tick_noise(c, 0.004, tick_hp, 0.0007);
    mix_at(dst, sr, &t, at, gain * 0.55);
    let jit = c.rng.range(0.97, 1.03) * c.p;
    modal(dst, sr, at, modes, jit, 1.0, gain);
}

/// Low "thunk" of a body knocking something: a short sine with falling pitch and a soft
/// (zero-crossing) onset.
fn thunk(c: &mut Ctx, dst: &mut [f32], at: f32, gain: f32, f0: f32, f1: f32, tau: f32) {
    let sr = c.sr;
    let t = thump_from(sr, tau * 8.0, f0 * c.p, f1 * c.p, tau * 0.6, tau, 0.0);
    mix_at(dst, sr, &t, at, gain);
}

/// Noise whoosh: band-passed white noise with centre frequency `f(t)` and amplitude `a(t)`.
fn whoosh(c: &mut Ctx, dur: f32, q: f32, f: impl Fn(f32) -> f32, a: impl Fn(f32) -> f32) -> Vec<f32> {
    let sr = c.sr;
    c.white(dur).sweep(sr, Kind::Bp, q, f).env_fn(sr, a)
}

/// Smooth 0 -> 1 -> 0 hump (rise over `rise` seconds, fall over `fall`).
fn hump(t: f32, rise: f32, fall: f32) -> f32 {
    if t < rise {
        smoothstep(0.0, rise, t)
    } else {
        1.0 - smoothstep(rise, rise + fall, t)
    }
}

/// Smooth random control signal in `[-1, 1]` with roughly `rate` new values per second
/// (linearly interpolated sample-and-hold): wobble, jitter, flicker.
fn smooth_noise(c: &mut Ctx, secs: f32, rate: f32) -> Vec<f32> {
    let n = c.n(secs);
    let step = (c.sr / rate.max(0.01)).max(1.0);
    let mut out = Vec::with_capacity(n);
    let (mut a, mut b) = (c.rng.bi(), c.rng.bi());
    let mut pos = 0.0f32;
    for _ in 0..n {
        if pos >= step {
            pos -= step;
            a = b;
            b = c.rng.bi();
        }
        let t = pos / step;
        out.push(lerp(a, b, t * t * (3.0 - 2.0 * t)));
        pos += 1.0;
    }
    out
}

/// Random granular amplitude gate for rustles / crunches: noise through a band-pass whose
/// amplitude flickers like many tiny crackles.  `rate` = grains per second.
fn rustle(c: &mut Ctx, dur: f32, f_lo: f32, f_hi: f32, rate: f32) -> Vec<f32> {
    let sr = c.sr;
    let n = c.n(dur);
    let mut g = smooth_noise(c, dur, rate);
    for x in g.iter_mut() {
        let v = (*x * 0.5 + 0.5).max(0.0);
        *x = v * v;
    }
    let mut y = c.white(dur).hp(sr, f_lo).lp(sr, f_hi);
    for i in 0..n.min(y.len()) {
        y[i] *= g[i] * 2.0;
    }
    y
}

/// Sparse random pops (debris, crackle, sparks): a Poisson process whose rate goes from
/// `rate0` to `rate1` per second over `[t0, t1]`, each pop a short ring at a random
/// frequency in `[f_lo, f_hi]` with amplitude scaled by `amp(t)`.
fn crackle(c: &mut Ctx, dst: &mut [f32], t0: f32, t1: f32, rate0: f32, rate1: f32, f_lo: f32, f_hi: f32, tau: (f32, f32), amp: impl Fn(f32) -> f32) {
    let sr = c.sr;
    let (i0, i1) = ((t0 * sr) as usize, ((t1 * sr) as usize).min(dst.len()));
    for i in i0..i1 {
        let s = (i - i0) as f32 / (i1 - i0).max(1) as f32;
        let rate = rate0 * (rate1 / rate0.max(1e-3)).powf(s);
        if c.rng.chance(rate / sr) {
            let f = geo(f_lo, f_hi, c.rng.f());
            let tau_p = c.rng.range(tau.0, tau.1);
            let a = amp(i as f32 / sr) * c.rng.range(0.25, 1.0);
            ring(dst, sr, i, f, tau_p, a);
            if c.rng.chance(0.5) {
                ring(dst, sr, i, f * c.rng.range(1.9, 3.1), tau_p * 0.6, a * 0.5);
            }
        }
    }
}

/// Short chirp: a sine gliding `f0 -> f1` (geometric) over `dur` seconds with an
/// exponential amplitude decay of constant `tau` (a "bubble" / "bloop" / blip).
fn chirp(dst: &mut [f32], sr: f32, at: f32, dur: f32, f0: f32, f1: f32, tau: f32, amp: f32) {
    let start = (at.max(0.0) * sr).round() as usize;
    if start >= dst.len() {
        return;
    }
    let n = ((dur * sr) as usize).min(dst.len() - start);
    let att = (0.002 * sr).max(1.0);
    let fade = (n / 6).min((0.003 * sr) as usize).max(1);
    let mut ph = 0.0f32;
    for i in 0..n {
        let t = i as f32 / sr;
        let f = geo(f0, f1, t / dur);
        ph += f / sr;
        if ph >= 1.0 {
            ph -= 1.0;
        }
        let mut g = decay(t, tau) * (i as f32 / att).min(1.0);
        if i + fade >= n {
            g *= 0.5 - 0.5 * cos_cyc(0.5 * (n - i) as f32 / fade as f32);
        }
        dst[start + i] += sin_cyc(ph) * amp * g;
    }
}

/// Picks a random pitch (Hz) from a major pentatonic scale rooted at MIDI `root`, between
/// `lo` and `hi` Hz: used for musical sparkles.
fn pentatonic(c: &mut Ctx, root: f32, lo: f32, hi: f32) -> f32 {
    const DEG: [f32; 5] = [0.0, 2.0, 4.0, 7.0, 9.0];
    for _ in 0..16 {
        let oct = c.rng.below(5) as f32 * 12.0;
        let f = hz(root + oct + DEG[c.rng.below(5)]);
        if f >= lo && f <= hi {
            return f;
        }
    }
    lo
}

/// Random high twinkles (bell pings) scattered in `[t0, t1]` at `rate` per second.
fn sparkle(c: &mut Ctx, dst: &mut [f32], t0: f32, t1: f32, rate: f32, lo: f32, hi: f32, amp: f32, tau: f32) {
    let sr = c.sr;
    let mut t = t0;
    while t < t1 {
        t += c.rng.range(0.4, 1.6) / rate;
        if t >= t1 {
            break;
        }
        let f = pentatonic(c, 72.0, lo, hi);
        let a = amp * c.rng.range(0.4, 1.0);
        let tau_n = tau * c.rng.range(0.7, 1.3);
        let at = (t * sr) as usize;
        ring(dst, sr, at, f, tau_n, a);
        ring(dst, sr, at, f * 2.01, tau_n * 0.5, a * 0.3);
    }
}

// ----- musical voices --------------------------------------------------------

/// Detuned "super-saw brass" note rendered into `dst` at `at` seconds: three polyBLEP saws
/// (+-9 cents) and a sub square through a resonant low-pass whose cut-off blats open and
/// settles, with an ADSR amplitude.  `dur` is the gate time, `rel` the release time.
fn brass(dst: &mut [f32], sr: f32, at: f32, f: f32, dur: f32, rel: f32, amp: f32, bright: f32) {
    let start = (at.max(0.0) * sr) as usize;
    if start >= dst.len() {
        return;
    }
    let n = (((dur + rel) * sr) as usize).min(dst.len() - start);
    let mut o = [Osc::new(0.0), Osc::new(0.31), Osc::new(0.67), Osc::new(0.0)];
    let det = [cents(-9.0), 1.0, cents(9.0)];
    let mut bq = Biquad::new(Kind::Lp, sr, 1000.0, 0.9, 0.0);
    for i in 0..n {
        let t = i as f32 / sr;
        if i % SWEEP_STEP == 0 {
            let fc = (f * (2.2 + 5.0 * decay(t, 0.14)) * bright).min(9000.0);
            bq.set(Kind::Lp, sr, fc, 0.9, 0.0);
        }
        let mut s = 0.0;
        for k in 0..3 {
            s += o[k].tick(Wave::Saw, f * det[k], sr);
        }
        s += 0.6 * o[3].tick(Wave::Square, f * 0.5, sr);
        let y = bq.tick(s * 0.28);
        dst[start + i] += y * amp * adsr(t, 0.018, 0.16, 0.78, dur, rel);
    }
}

/// Soft pad note: two detuned triangles + a sine an octave up, slow attack.
fn pad(dst: &mut [f32], sr: f32, at: f32, f: f32, dur: f32, rel: f32, amp: f32, att: f32, vib: f32) {
    let start = (at.max(0.0) * sr) as usize;
    if start >= dst.len() {
        return;
    }
    let n = (((dur + rel) * sr) as usize).min(dst.len() - start);
    let mut o = [Osc::new(0.0), Osc::new(0.4), Osc::new(0.7)];
    for i in 0..n {
        let t = i as f32 / sr;
        let v = 1.0 + vib * sin_cyc(5.2 * t) * smoothstep(0.0, 0.4, t);
        let s = o[0].tick(Wave::Tri, f * v * cents(-5.0), sr)
            + o[1].tick(Wave::Tri, f * v * cents(5.0), sr)
            + 0.35 * o[2].tick(Wave::Sine, 2.0 * f * v, sr);
        dst[start + i] += s * 0.4 * amp * adsr(t, att, 0.3, 0.85, dur, rel);
    }
}

/// Timpani / boom hit: pitched sine thump + a puff of low noise.
fn boom(c: &mut Ctx, dst: &mut [f32], at: f32, gain: f32, f0: f32, f1: f32, tau: f32) {
    let sr = c.sr;
    let t = thump(sr, tau * 6.0, f0, f1, tau * 0.35, tau).clip(1.5);
    mix_at(dst, sr, &t, at, gain);
    let nz = c.white(tau).lp(sr, 600.0).env(sr, 0.001, tau * 0.25);
    mix_at(dst, sr, &nz, at, gain * 0.5);
}

/// Cymbal-ish noise swell/crash: high-passed noise with a given rise and decay.
fn cymbal(c: &mut Ctx, dst: &mut [f32], at: f32, gain: f32, rise: f32, tau: f32) {
    let sr = c.sr;
    let dur = rise + tau * 5.0;
    let nz = c.white(dur).hp(sr, 5500.0).peak(sr, 9500.0, 1.2, 4.0).env_fn(sr, move |t| {
        if t < rise {
            let x = t / rise;
            x * x
        } else {
            decay(t - rise, tau)
        }
    });
    mix_at(dst, sr, &nz, at, gain);
}

/// Stick-slip creak (hinges, straining wood, branches): a wobbling-pitch saw squeezed through a
/// formant resonance and gated by random "slips".
fn creak(c: &mut Ctx, dur: f32, f0: f32, f1: f32, formant: f32, wobble: f32) -> Vec<f32> {
    let sr = c.sr;
    let n = c.n(dur);
    let jit = smooth_noise(c, dur, 30.0);
    let slip = smooth_noise(c, dur, 13.0);
    let mut o = Osc::new(0.0);
    let y: Vec<f32> = (0..n)
        .map(|i| {
            let f = geo(f0, f1, i as f32 / n as f32) * (1.0 + wobble * jit[i]);
            o.tick(Wave::Saw, f, sr)
        })
        .collect();
    let mut y = y.peak(sr, formant, 2.5, 14.0).lp(sr, formant * 3.0);
    for (x, s) in y.iter_mut().zip(&slip) {
        let g = 0.5 + 0.5 * s;
        *x *= 0.35 + 0.65 * g * g;
    }
    y.fade_in(sr, 0.01).fade_out(sr, 0.02)
}

/// Shimmering partials: sines with individually phased tremolo, swelling in over `rise`
/// seconds and out over the rest of `dur`.
fn shimmer(dst: &mut [f32], sr: f32, at: f32, dur: f32, freqs: &[f32], amp: f32, rise: f32, tremolo: f32) {
    let start = (at.max(0.0) * sr) as usize;
    if start >= dst.len() {
        return;
    }
    let n = ((dur * sr) as usize).min(dst.len() - start);
    for (k, &f) in freqs.iter().enumerate() {
        let mut ph = 0.0f32;
        let ph0 = k as f32 * 0.37;
        for i in 0..n {
            let t = i as f32 / sr;
            ph += f / sr;
            if ph >= 1.0 {
                ph -= 1.0;
            }
            let trem = 0.55 + 0.45 * sin_cyc(tremolo * (1.0 + 0.13 * k as f32) * t + ph0);
            dst[start + i] += sin_cyc(ph) * amp * trem * hump(t, rise, dur - rise);
        }
    }
}

/// Falling debris: random short band-passed noise bursts (rate goes `rate0 -> rate1` per
/// second over `[t0, t1]`) with amplitude `amp(t)`.
fn debris(c: &mut Ctx, dst: &mut [f32], t0: f32, t1: f32, rate0: f32, rate1: f32, f_lo: f32, f_hi: f32, amp: impl Fn(f32) -> f32) {
    let sr = c.sr;
    let mut t = t0;
    while t < t1 {
        let s = (t - t0) / (t1 - t0);
        let rate = rate0 * (rate1 / rate0).powf(s);
        t += c.rng.range(0.5, 1.5) / rate;
        if t >= t1 {
            break;
        }
        let f = geo(f_lo, f_hi, c.rng.f());
        let len = c.rng.range(0.008, 0.035);
        let a = amp(t) * c.rng.range(0.3, 1.0);
        let k = c.white(len * 1.5).bp(sr, f, 1.3).env(sr, 0.0008, len * 0.3);
        mix_at(dst, sr, &k, t, a);
    }
}

/// Snare / clap hit: noise burst + short body tone.
fn snare(c: &mut Ctx, dst: &mut [f32], at: f32, gain: f32) {
    let sr = c.sr;
    let k = c.white(0.3).hp(sr, 1400.0).lp(sr, 9000.0).env(sr, 0.0005, 0.06);
    mix_at(dst, sr, &k, at, gain * 0.8);
    let k = thump(sr, 0.2, 260.0, 170.0, 0.02, 0.035).clip(1.3);
    mix_at(dst, sr, &k, at, gain * 0.5);
}

/// Distant horn / siren tone: three detuned saws through a vowel-like resonance, a slow
/// swell and a vibrato that grows in.
fn horn(dst: &mut [f32], sr: f32, at: f32, f: f32, dur: f32, rel: f32, amp: f32) {
    let start = (at.max(0.0) * sr) as usize;
    if start >= dst.len() {
        return;
    }
    let n = (((dur + rel) * sr) as usize).min(dst.len() - start);
    let mut o = [Osc::new(0.0), Osc::new(0.4), Osc::new(0.8)];
    let det = [cents(-7.0), 1.0, cents(7.0)];
    let mut lp = Biquad::new(Kind::Lp, sr, 1100.0, 1.1, 0.0);
    let mut fm = Biquad::new(Kind::Peak, sr, 620.0, 2.0, 9.0);
    for i in 0..n {
        let t = i as f32 / sr;
        let vib = 1.0 + 0.007 * sin_cyc(5.2 * t) * smoothstep(0.15, 0.8, t);
        let mut s = 0.0;
        for k in 0..3 {
            s += o[k].tick(Wave::Saw, f * vib * det[k], sr);
        }
        let y = fm.tick(lp.tick(s * 0.33));
        let env = adsr(t, 0.45, 0.2, 0.85, dur, rel);
        dst[start + i] += y * amp * env;
    }
}

// ============================================================================
// 4. Sound recipes
// ============================================================================

// ---------------------------------------------------------------------------
// 4.1 Weapons
// ---------------------------------------------------------------------------

/// Parameters of the layered "gun" recipe shared by AR, SMG, pistol and shotgun:
/// click + crack (bright band-passed noise) + blast (noise through a low-pass that sweeps
/// from bright to dark) + snap (pitched mid "pok") + thump (falling sine) + airy tail, plus an
/// optional pitched pop / metallic ring, glued by a small room and a soft clip.
struct Gun {
    dur: f32,
    click_hp: f32,
    click: f32,
    crack_f: f32,
    crack_q: f32,
    crack_tau: f32,
    crack: f32,
    /// `(low-pass start Hz, low-pass end Hz, sweep tau, amplitude tau, gain)`
    blast: (f32, f32, f32, f32, f32),
    /// `(start Hz, end Hz, amplitude tau, gain)`
    snap: (f32, f32, f32, f32),
    /// `(start Hz, end Hz, pitch tau, amplitude tau, gain)`
    thump: (f32, f32, f32, f32, f32),
    /// `(high-pass Hz, low-pass Hz, amplitude tau, gain)`
    tail: (f32, f32, f32, f32),
    pop: f32,
    ring: f32,
    room: Room,
    drive: f32,
}

fn gun(c: &mut Ctx, g: &Gun) -> Vec<f32> {
    let sr = c.sr;
    let (p, b) = (c.p, c.b);
    let mut out = c.silence(g.dur);
    // 1. broadband transient tick
    let k = c.white(0.006).hp(sr, g.click_hp * b).env(sr, 0.00004, 0.0007);
    mix(&mut out, &k, 0, g.click);
    // 2. crack: band-passed noise burst, saturated
    let k = c
        .white(g.crack_tau * 9.0)
        .bp(sr, g.crack_f * b, g.crack_q)
        .env(sr, 0.00008, g.crack_tau)
        .clip(2.2);
    mix(&mut out, &k, 0, g.crack);
    // 3. blast: noise through a low-pass sweeping bright -> dark
    let (hi, lo, tf, ta, gain) = g.blast;
    let k = c
        .white(ta * 9.0)
        .sweep(sr, Kind::Lp, 0.8, move |t| b * (lo + (hi - lo) * decay(t, tf)))
        .env(sr, 0.0003, ta);
    mix(&mut out, &k, 0, gain);
    // 4. snap: pitched mid "pok"
    let (f0, f1, tau, gain) = g.snap;
    if gain > 0.0 {
        let k = thump(sr, tau * 8.0, f0 * p, f1 * p, tau * 0.7, tau).clip(1.5);
        mix(&mut out, &k, 0, gain);
    }
    // 5. thump: falling sine, saturated so its harmonics survive small speakers
    let (f0, f1, tf, ta, gain) = g.thump;
    let k = thump(sr, ta * 7.0, f0 * p, f1 * p, tf, ta).clip(1.8);
    mix(&mut out, &k, 0, gain);
    // 6. pitched "pop" (pistol snap)
    if g.pop > 0.0 {
        let mut k = c.silence(0.06);
        chirp(&mut k, sr, 0.0, 0.05, 1300.0 * p, 380.0 * p, 0.011, 1.0);
        mix(&mut out, &k, 0, g.pop);
    }
    // 7. small metallic ring
    if g.ring > 0.0 {
        ring(&mut out, sr, 0, 1900.0 * p, 0.035, g.ring);
        ring(&mut out, sr, 0, 3050.0 * p, 0.02, g.ring * 0.5);
    }
    // 8. airy tail
    let (hp, lp, tau, gain) = g.tail;
    let k = c.pink(tau * 6.0).hp(sr, hp).lp(sr, lp * b).env(sr, 0.004, tau);
    mix_at(&mut out, sr, &k, 0.003, gain);
    // 9. room + saturation
    reverb(&out, sr, g.room).clip(g.drive)
}

/// Assault rifle: tight punchy crack, 60-180 Hz thump, short airy tail (~0.28 s).
fn shot_ar(c: &mut Ctx) -> Vec<f32> {
    gun(
        c,
        &Gun {
            dur: 0.29,
            click_hp: 1800.0,
            click: 0.4,
            crack_f: 3300.0,
            crack_q: 0.8,
            crack_tau: 0.0045,
            crack: 1.0,
            blast: (6000.0, 600.0, 0.008, 0.022, 1.0),
            snap: (430.0, 160.0, 0.012, 0.45),
            thump: (150.0, 60.0, 0.010, 0.024, 0.5),
            tail: (500.0, 3500.0, 0.07, 0.3),
            pop: 0.0,
            ring: 0.0,
            room: Room { rt60: 0.25, damp: 0.65, size: 0.6, wet: 0.12, tail: 0.0, pre: 0.0, hp: 250.0 },
            drive: 1.3,
        },
    )
}

/// SMG: shorter, higher and lighter (~0.16 s).
fn shot_smg(c: &mut Ctx) -> Vec<f32> {
    gun(
        c,
        &Gun {
            dur: 0.17,
            click_hp: 2500.0,
            click: 0.4,
            crack_f: 4200.0,
            crack_q: 0.9,
            crack_tau: 0.0032,
            crack: 0.9,
            blast: (6500.0, 900.0, 0.007, 0.018, 0.8),
            snap: (520.0, 220.0, 0.012, 0.4),
            thump: (200.0, 80.0, 0.008, 0.02, 0.35),
            tail: (700.0, 4800.0, 0.04, 0.3),
            pop: 0.0,
            ring: 0.0,
            room: Room { rt60: 0.15, damp: 0.65, size: 0.5, wet: 0.08, tail: 0.0, pre: 0.0, hp: 300.0 },
            drive: 1.3,
        },
    )
}

/// Pistol: snappy pop with a little metallic ring (~0.22 s).
fn shot_pistol(c: &mut Ctx) -> Vec<f32> {
    gun(
        c,
        &Gun {
            dur: 0.23,
            click_hp: 2500.0,
            click: 0.6,
            crack_f: 3400.0,
            crack_q: 0.8,
            crack_tau: 0.0045,
            crack: 1.0,
            blast: (7500.0, 700.0, 0.008, 0.020, 0.9),
            snap: (480.0, 170.0, 0.012, 0.5),
            thump: (170.0, 66.0, 0.010, 0.025, 0.5),
            tail: (600.0, 4000.0, 0.06, 0.3),
            pop: 0.4,
            ring: 0.13,
            room: Room { rt60: 0.3, damp: 0.65, size: 0.7, wet: 0.12, tail: 0.0, pre: 0.0, hp: 250.0 },
            drive: 1.3,
        },
    )
}

/// Shotgun: huge boom - low-passed noise burst, 70 Hz thump, decaying noise tail with a
/// comb-based room (~0.7 s).
fn shot_shotgun(c: &mut Ctx) -> Vec<f32> {
    gun(
        c,
        &Gun {
            dur: 0.7,
            click_hp: 1500.0,
            click: 0.7,
            crack_f: 1900.0,
            crack_q: 0.5,
            crack_tau: 0.009,
            crack: 1.0,
            blast: (6000.0, 300.0, 0.025, 0.06, 1.1),
            snap: (300.0, 110.0, 0.025, 0.5),
            thump: (120.0, 48.0, 0.025, 0.07, 0.8),
            tail: (150.0, 3000.0, 0.2, 0.5),
            pop: 0.0,
            ring: 0.0,
            room: Room { rt60: 0.7, damp: 0.5, size: 1.0, wet: 0.3, tail: 0.0, pre: 0.0, hp: 120.0 },
            drive: 1.4,
        },
    )
}

/// Sniper rifle: massive crack, deep boom, slap-back echoes and a long reverb tail (~1.4 s).
fn shot_sniper(c: &mut Ctx) -> Vec<f32> {
    let sr = c.sr;
    let (p, b) = (c.p, c.b);
    let mut dry = c.silence(0.85);
    // crack
    let k = c.white(0.1).bp(sr, 2200.0 * b, 0.6).env(sr, 0.00006, 0.008).clip(2.5);
    mix(&mut dry, &k, 0, 1.0);
    let k = tick_noise(c, 0.01, 1800.0, 0.0012);
    mix(&mut dry, &k, 0, 0.8);
    // blast
    let k = c
        .white(0.6)
        .sweep(sr, Kind::Lp, 0.8, move |t| b * (250.0 + 6000.0 * decay(t, 0.035)))
        .env(sr, 0.0004, 0.12);
    mix(&mut dry, &k, 0, 1.2);
    // mid snap + deep boom
    let k = thump(sr, 0.2, 320.0 * p, 100.0 * p, 0.03, 0.04).clip(1.5);
    mix(&mut dry, &k, 0, 0.5);
    let k = thump(sr, 1.0, 105.0 * p, 40.0 * p, 0.05, 0.14).clip(2.0);
    mix(&mut dry, &k, 0, 0.8);
    // tail
    let k = c.pink(0.9).hp(sr, 150.0).lp(sr, 2400.0).env(sr, 0.01, 0.2);
    mix_at(&mut dry, sr, &k, 0.01, 0.4);
    // distant slap-back echoes: darker delayed copies of the early part
    let early = dry[..(0.3 * sr) as usize].to_vec().lp(sr, 1800.0).fade_out(sr, 0.05);
    mix_at(&mut dry, sr, &early, 0.23, 0.3);
    let early2 = early.lp(sr, 900.0);
    mix_at(&mut dry, sr, &early2, 0.47, 0.5);
    // long reverb tail
    let room = Room { rt60: 1.3, damp: 0.55, size: 1.4, wet: 0.3, tail: 0.55, pre: 0.012, hp: 150.0 };
    reverb(&dry, sr, room).clip(1.2)
}

/// Rocket launch: ignition thump + whoosh (noise through a rising then falling band-pass)
/// (~0.9 s).
fn rocket_fire(c: &mut Ctx) -> Vec<f32> {
    let sr = c.sr;
    let (p, b) = (c.p, c.b);
    let mut out = c.silence(0.9);
    // ignition: thump, foom, pop
    let k = thump(sr, 0.3, 120.0 * p, 55.0 * p, 0.035, 0.07).clip(1.8);
    mix(&mut out, &k, 0, 0.6);
    let k = c.white(0.3).lp(sr, 520.0).env(sr, 0.002, 0.07);
    mix(&mut out, &k, 0, 0.6);
    let k = tick_noise(c, 0.012, 1200.0, 0.0015);
    mix(&mut out, &k, 0, 0.5);
    let k = c.white(0.06).bp(sr, 2500.0 * b, 0.7).env(sr, 0.0002, 0.006).clip(2.0);
    mix(&mut out, &k, 0, 0.4);
    // whoosh: a band-pass sweeping up quickly, then falling away as the rocket leaves
    let path = move |t: f32| {
        b * if t < 0.3 {
            geo(350.0, 3000.0, t / 0.3)
        } else {
            geo(3000.0, 650.0, (t - 0.3) / 0.6)
        }
    };
    let amp = |t: f32| smoothstep(0.0, 0.05, t) * decay(t, 0.4);
    let w = whoosh(c, 0.9, 1.8, path, amp).clip(1.6);
    mix(&mut out, &w, 0, 1.6);
    // a darker, wider layer one octave down for body
    let w = whoosh(c, 0.9, 1.0, move |t| 0.45 * path(t), amp).clip(1.4);
    mix(&mut out, &w, 0, 1.0);
    // burning-propellant hiss with a flicker
    let fl = smooth_noise(c, 0.9, 70.0);
    let mut h = c.white(0.9).hp(sr, 2500.0).lp(sr, 9000.0).env(sr, 0.01, 0.5);
    for (x, f) in h.iter_mut().zip(&fl) {
        *x *= 0.6 + 0.4 * f;
    }
    mix(&mut out, &h, 0, 0.12);
    let room = Room { rt60: 0.5, damp: 0.5, size: 0.9, wet: 0.15, tail: 0.0, pre: 0.0, hp: 200.0 };
    reverb(&out, sr, room).clip(1.1)
}

/// Explosion: bright crack, fireball (noise whose low-pass sweeps down), deep rumble, sub
/// sweep 90 -> 30 Hz and a long crackling debris tail (~2.0 s).
fn explosion(c: &mut Ctx) -> Vec<f32> {
    let sr = c.sr;
    let (p, b) = (c.p, c.b);
    let mut out = c.silence(2.0);
    // initial crack
    let k = c.white(0.08).bp(sr, 2400.0 * b, 0.5).env(sr, 0.00008, 0.011).clip(2.5);
    mix(&mut out, &k, 0, 1.0);
    let k = tick_noise(c, 0.01, 1000.0, 0.0015);
    mix(&mut out, &k, 0, 0.8);
    // fireball: bright -> dark
    let k = c
        .white(1.4)
        .sweep(sr, Kind::Lp, 0.8, move |t| 350.0 + 6500.0 * b * decay(t, 0.13))
        .env(sr, 0.0008, 0.25)
        .clip(1.5);
    mix(&mut out, &k, 0, 1.0);
    // rumble
    let k = c.brown(2.0).lp(sr, 170.0).hp(sr, 22.0).norm(1.0).env(sr, 0.02, 0.5).clip(1.6);
    mix(&mut out, &k, 0, 1.0);
    // sub sweep
    let k = thump(sr, 1.7, 95.0 * p, 28.0 * p, 0.25, 0.4).clip(1.8);
    mix(&mut out, &k, 0, 0.9);
    // mid body
    let k = c.pink(1.2).lp(sr, 700.0).env(sr, 0.004, 0.22);
    mix(&mut out, &k, 0, 0.7);
    // falling debris: noisy chunks plus sparkly crackle
    debris(c, &mut out, 0.15, 1.9, 110.0, 8.0, 700.0, 5000.0, |t| 0.5 * decay(t, 0.8));
    crackle(c, &mut out, 0.12, 1.9, 200.0, 12.0, 1500.0, 7000.0, (0.0006, 0.002), |t| 0.45 * decay(t, 0.9));
    let room = Room { rt60: 1.0, damp: 0.45, size: 1.1, wet: 0.14, tail: 0.0, pre: 0.0, hp: 120.0 };
    reverb(&out, sr, room).clip(1.1)
}

// ---------------------------------------------------------------------------
// 4.2 Weapon handling
// ---------------------------------------------------------------------------

/// Magazine reload: release click, mag slide out, slide in, firm insertion clack, charging
/// handle ratchet and bolt slam (~1.1 s).
fn reload_mag(c: &mut Ctx) -> Vec<f32> {
    let sr = c.sr;
    let mut out = c.silence(1.1);
    // magazine release button + the mag dropping a hair
    mclick(c, &mut out, 0.0, 0.8, 1800.0, &[(2900.0, 0.005, 0.6), (1800.0, 0.008, 0.5), (4300.0, 0.003, 0.25)]);
    thunk(c, &mut out, 0.012, 0.35, 420.0, 250.0, 0.012);
    // mag slides out (friction scrape, pitch falling)
    let k = c
        .white(0.2)
        .sweep(sr, Kind::Bp, 2.0, |t| geo(2200.0, 1100.0, t / 0.2))
        .env_fn(sr, |t| hump(t, 0.03, 0.17));
    mix_at(&mut out, sr, &k, 0.07, 0.5);
    // fresh mag slides in (pitch rising)
    let k = c
        .white(0.14)
        .sweep(sr, Kind::Bp, 2.0, |t| geo(900.0, 2400.0, t / 0.14))
        .env_fn(sr, |t| hump(t, 0.1, 0.04));
    mix_at(&mut out, sr, &k, 0.42, 0.5);
    // firm insertion clack
    mclick(c, &mut out, 0.575, 1.0, 1200.0, &[(1400.0, 0.012, 0.6), (2600.0, 0.008, 0.5), (3500.0, 0.005, 0.3), (950.0, 0.02, 0.35)]);
    thunk(c, &mut out, 0.575, 0.7, 260.0, 150.0, 0.02);
    // charging handle ratchet: tr-r-r-rk
    let mut t = 0.75;
    for i in 0..5 {
        let g = 0.3 + 0.07 * i as f32;
        mclick(c, &mut out, t, g, 2500.0, &[(2400.0, 0.004, 0.6), (3400.0, 0.003, 0.4), (1500.0, 0.006, 0.3)]);
        t += 0.024 - 0.002 * i as f32;
    }
    // bolt slams forward
    mclick(c, &mut out, 0.93, 0.95, 1400.0, &[(1250.0, 0.016, 0.6), (2300.0, 0.01, 0.5), (3300.0, 0.006, 0.3), (850.0, 0.025, 0.4)]);
    thunk(c, &mut out, 0.93, 0.55, 230.0, 120.0, 0.02);
    out.clip(1.1)
}

/// One shell dropping into the tube: brass tick, two little bounces, plastic seating (~0.35 s).
fn reload_shell(c: &mut Ctx) -> Vec<f32> {
    let sr = c.sr;
    let mut out = c.silence(0.35);
    mclick(c, &mut out, 0.0, 0.8, 2500.0, &[(3700.0, 0.012, 0.5), (2400.0, 0.02, 0.6), (5100.0, 0.006, 0.25)]);
    mclick(c, &mut out, 0.066, 0.3, 2500.0, &[(3500.0, 0.01, 0.5), (2300.0, 0.015, 0.5)]);
    mclick(c, &mut out, 0.106, 0.13, 2500.0, &[(3600.0, 0.008, 0.5), (2450.0, 0.012, 0.5)]);
    // seating in the magazine tube
    mclick(c, &mut out, 0.17, 0.7, 1000.0, &[(1300.0, 0.006, 0.5), (2100.0, 0.004, 0.3)]);
    thunk(c, &mut out, 0.17, 0.5, 300.0, 180.0, 0.012);
    let k = c.white(0.05).lp(sr, 1800.0).env(sr, 0.004, 0.015);
    mix_at(&mut out, sr, &k, 0.16, 0.12);
    out
}

/// Pump-action shotgun "ka-chunk" (~0.4 s).
fn pump_action(c: &mut Ctx) -> Vec<f32> {
    let sr = c.sr;
    let mut out = c.silence(0.4);
    // slide back: "ka"
    mclick(c, &mut out, 0.0, 0.9, 1600.0, &[(1800.0, 0.01, 0.6), (3100.0, 0.006, 0.4), (1100.0, 0.014, 0.4)]);
    thunk(c, &mut out, 0.0, 0.4, 420.0, 250.0, 0.012);
    // metal-on-metal friction while the slide travels
    let fr = c
        .white(0.17)
        .bp(sr, 1300.0, 2.0)
        .env_fn(sr, |t| hump(t, 0.02, 0.15) * (0.55 + 0.45 * (2.0 * sin_cyc(70.0 * t) * 0.5 + 0.5)));
    mix_at(&mut out, sr, &fr, 0.04, 0.35);
    // slide forward: "chunk"
    thunk(c, &mut out, 0.215, 1.0, 220.0, 110.0, 0.03);
    mclick(c, &mut out, 0.215, 1.0, 1100.0, &[(1250.0, 0.02, 0.6), (2200.0, 0.014, 0.45), (900.0, 0.03, 0.4), (3100.0, 0.006, 0.2)]);
    let k = c.white(0.06).lp(sr, 3000.0).env(sr, 0.0005, 0.012);
    mix_at(&mut out, sr, &k, 0.215, 0.5);
    out.clip(1.1)
}

/// Dry dull click of a hammer falling on an empty chamber (~0.1 s).
fn empty_click(c: &mut Ctx) -> Vec<f32> {
    let sr = c.sr;
    let p = c.p;
    let mut out = c.silence(0.1);
    let k = c.white(0.01).lp(sr, 3000.0).hp(sr, 600.0).env(sr, 0.00005, 0.0012);
    mix(&mut out, &k, 0, 0.8);
    modal(&mut out, sr, 0.0, &[(1100.0, 0.004, 0.5), (1900.0, 0.003, 0.25), (2750.0, 0.002, 0.15)], p, 1.0, 1.0);
    thunk(c, &mut out, 0.0, 0.5, 330.0, 200.0, 0.008);
    // faint trigger-reset tick
    mclick(c, &mut out, 0.052, 0.22, 1500.0, &[(1500.0, 0.004, 0.5), (2400.0, 0.003, 0.3)]);
    out
}

/// Weapon swap: cloth swish plus a quick metal rattle (~0.2 s).
fn weapon_swap(c: &mut Ctx) -> Vec<f32> {
    let sr = c.sr;
    let mut out = c.silence(0.2);
    let k = c
        .white(0.12)
        .sweep(sr, Kind::Bp, 0.8, |t| geo(1200.0, 3200.0, t / 0.12))
        .env_fn(sr, |t| hump(t, 0.03, 0.09));
    mix(&mut out, &k, 0, 0.35);
    let k = rustle(c, 0.1, 1500.0, 6000.0, 90.0).env_fn(sr, |t| hump(t, 0.02, 0.08));
    mix(&mut out, &k, 0, 0.3);
    let mut t = 0.045;
    for (i, g) in [0.6f32, 0.45, 0.32].iter().enumerate() {
        mclick(c, &mut out, t, *g, 2000.0, &[(2200.0, 0.008, 0.5), (3100.0, 0.005, 0.4), (1500.0, 0.01, 0.3)]);
        t += 0.032 + 0.006 * i as f32;
    }
    thunk(c, &mut out, 0.13, 0.35, 330.0, 190.0, 0.01);
    out
}

// ---------------------------------------------------------------------------
// 4.3 Movement
// ---------------------------------------------------------------------------

/// Grass step: soft low-passed swish + faint crunch (~0.16 s).
fn step_grass(c: &mut Ctx) -> Vec<f32> {
    let sr = c.sr;
    let b = c.b;
    let mut out = c.silence(0.16);
    // soft swish
    let k = c
        .pink(0.16)
        .hp(sr, 250.0)
        .sweep(sr, Kind::Lp, 0.7, move |t| b * (2600.0 - 1400.0 * smoothstep(0.0, 0.12, t)))
        .env(sr, 0.012, 0.045);
    mix(&mut out, &k, 0, 1.0);
    // faint crunch: a few tiny grass-blade clicks right after the contact
    let mut t = 0.004;
    for _ in 0..6 {
        let g = c.rng.range(0.15, 0.4);
        let f = c.rng.range(2500.0, 5500.0) * b;
        let k = c.white(0.004).bp(sr, f, 1.5).env(sr, 0.0002, 0.0012);
        mix_at(&mut out, sr, &k, t, g);
        t += c.rng.range(0.004, 0.014);
    }
    // body weight
    thunk(c, &mut out, 0.0, 0.3, 120.0, 70.0, 0.02);
    out
}

/// Sand step: grainy hiss + soft thud (~0.17 s).
fn step_sand(c: &mut Ctx) -> Vec<f32> {
    let sr = c.sr;
    let b = c.b;
    let mut out = c.silence(0.17);
    let k = rustle(c, 0.17, 900.0 * b, 3800.0 * b, 650.0).env_fn(sr, |t| smoothstep(0.0, 0.012, t) * decay(t, 0.05));
    mix(&mut out, &k, 0, 1.0);
    let k = c.white(0.17).bp(sr, 2000.0 * b, 0.6).env(sr, 0.01, 0.04);
    mix(&mut out, &k, 0, 0.18);
    thunk(c, &mut out, 0.0, 0.5, 110.0, 60.0, 0.025);
    out
}

/// Stone step: harder tick with a little resonance (~0.14 s).
fn step_stone(c: &mut Ctx) -> Vec<f32> {
    let sr = c.sr;
    let b = c.b;
    let mut out = c.silence(0.14);
    mclick(c, &mut out, 0.0, 1.0, 2500.0, &[(1650.0, 0.010, 0.5), (3300.0, 0.005, 0.3), (2400.0, 0.007, 0.25), (900.0, 0.012, 0.3)]);
    thunk(c, &mut out, 0.0, 0.22, 170.0, 100.0, 0.015);
    let k = c.white(0.05).bp(sr, 3200.0 * b, 0.9).env(sr, 0.002, 0.014);
    mix(&mut out, &k, 0, 0.15);
    out
}

/// Wood step: hollow knock around 200-400 Hz with a short body (~0.16 s).
fn step_wood(c: &mut Ctx) -> Vec<f32> {
    let sr = c.sr;
    let (p, b) = (c.p, c.b);
    let mut out = c.silence(0.16);
    modal(&mut out, sr, 0.0, &[(230.0, 0.024, 0.8), (340.0, 0.016, 0.55), (560.0, 0.010, 0.3), (850.0, 0.006, 0.2)], p, 1.0, 1.0);
    let k = c.white(0.01).lp(sr, 2200.0 * b).env(sr, 0.0001, 0.0015);
    mix(&mut out, &k, 0, 0.7);
    thunk(c, &mut out, 0.0, 0.45, 140.0, 90.0, 0.02);
    out
}

/// Metal step: clank with ringing 800-2000 Hz (~0.18 s).
fn step_metal(c: &mut Ctx) -> Vec<f32> {
    let mut out = c.silence(0.18);
    mclick(c, &mut out, 0.0, 0.8, 3000.0, &[(870.0, 0.032, 0.5), (1370.0, 0.022, 0.5), (1930.0, 0.016, 0.35), (2650.0, 0.009, 0.25), (3600.0, 0.005, 0.15)]);
    thunk(c, &mut out, 0.0, 0.3, 130.0, 80.0, 0.012);
    out
}

/// Water step: noise splash + bubbly random chirps (~0.18 s).
fn step_water(c: &mut Ctx) -> Vec<f32> {
    let sr = c.sr;
    let (p, b) = (c.p, c.b);
    let mut out = c.silence(0.18);
    let k = c.white(0.18).bp(sr, 1800.0 * b, 0.6).env(sr, 0.006, 0.045);
    mix(&mut out, &k, 0, 0.7);
    let k = c.white(0.15).lp(sr, 600.0 * b).env(sr, 0.01, 0.05);
    mix(&mut out, &k, 0, 0.5);
    // bubbles
    for _ in 0..c.rng.below(3) + 4 {
        let at = c.rng.range(0.008, 0.11);
        let f0 = c.rng.range(500.0, 1400.0) * p;
        let rise = c.rng.range(1.4, 2.2);
        let amp = c.rng.range(0.15, 0.35);
        chirp(&mut out, sr, at, 0.04, f0, f0 * rise, 0.012, amp);
    }
    // droplets
    for _ in 0..3 {
        let at = c.rng.range(0.03, 0.14);
        let f0 = c.rng.range(2000.0, 3600.0) * p;
        chirp(&mut out, sr, at, 0.02, f0, f0 * 1.5, 0.006, 0.08);
    }
    out
}

/// Jump: cloth rustle and an air whoosh (~0.15 s).
fn jump(c: &mut Ctx) -> Vec<f32> {
    let sr = c.sr;
    let b = c.b;
    let mut out = c.silence(0.15);
    let k = whoosh(c, 0.15, 0.9, move |t| b * geo(500.0, 2000.0, t / 0.12), |t| hump(t, 0.035, 0.11));
    mix(&mut out, &k, 0, 0.9);
    let k = rustle(c, 0.13, 1200.0 * b, 6000.0, 120.0).env_fn(sr, |t| hump(t, 0.015, 0.11));
    mix(&mut out, &k, 0, 0.5);
    thunk(c, &mut out, 0.0, 0.2, 150.0, 90.0, 0.015);
    out
}

/// Landing: dull thud (low-passed noise + 70 Hz thump) with a cloth/gear rustle (~0.2 s).
fn land(c: &mut Ctx) -> Vec<f32> {
    let sr = c.sr;
    let (p, b) = (c.p, c.b);
    let mut out = c.silence(0.2);
    let k = thump(sr, 0.2, 100.0 * p, 55.0 * p, 0.018, 0.035).clip(1.4);
    mix(&mut out, &k, 0, 0.8);
    let k = c.white(0.2).lp(sr, 650.0 * b).env(sr, 0.0008, 0.035);
    mix(&mut out, &k, 0, 1.0);
    let k = rustle(c, 0.12, 1000.0 * b, 4500.0, 150.0).env(sr, 0.002, 0.045);
    mix(&mut out, &k, 0, 0.12);
    let k = c.white(0.04).bp(sr, 2000.0 * b, 0.9).env(sr, 0.0003, 0.012);
    mix(&mut out, &k, 0, 0.06);
    out
}

// ---------------------------------------------------------------------------
// 4.4 Pickups and chests
// ---------------------------------------------------------------------------

/// Weapon pickup: metallic clunk + a short bright rising two-note chime (~0.35 s).
fn pickup_weapon(c: &mut Ctx) -> Vec<f32> {
    let sr = c.sr;
    let p = c.p;
    let mut out = c.silence(0.36);
    mclick(c, &mut out, 0.0, 0.9, 1500.0, &[(1500.0, 0.012, 0.6), (2300.0, 0.008, 0.5), (3300.0, 0.005, 0.3), (850.0, 0.02, 0.3)]);
    thunk(c, &mut out, 0.0, 0.6, 220.0, 120.0, 0.02);
    bell(&mut out, sr, 0.07, 1318.5 * p, 0.45, 0.09, 0.7);
    bell(&mut out, sr, 0.15, 1975.5 * p, 0.6, 0.16, 0.8);
    sparkle(c, &mut out, 0.2, 0.33, 40.0, 3500.0, 8000.0, 0.12, 0.04);
    out
}

/// Ammo pickup: a rattle of small brass tinks (~0.25 s).
fn pickup_ammo(c: &mut Ctx) -> Vec<f32> {
    let mut out = c.silence(0.25);
    thunk(c, &mut out, 0.0, 0.35, 250.0, 150.0, 0.01);
    let mut t = 0.0;
    for i in 0..7 {
        let f = c.rng.range(2400.0, 4400.0);
        let g = c.rng.range(0.4, 1.0) * (1.0 - 0.08 * i as f32);
        mclick(c, &mut out, t, g, 3500.0, &[(f, 0.007, 0.6), (f * 1.51, 0.004, 0.3), (f * 0.62, 0.01, 0.3)]);
        t += c.rng.range(0.012, 0.03);
    }
    out
}

/// Heal pickup: bubbly rising "bloop" + sparkle (~0.3 s).
fn pickup_heal(c: &mut Ctx) -> Vec<f32> {
    let sr = c.sr;
    let p = c.p;
    let mut out = c.silence(0.3);
    chirp(&mut out, sr, 0.0, 0.14, 330.0 * p, 760.0 * p, 0.06, 0.7);
    chirp(&mut out, sr, 0.0, 0.14, 660.0 * p, 1520.0 * p, 0.04, 0.2);
    chirp(&mut out, sr, 0.09, 0.14, 520.0 * p, 1180.0 * p, 0.06, 0.5);
    chirp(&mut out, sr, 0.09, 0.14, 1040.0 * p, 2360.0 * p, 0.04, 0.15);
    for (i, f) in [2637.0f32, 3136.0, 4186.0].iter().enumerate() {
        bell(&mut out, sr, 0.13 + 0.04 * i as f32, f * p, 0.4, 0.06, 0.8);
    }
    sparkle(c, &mut out, 0.12, 0.28, 45.0, 3000.0, 7500.0, 0.12, 0.05);
    out
}

/// Chest opening: creaky latch clunk, then a magical rising glissando with shimmering high
/// partials and a warm swell (~1.3 s).
fn chest_open(c: &mut Ctx) -> Vec<f32> {
    let sr = c.sr;
    let p = c.p;
    let mut out = c.silence(1.3);
    // latch clunk + lock clicks
    thunk(c, &mut out, 0.0, 1.6, 180.0, 90.0, 0.04);
    mclick(c, &mut out, 0.0, 1.4, 1500.0, &[(1800.0, 0.012, 0.5), (2900.0, 0.008, 0.4), (1200.0, 0.02, 0.4)]);
    mclick(c, &mut out, 0.06, 0.8, 1800.0, &[(2300.0, 0.008, 0.5), (3400.0, 0.005, 0.3)]);
    // lid creak
    let k = creak(c, 0.34, 150.0, 260.0, 900.0, 0.1).env_fn(sr, |t| hump(t, 0.1, 0.24));
    mix_at(&mut out, sr, &k, 0.08, 0.7);
    // warm swell: C major on soft pads
    for (m, g) in [(60.0f32, 0.5f32), (64.0, 0.4), (67.0, 0.4), (72.0, 0.35)] {
        pad(&mut out, sr, 0.3, hz(m), 0.6, 0.45, g * 0.6, 0.45, 0.004);
    }
    // rising harp glissando (C major pentatonic), accelerating
    let notes = [72.0f32, 74.0, 76.0, 79.0, 81.0, 84.0, 86.0, 88.0, 91.0, 93.0];
    let mut t = 0.36;
    for (i, m) in notes.iter().enumerate() {
        pluck(&mut out, sr, t, hz(*m) * p, 0.25 + 0.025 * i as f32, 0.35);
        t += 0.065 - 0.004 * i as f32;
    }
    // final ding and shimmering high partials
    bell(&mut out, sr, 0.84, hz(96.0) * p, 0.45, 0.45, 0.9);
    bell(&mut out, sr, 0.86, hz(103.0) * p, 0.25, 0.3, 0.8);
    shimmer(&mut out, sr, 0.45, 0.8, &[hz(96.0) * p, hz(100.0) * p, hz(103.0) * p, hz(108.0) * p], 0.05, 0.3, 7.0);
    sparkle(c, &mut out, 0.4, 1.25, 28.0, 2500.0, 9000.0, 0.14, 0.12);
    let room = Room { rt60: 1.0, damp: 0.4, size: 1.0, wet: 0.25, tail: 0.0, pre: 0.0, hp: 300.0 };
    reverb(&out, sr, room)
}

// ---------------------------------------------------------------------------
// 4.5 Combat feedback
// ---------------------------------------------------------------------------

/// Hit marker: crisp short tick around 2 kHz (~0.07 s).
fn hit_marker(c: &mut Ctx) -> Vec<f32> {
    let sr = c.sr;
    let p = c.p;
    let mut out = c.silence(0.07);
    let k = tick_noise(c, 0.003, 2000.0, 0.0006);
    mix(&mut out, &k, 0, 0.5);
    modal(&mut out, sr, 0.0, &[(2200.0, 0.012, 0.8), (3300.0, 0.006, 0.35), (1500.0, 0.01, 0.2)], p, 1.0, 1.0);
    thunk(c, &mut out, 0.0, 0.25, 500.0, 300.0, 0.006);
    out
}

/// Headshot: higher double tick with a ring (~0.12 s).
fn hit_head(c: &mut Ctx) -> Vec<f32> {
    let mut out = c.silence(0.12);
    mclick(c, &mut out, 0.0, 0.8, 3000.0, &[(2700.0, 0.01, 0.8), (4000.0, 0.005, 0.35)]);
    mclick(c, &mut out, 0.034, 1.0, 3500.0, &[(3500.0, 0.012, 0.9), (5200.0, 0.006, 0.35), (4800.0, 0.05, 0.22)]);
    thunk(c, &mut out, 0.0, 0.25, 600.0, 350.0, 0.006);
    out
}

/// Shield hit: glassy blue "tink" with shimmer (~0.12 s).
fn hit_shield(c: &mut Ctx) -> Vec<f32> {
    let sr = c.sr;
    let p = c.p;
    let mut out = c.silence(0.12);
    let modes = [(3000.0, 0.05, 0.6), (3036.0, 0.05, 0.5), (4400.0, 0.06, 0.4), (4452.0, 0.06, 0.35), (5900.0, 0.07, 0.22), (7400.0, 0.045, 0.1)];
    modal(&mut out, sr, 0.0, &modes, p, 1.0, 1.0);
    let k = tick_noise(c, 0.004, 4000.0, 0.0005);
    mix(&mut out, &k, 0, 0.3);
    out.env_fn(sr, |t| 0.75 + 0.25 * sin_cyc(45.0 * t))
}

/// Player hurt: dull body thump, low-passed noise + 100 Hz, with a faint vocal "uh" (~0.25 s).
fn player_hurt(c: &mut Ctx) -> Vec<f32> {
    let sr = c.sr;
    let (p, b) = (c.p, c.b);
    let mut out = c.silence(0.25);
    let k = thump(sr, 0.25, 140.0 * p, 75.0 * p, 0.02, 0.045).clip(1.5);
    mix(&mut out, &k, 0, 0.9);
    let k = c.white(0.25).lp(sr, 700.0 * b).env(sr, 0.0008, 0.05);
    mix(&mut out, &k, 0, 0.8);
    let k = tick_noise(c, 0.01, 1500.0, 0.004);
    mix(&mut out, &k, 0, 0.25);
    // faint "uh": a falling saw through two formants
    let g = osc_buf(sr, 0.2, Wave::Saw, move |t| geo(160.0 * p, 95.0 * p, t / 0.2)).lp(sr, 3000.0);
    let mut f1 = g.clone().bp(sr, 600.0, 3.0);
    let f2 = g.bp(sr, 1100.0, 3.0);
    for (a, b2) in f1.iter_mut().zip(&f2) {
        *a += 0.5 * *b2;
    }
    let f1 = f1.env(sr, 0.01, 0.07);
    mix(&mut out, &f1, 0, 0.35);
    out
}

/// Elimination confirmed: a satisfying rising "ding" + soft whoosh (~0.5 s).
fn kill(c: &mut Ctx) -> Vec<f32> {
    let sr = c.sr;
    let p = c.p;
    let mut out = c.silence(0.5);
    let k = whoosh(c, 0.45, 1.5, |t| geo(600.0, 3500.0, t / 0.4), |t| hump(t, 0.15, 0.3));
    mix(&mut out, &k, 0, 0.35);
    thunk(c, &mut out, 0.0, 0.4, 300.0, 150.0, 0.012);
    bell(&mut out, sr, 0.0, 1046.5 * p, 0.7, 0.14, 0.7);
    bell(&mut out, sr, 0.085, 1568.0 * p, 0.9, 0.28, 0.9);
    bell(&mut out, sr, 0.13, 2637.0 * p, 0.25, 0.16, 0.6);
    out
}

// ---------------------------------------------------------------------------
// 4.6 Building and harvesting
// ---------------------------------------------------------------------------

/// Wood piece placed: chunky wooden thunk + a short creak (~0.3 s).
fn build_wood(c: &mut Ctx) -> Vec<f32> {
    let sr = c.sr;
    let (p, b) = (c.p, c.b);
    let mut out = c.silence(0.3);
    let modes = [(150.0, 0.05, 0.8), (260.0, 0.035, 0.6), (410.0, 0.025, 0.45), (700.0, 0.015, 0.3), (1100.0, 0.008, 0.2)];
    modal(&mut out, sr, 0.0, &modes, p, 1.0, 1.0);
    let k = thump(sr, 0.2, 100.0 * p, 60.0 * p, 0.02, 0.05).clip(1.4);
    mix(&mut out, &k, 0, 0.7);
    let k = c.white(0.1).lp(sr, 900.0 * b).env(sr, 0.0005, 0.02);
    mix(&mut out, &k, 0, 0.6);
    // the plank settles
    modal(&mut out, sr, 0.13, &modes, p * 1.03, 0.8, 0.35);
    let k = creak(c, 0.16, 300.0 * p, 420.0 * p, 1000.0, 0.08).env_fn(sr, |t| hump(t, 0.05, 0.11));
    mix_at(&mut out, sr, &k, 0.1, 0.14);
    out.clip(1.1)
}

/// Stone piece placed: heavy scrape-and-thud (~0.3 s).
fn build_stone(c: &mut Ctx) -> Vec<f32> {
    let sr = c.sr;
    let (p, b) = (c.p, c.b);
    let mut out = c.silence(0.3);
    // scrape
    let k = rustle(c, 0.14, 1200.0 * b, 4200.0 * b, 90.0).env_fn(sr, |t| hump(t, 0.09, 0.05));
    mix(&mut out, &k, 0, 0.45);
    crackle(c, &mut out, 0.0, 0.12, 200.0, 60.0, 1500.0, 5000.0, (0.001, 0.003), |_| 0.15);
    // thud
    let at = 0.11;
    let k = thump(sr, 0.2, 90.0 * p, 58.0 * p, 0.015, 0.045).clip(1.6);
    mix_at(&mut out, sr, &k, at, 0.9);
    let k = c.white(0.2).lp(sr, 500.0 * b).env(sr, 0.0008, 0.05);
    mix_at(&mut out, sr, &k, at, 0.7);
    modal(&mut out, sr, at, &[(900.0, 0.01, 0.4), (1600.0, 0.007, 0.35), (2400.0, 0.005, 0.25)], p, 1.0, 1.0);
    let k = tick_noise(c, 0.006, 1500.0, 0.001);
    mix_at(&mut out, sr, &k, at, 0.5);
    out.clip(1.1)
}

/// Metal piece placed: metallic clank with a ring (~0.35 s).
fn build_metal(c: &mut Ctx) -> Vec<f32> {
    let mut out = c.silence(0.35);
    let modes = [(520.0, 0.12, 0.6), (880.0, 0.1, 0.5), (1430.0, 0.08, 0.4), (2180.0, 0.05, 0.3), (3150.0, 0.03, 0.2), (4400.0, 0.015, 0.12)];
    mclick(c, &mut out, 0.0, 1.0, 2500.0, &modes);
    thunk(c, &mut out, 0.0, 0.6, 130.0, 90.0, 0.015);
    // slap: a second, slightly detuned hit right behind the first
    let modes2 = [(540.0, 0.1, 0.6), (915.0, 0.08, 0.5), (1490.0, 0.06, 0.4), (2260.0, 0.04, 0.3)];
    mclick(c, &mut out, 0.045, 0.5, 2500.0, &modes2);
    out.clip(1.1)
}

/// A building piece is destroyed: crumbling crash of wood cracks, falling debris and rumble
/// (~1.0 s).
fn piece_destroy(c: &mut Ctx) -> Vec<f32> {
    let sr = c.sr;
    let (p, b) = (c.p, c.b);
    let mut out = c.silence(1.0);
    // initial crack and splintering
    let k = c.white(0.05).bp(sr, 1800.0 * b, 0.6).env(sr, 0.0001, 0.007).clip(2.0);
    mix(&mut out, &k, 0, 0.9);
    modal(&mut out, sr, 0.0, &[(180.0, 0.06, 0.7), (310.0, 0.04, 0.5), (520.0, 0.025, 0.35)], p, 1.0, 1.0);
    // more wood cracks
    for &t in &[0.05f32, 0.13, 0.22, 0.31] {
        let f = c.rng.range(900.0, 2800.0) * b;
        let k = c.white(0.03).bp(sr, f, 0.8).env(sr, 0.0001, 0.004).clip(2.0);
        mix_at(&mut out, sr, &k, t, 0.6);
        let (f1, f2) = (c.rng.range(200.0, 500.0), c.rng.range(600.0, 1400.0));
        modal(&mut out, sr, t, &[(f1, 0.03, 0.5), (f2, 0.015, 0.3)], p, 1.0, 1.0);
    }
    // rumble, body and a low thump
    let k = c.brown(1.0).lp(sr, 160.0).hp(sr, 25.0).norm(1.0).env(sr, 0.02, 0.35).clip(1.4);
    mix(&mut out, &k, 0, 0.5);
    let k = c.pink(0.6).lp(sr, 900.0 * b).env(sr, 0.004, 0.2);
    mix(&mut out, &k, 0, 0.5);
    let k = thump(sr, 0.4, 90.0 * p, 55.0 * p, 0.03, 0.08).clip(1.5);
    mix(&mut out, &k, 0, 0.45);
    // falling debris and the final clatter
    debris(c, &mut out, 0.03, 0.9, 70.0, 8.0, 500.0, 3500.0, |t| 0.5 * decay(t, 0.45));
    crackle(c, &mut out, 0.45, 0.95, 50.0, 8.0, 700.0, 4500.0, (0.002, 0.01), |t| 0.3 * decay(t - 0.45, 0.25));
    out.clip(1.1)
}

/// Pickaxe hit: woody/stony crack with a little metallic clang (~0.25 s).
fn harvest_hit(c: &mut Ctx) -> Vec<f32> {
    let sr = c.sr;
    let (p, b) = (c.p, c.b);
    let mut out = c.silence(0.25);
    let k = c.white(0.03).bp(sr, 2000.0 * b, 0.8).env(sr, 0.0001, 0.005).clip(2.2);
    mix(&mut out, &k, 0, 1.1);
    modal(&mut out, sr, 0.0, &[(280.0, 0.03, 0.7), (520.0, 0.02, 0.5), (900.0, 0.012, 0.3)], p, 1.0, 0.6);
    modal(&mut out, sr, 0.0, &[(2600.0, 0.006, 0.3), (3900.0, 0.004, 0.2)], p, 1.0, 1.0);
    modal(&mut out, sr, 0.0, &[(1900.0, 0.02, 0.25), (3100.0, 0.012, 0.15)], p, 1.0, 1.0);
    let k = thump(sr, 0.2, 110.0 * p, 65.0 * p, 0.015, 0.03).clip(1.4);
    mix(&mut out, &k, 0, 0.3);
    out.clip(1.1)
}

/// Resource node breaks: bigger crack + falling chips (~0.5 s).
fn harvest_break(c: &mut Ctx) -> Vec<f32> {
    let sr = c.sr;
    let (p, b) = (c.p, c.b);
    let mut out = c.silence(0.5);
    let k = c.white(0.06).bp(sr, 1500.0 * b, 0.6).env(sr, 0.0001, 0.012).clip(2.2);
    mix(&mut out, &k, 0, 1.0);
    modal(&mut out, sr, 0.0, &[(190.0, 0.08, 0.8), (340.0, 0.05, 0.6), (620.0, 0.03, 0.4)], p, 1.0, 0.6);
    let k = thump(sr, 0.3, 100.0 * p, 58.0 * p, 0.02, 0.05).clip(1.5);
    mix(&mut out, &k, 0, 0.45);
    let k = c.white(0.3).lp(sr, 800.0 * b).env(sr, 0.0008, 0.06);
    mix(&mut out, &k, 0, 0.6);
    // falling chips
    for i in 0..9 {
        let t = c.rng.range(0.05, 0.42);
        let g = 0.5 * (1.0 - t / 0.5) * c.rng.range(0.5, 1.0);
        if i % 3 == 0 {
            let f = c.rng.range(300.0, 900.0);
            modal(&mut out, sr, t, &[(f, 0.015, 0.5), (f * 2.3, 0.008, 0.25)], p, 1.0, g);
        } else {
            let f = c.rng.range(1500.0, 4500.0);
            modal(&mut out, sr, t, &[(f, c.rng.range(0.004, 0.01), 0.5), (f * 1.6, 0.004, 0.25)], p, 1.0, g);
        }
    }
    let k = rustle(c, 0.3, 3000.0 * b, 9000.0, 110.0).env(sr, 0.01, 0.12);
    mix_at(&mut out, sr, &k, 0.05, 0.15);
    out.clip(1.1)
}

/// A tree falls: groaning creaks, snapping fibres, a rushing crown, then a heavy thud with a
/// leafy rustle (~1.2 s).
fn tree_fall(c: &mut Ctx) -> Vec<f32> {
    let sr = c.sr;
    let (p, b) = (c.p, c.b);
    let mut out = c.silence(1.2);
    // groaning creaks
    let k = creak(c, 0.7, 85.0 * p, 175.0 * p, 550.0, 0.12).env_fn(sr, |t| smoothstep(0.0, 0.3, t) * (1.0 - smoothstep(0.55, 0.7, t)));
    mix(&mut out, &k, 0, 0.6);
    let k = creak(c, 0.6, 140.0 * p, 260.0 * p, 1100.0, 0.1).env_fn(sr, |t| hump(t, 0.25, 0.3));
    mix_at(&mut out, sr, &k, 0.1, 0.25);
    // fibres snapping
    for &t in &[0.08f32, 0.2, 0.29, 0.41, 0.5, 0.58] {
        let f = c.rng.range(700.0, 2400.0);
        let k = c.white(0.03).bp(sr, f, 1.0).env(sr, 0.0001, 0.004).clip(2.0);
        mix_at(&mut out, sr, &k, t, 0.5);
        let (f1, f2) = (c.rng.range(300.0, 800.0), c.rng.range(1000.0, 2200.0));
        modal(&mut out, sr, t, &[(f1, 0.02, 0.5), (f2, 0.01, 0.3)], p, 1.0, 0.8);
    }
    // the crown rushes through the air
    let k = whoosh(c, 0.7, 0.8, move |t| b * geo(300.0, 1800.0, t / 0.7), |t| hump(t, 0.4, 0.3));
    mix_at(&mut out, sr, &k, 0.3, 0.55);
    let k = rustle(c, 0.7, 2500.0 * b, 9000.0, 90.0).env_fn(sr, |t| hump(t, 0.4, 0.3));
    mix_at(&mut out, sr, &k, 0.25, 0.4);
    // heavy ground impact
    let at = 0.9;
    let k = thump(sr, 0.3, 80.0 * p, 48.0 * p, 0.04, 0.1).clip(1.5);
    mix_at(&mut out, sr, &k, at, 1.0);
    let k = c.white(0.4).lp(sr, 350.0 * b).env(sr, 0.001, 0.1);
    mix_at(&mut out, sr, &k, at, 0.8);
    crackle(c, &mut out, at, 1.1, 150.0, 30.0, 800.0, 3500.0, (0.002, 0.008), |t| 0.35 * decay(t - at, 0.1));
    let k = rustle(c, 0.3, 2500.0 * b, 9000.0, 110.0).env(sr, 0.004, 0.1);
    mix_at(&mut out, sr, &k, at, 0.4);
    out.clip(1.1)
}

// ---------------------------------------------------------------------------
// 4.7 Healing
// ---------------------------------------------------------------------------

/// Bandage: cloth rip, wrapping rustle and a rising soft tone (~1.0 s).
fn heal_bandage(c: &mut Ctx) -> Vec<f32> {
    let sr = c.sr;
    let (p, b) = (c.p, c.b);
    let mut out = c.silence(1.0);
    // cloth rip: stick-slip gated noise with rising pitch
    let gate = smooth_noise(c, 0.25, 160.0);
    let mut k = c.white(0.25).hp(sr, 1200.0).sweep(sr, Kind::Bp, 0.9, move |t| b * geo(1500.0, 4200.0, t / 0.25));
    for (x, g) in k.iter_mut().zip(&gate) {
        let v = 0.5 + 0.5 * g;
        *x *= v * v * 2.0;
    }
    let k = k.env_fn(sr, |t| hump(t, 0.02, 0.22));
    mix(&mut out, &k, 0, 1.0);
    // wrapping swishes
    for (i, &t) in [0.32f32, 0.52, 0.72].iter().enumerate() {
        let k = rustle(c, 0.17, 1500.0 * b, 5500.0 * b, 70.0).env_fn(sr, |t| hump(t, 0.05, 0.12));
        mix_at(&mut out, sr, &k, t, 0.4 - 0.05 * i as f32);
    }
    // rising soft tone
    let k = osc_buf(sr, 0.6, Wave::Tri, move |t| p * geo(440.0, 880.0, t / 0.6)).env_fn(sr, |t| hump(t, 0.25, 0.35));
    mix_at(&mut out, sr, &k, 0.35, 0.2);
    let k = osc_buf(sr, 0.6, Wave::Sine, move |t| p * geo(880.0, 1760.0, t / 0.6)).env_fn(sr, |t| hump(t, 0.3, 0.3));
    mix_at(&mut out, sr, &k, 0.35, 0.08);
    bell(&mut out, sr, 0.86, 1760.0 * p, 0.22, 0.14, 0.7);
    out
}

/// Potion: glug-glug bubbling with a sparkly finish (~1.2 s).
fn heal_potion(c: &mut Ctx) -> Vec<f32> {
    let sr = c.sr;
    let (p, b) = (c.p, c.b);
    let mut out = c.silence(1.2);
    // glugs: bubbles rise in pitch as the bottle empties
    let times = [0.0f32, 0.16, 0.31, 0.45, 0.58, 0.69];
    for (i, &t) in times.iter().enumerate() {
        let f0 = (230.0 + 40.0 * i as f32) * p;
        chirp(&mut out, sr, t, 0.1, f0, f0 * 1.6, 0.05, 0.55);
        chirp(&mut out, sr, t, 0.1, f0 * 2.0, f0 * 3.2, 0.03, 0.15);
        let k = c.white(0.08).lp(sr, 900.0 * b).env(sr, 0.003, 0.03);
        mix_at(&mut out, sr, &k, t, 0.2);
    }
    // micro bubbles
    for _ in 0..14 {
        let t = c.rng.range(0.05, 0.75);
        let f0 = c.rng.range(1000.0, 2500.0) * p;
        chirp(&mut out, sr, t, 0.03, f0, f0 * 1.5, 0.01, c.rng.range(0.05, 0.14));
    }
    // sparkly finish: ascending arpeggio + shimmer
    for (i, m) in [88.0f32, 91.0, 93.0, 96.0, 100.0].iter().enumerate() {
        bell(&mut out, sr, 0.78 + 0.055 * i as f32, hz(*m) * p, 0.5, 0.2, 0.9);
    }
    let k = c.white(0.4).hp(sr, 6000.0).env_fn(sr, |t| hump(t, 0.2, 0.2));
    mix_at(&mut out, sr, &k, 0.78, 0.2);
    sparkle(c, &mut out, 0.8, 1.15, 40.0, 3000.0, 9000.0, 0.2, 0.08);
    let room = Room { rt60: 0.5, damp: 0.4, size: 0.8, wet: 0.15, tail: 0.0, pre: 0.0, hp: 400.0 };
    reverb(&out, sr, room)
}

/// Shield potion: electric charging hum rising in pitch, crystalline shimmer and a "zap"
/// lock-in (~1.2 s).
fn shield_use(c: &mut Ctx) -> Vec<f32> {
    let sr = c.sr;
    let (p, b) = (c.p, c.b);
    let mut out = c.silence(1.2);
    // charging hum: detuned saws, rising pitch, opening low-pass, growing tremolo
    let n = c.n(0.97);
    let mut o1 = Osc::new(0.0);
    let mut o2 = Osc::new(0.3);
    let mut bq = Biquad::new(Kind::Lp, sr, 400.0, 2.0, 0.0);
    let hum: Vec<f32> = (0..n)
        .map(|i| {
            let t = i as f32 / sr;
            let s = (t / 0.95).min(1.0);
            let f = p * geo(110.0, 440.0, s.powf(1.3));
            if i % SWEEP_STEP == 0 {
                bq.set(Kind::Lp, sr, geo(300.0, 3800.0, s) * b, 2.0, 0.0);
            }
            let y = bq.tick(0.5 * (o1.tick(Wave::Saw, f, sr) + o2.tick(Wave::Saw, f * cents(7.0), sr)));
            y * (1.0 - 0.3 * (0.5 + 0.5 * sin_cyc((25.0 + 30.0 * s) * t))) * smoothstep(0.0, 0.12, t)
        })
        .collect();
    mix(&mut out, &hum, 0, 0.3);
    // crystalline shimmer
    shimmer(&mut out, sr, 0.15, 0.82, &[2400.0 * p, 3100.0 * p, 4300.0 * p, 5700.0 * p, 7300.0 * p], 0.11, 0.6, 9.0);
    sparkle(c, &mut out, 0.3, 0.95, 35.0, 3000.0, 9000.0, 0.3, 0.05);
    // zap + lock-in
    let at = 0.95;
    let k = whoosh(c, 0.06, 1.0, |t| geo(7000.0, 1200.0, t / 0.05), |t| decay(t, 0.012)).clip(2.0);
    mix_at(&mut out, sr, &k, at, 0.7);
    let k = osc_buf(sr, 0.08, Wave::Square, |t| geo(1800.0, 300.0, t / 0.07)).env(sr, 0.0005, 0.02).lp(sr, 4000.0);
    mix_at(&mut out, sr, &k, at, 0.4);
    thunk(c, &mut out, at, 0.5, 120.0, 70.0, 0.04);
    bell(&mut out, sr, at + 0.01, 1568.0 * p, 0.45, 0.25, 0.8);
    bell(&mut out, sr, at + 0.01, 2349.0 * p, 0.3, 0.2, 0.7);
    out
}

// ---------------------------------------------------------------------------
// 4.8 Chest beacon, storm, bus and air
// ---------------------------------------------------------------------------

/// Smooth periodic control in `0..1` built from the first three harmonics of the loop
/// (`cyc` = loop position in cycles).  Used for gusts and swells.
fn periodic_gust(cyc: f32, ph: &[f32; 3]) -> f32 {
    let g = 0.5 + 0.30 * sin_cyc(cyc + ph[0]) + 0.17 * sin_cyc(2.0 * cyc + ph[1]) + 0.09 * sin_cyc(3.0 * cyc + ph[2]);
    g.clamp(0.0, 1.0)
}

/// Chest beacon: a soft shimmering magical hum with faint twinkles (loop, 2.0 s).  Every
/// partial and LFO has a whole number of cycles per loop, so the buffer is exactly periodic.
fn chest_hum(c: &mut Ctx) -> Vec<f32> {
    let sr = c.sr;
    let spec = LoopSpec::new(sr, 2.0, 0.1);
    let n = spec.n;
    let mut out = vec![0.0f32; n];
    // (frequency Hz, amplitude, tremolo cycles per loop, vibrato cycles per loop)
    let partials: [(f32, f32, f32, f32); 10] = [
        (220.0, 0.50, 1.0, 1.0),
        (222.0, 0.30, 2.0, 2.0),
        (330.0, 0.30, 3.0, 1.0),
        (333.0, 0.20, 2.0, 3.0),
        (440.0, 0.24, 1.0, 2.0),
        (444.0, 0.16, 3.0, 1.0),
        (660.0, 0.14, 2.0, 3.0),
        (880.0, 0.09, 3.0, 2.0),
        (1320.0, 0.05, 1.0, 1.0),
        (1762.0, 0.03, 2.0, 3.0),
    ];
    for (j, &(f, a, tk, vk)) in partials.iter().enumerate() {
        let k = spec.cycles(f);
        let (ph_start, ph0, ph1) = (c.rng.f(), c.rng.f(), c.rng.f());
        for (i, o) in out.iter_mut().enumerate() {
            let wob = 0.012 * spec.lfo_l(i, vk, ph1);
            let trem = 0.7 + 0.3 * spec.lfo_l(i, tk, ph0 + j as f32 * 0.13);
            *o += a * trem * sin_cyc(spec.ph_l(i, k) + ph_start + wob);
        }
    }
    // faint twinkles; their tails wrap around the loop point
    let mut t = 0.0f32;
    for _ in 0..6 {
        t += c.rng.range(0.15, 0.5);
        let mut tw = c.silence(0.7);
        let f = pentatonic(c, 72.0, 1500.0, 4200.0);
        bell(&mut tw, sr, 0.0, f, 0.09, 0.12, 0.5);
        mix_wrap(&mut out, &tw, ((t % 2.0) * sr) as usize, 1.0);
    }
    out
}

/// Storm ambience: ominous low rumble under an eerie detuned pad with a slow wobble
/// (loop, 4.0 s).
fn storm_ambient(c: &mut Ctx) -> Vec<f32> {
    let sr = c.sr;
    let spec = LoopSpec::new(sr, 4.0, 0.4);
    let secs = spec.total() as f32 / sr;
    let ph = [c.rng.f(), c.rng.f(), c.rng.f()];
    // noise layers (cross-fade loop): breathing rumble + a gusting air bed
    let rumble = c
        .brown(secs)
        .lp(sr, 140.0)
        .hp(sr, 20.0)
        .norm(1.0)
        .env_fn(sr, |t| 0.45 + 0.55 * periodic_gust(spec.cyc_t(t), &ph));
    let bed = c
        .pink(secs)
        .sweep(sr, Kind::Bp, 0.7, |t| 420.0 + 260.0 * sin_cyc(2.0 * spec.cyc_t(t) + ph[1]))
        .env_fn(sr, |t| 0.3 + 0.7 * periodic_gust(spec.cyc_t(t) + 0.37, &ph))
        .norm(1.0);
    let mut raw = spec.fit(rumble);
    for (r, b) in raw.iter_mut().zip(&bed) {
        *r += 0.22 * b;
    }
    let mut out = spec.fold(&raw);
    // eerie pad: a cluster of detuned, wobbling sines (periodic)
    let n = spec.n;
    let voices: [(f32, f32); 12] = [
        (82.5, 0.50),
        (83.0, 0.40),
        (87.25, 0.30),
        (123.5, 0.28),
        (130.75, 0.22),
        (164.75, 0.14),
        (196.0, 0.10),
        (261.5, 0.08),
        (329.5, 0.07),
        (349.25, 0.06),
        (523.25, 0.04),
        (659.5, 0.03),
    ];
    for (j, &(f, a)) in voices.iter().enumerate() {
        let k = spec.cycles(f);
        let wk = [1.0f32, 2.0, 3.0][j % 3];
        let (p_start, p0, p1) = (c.rng.f(), c.rng.f(), c.rng.f());
        for (i, o) in out.iter_mut().enumerate() {
            let wob = 0.02 * (1.0 + j as f32 * 0.3) * spec.lfo_l(i, wk, p0);
            let trem = 0.7 + 0.3 * spec.lfo_l(i, 1.0 + (j % 2) as f32, p1);
            *o += 0.18 * a * trem * sin_cyc(spec.ph_l(i, k) + p_start + wob);
        }
    }
    debug_assert_eq!(out.len(), n);
    out
}

/// Storm warning: a distant horn/siren with two swelling tones a tritone apart (~2.0 s).
fn storm_warning(c: &mut Ctx) -> Vec<f32> {
    let sr = c.sr;
    let p = c.p;
    let mut dry = c.silence(1.85);
    horn(&mut dry, sr, 0.0, hz(58.0) * p, 0.8, 0.3, 0.9);
    horn(&mut dry, sr, 0.85, hz(52.0) * p, 0.7, 0.3, 0.9);
    // breath of air that swells with the horns
    let air = c.white(1.85).bp(sr, 900.0, 1.5).env_fn(sr, |t| hump(t, 0.5, 1.2));
    mix(&mut dry, &air, 0, 0.05);
    let dry = dry.lp(sr, 2600.0);
    let room = Room { rt60: 1.6, damp: 0.5, size: 1.3, wet: 0.35, tail: 0.15, pre: 0.02, hp: 150.0 };
    reverb(&dry, sr, room)
}

/// Storm damage tick: a low electric pulse with a buzzy zap and a few sparks (~0.35 s).
fn storm_damage(c: &mut Ctx) -> Vec<f32> {
    let sr = c.sr;
    let p = c.p;
    let mut out = c.silence(0.35);
    let k = thump(sr, 0.3, 140.0 * p, 60.0 * p, 0.04, 0.09).clip(1.5);
    mix(&mut out, &k, 0, 0.9);
    // mains-like hum burst
    let k = osc_buf(sr, 0.3, Wave::Sine, move |_| 60.0 * p).env(sr, 0.003, 0.1);
    mix(&mut out, &k, 0, 0.3);
    let k = osc_buf(sr, 0.3, Wave::Sine, move |_| 120.0 * p).env(sr, 0.003, 0.07);
    mix(&mut out, &k, 0, 0.15);
    // buzzy zap: falling saw gated by a 55 Hz square
    let mut z = osc_buf(sr, 0.3, Wave::Saw, move |t| geo(220.0 * p, 80.0 * p, t / 0.3));
    let gate = osc_buf(sr, 0.3, Wave::Square, |_| 55.0);
    for (a, g) in z.iter_mut().zip(&gate) {
        *a *= 0.6 + 0.4 * g;
    }
    let z = z.lpq(sr, 1500.0, 1.5).env(sr, 0.002, 0.1);
    mix(&mut out, &z, 0, 0.5);
    crackle(c, &mut out, 0.0, 0.25, 180.0, 30.0, 1500.0, 6500.0, (0.0008, 0.003), |t| 0.45 * decay(t, 0.1));
    out.clip(1.1)
}

/// Battle bus: rumbling engine drone with a low chugging rhythm and propeller flutter
/// (loop, 2.0 s).
fn bus_loop(c: &mut Ctx) -> Vec<f32> {
    let sr = c.sr;
    let spec = LoopSpec::new(sr, 2.0, 0.15);
    let n = spec.n;
    let secs = spec.total() as f32 / sr;
    // chug: 6 chugs per second (12 per loop), fast attack, exponential decay
    let chug = |cyc: f32| -> f32 {
        let x = (cyc * 12.0).rem_euclid(1.0);
        smoothstep(0.0, 0.07, x) * (-x * 4.5).exp() * 1.35
    };
    // 1. periodic engine drone: stacked saws (with a slow beating partner), low-passed
    let mut drone = vec![0.0f32; spec.warm + n];
    for &(f, a) in &[(46.0f32, 1.0f32), (49.0, 0.35), (92.5, 0.5), (138.0, 0.3), (184.5, 0.18)] {
        let k = spec.cycles(f);
        for (i, d) in drone.iter_mut().enumerate() {
            *d += a * (2.0 * spec.ph(i, k) - 1.0);
        }
    }
    let mut drone = drone.lp(sr, 380.0);
    for (i, d) in drone.iter_mut().enumerate() {
        *d *= 0.62 + 0.38 * chug(spec.cyc(i));
    }
    let mut out = spec.take(&drone);
    // 2. noise layers (cross-fade loop): exhaust rumble + propeller flutter
    let ph0 = c.rng.f();
    let rumble = c
        .brown(secs)
        .lp(sr, 220.0)
        .hp(sr, 25.0)
        .norm(1.0)
        .env_fn(sr, |t| 0.4 + 0.6 * chug(spec.cyc_t(t)));
    let flutter = c
        .pink(secs)
        .sweep(sr, Kind::Bp, 1.3, |t| 650.0 + 120.0 * sin_cyc(spec.cyc_t(t) * 3.0 + ph0))
        .norm(1.0)
        .env_fn(sr, |t| 0.55 + 0.45 * sin_cyc(spec.cyc_t(t) * 56.0));
    let mut raw = spec.fit(rumble);
    for (r, f) in raw.iter_mut().zip(&flutter) {
        *r = *r * 0.9 + 0.5 * f;
    }
    let noise = spec.fold(&raw);
    // 3. propeller blade-pass tone (periodic)
    let (k1, k2) = (spec.cycles(74.0), spec.cycles(148.0));
    for i in 0..n {
        let g = 0.7 + 0.3 * spec.lfo_l(i, 3.0, ph0);
        let tone = 0.22 * sin_cyc(spec.ph_l(i, k1)) + 0.1 * sin_cyc(spec.ph_l(i, k2));
        out[i] += noise[i] * 0.55 + tone * g;
    }
    out
}

/// Wind: a rush of filtered noise with slow gusting and a faint whistle (loop, 3.0 s).
fn wind_loop(c: &mut Ctx) -> Vec<f32> {
    let sr = c.sr;
    let spec = LoopSpec::new(sr, 3.0, 0.25);
    let secs = spec.total() as f32 / sr;
    let ph = [c.rng.f(), c.rng.f(), c.rng.f()];
    let gust = |t: f32| periodic_gust(spec.cyc_t(t), &ph);
    // main rush: pink noise through a band-pass that follows the gusts
    let rush = c
        .pink(secs)
        .sweep(sr, Kind::Bp, 0.55, |t| 450.0 + 1100.0 * gust(t))
        .norm(1.0)
        .env_fn(sr, |t| 0.45 + 0.55 * gust(t).powf(1.3));
    // whistle through gaps: narrow band, only in the strongest gusts
    let whistle = c
        .white(secs)
        .sweep(sr, Kind::Bp, 14.0, |t| 1800.0 + 700.0 * gust(t))
        .norm(1.0)
        .env_fn(sr, |t| gust(t).powi(3));
    // low body
    let body = c.brown(secs).lp(sr, 160.0).hp(sr, 30.0).norm(1.0).env_fn(sr, |t| 0.3 + 0.7 * gust(t));
    let mut raw = spec.fit(rush);
    for ((r, w), b) in raw.iter_mut().zip(&whistle).zip(&body) {
        *r += 0.12 * w + 0.25 * b;
    }
    spec.fold(&raw)
}

/// Glider deploy: a sharp fabric "fwump" snap with an air burst (~0.6 s).
fn glider_deploy(c: &mut Ctx) -> Vec<f32> {
    let sr = c.sr;
    let (p, b) = (c.p, c.b);
    let mut out = c.silence(0.6);
    // fwump
    let k = thump(sr, 0.3, 110.0 * p, 55.0 * p, 0.03, 0.08).clip(1.5);
    mix(&mut out, &k, 0, 0.8);
    let k = c.white(0.3).lp(sr, 450.0 * b).env(sr, 0.002, 0.06);
    mix(&mut out, &k, 0, 0.8);
    // fabric snap
    let k = c.white(0.03).bp(sr, 1400.0 * b, 0.7).env(sr, 0.0001, 0.006).clip(2.0);
    mix(&mut out, &k, 0, 0.9);
    let k = tick_noise(c, 0.008, 1200.0, 0.002);
    mix(&mut out, &k, 0, 0.5);
    // air burst
    let k = whoosh(c, 0.55, 1.0, move |t| b * geo(2800.0, 700.0, t / 0.5), |t| smoothstep(0.0, 0.01, t) * decay(t, 0.15));
    mix(&mut out, &k, 0, 0.7);
    // canvas flapping
    let k = c
        .white(0.5)
        .bp(sr, 900.0, 1.5)
        .env_fn(sr, |t| (0.5 + 0.5 * sin_cyc(18.0 * t)) * decay(t, 0.18) * smoothstep(0.0, 0.05, t));
    mix_at(&mut out, sr, &k, 0.08, 0.4);
    out.clip(1.1)
}

/// Gliding: smooth air flow with a soft flutter (loop, 2.0 s).
fn glider_loop(c: &mut Ctx) -> Vec<f32> {
    let sr = c.sr;
    let spec = LoopSpec::new(sr, 2.0, 0.2);
    let secs = spec.total() as f32 / sr;
    let ph = [c.rng.f(), c.rng.f(), c.rng.f()];
    let flow = c
        .pink(secs)
        .sweep(sr, Kind::Bp, 0.6, |t| 1100.0 + 350.0 * sin_cyc(spec.cyc_t(t) + ph[0]))
        .norm(1.0)
        .env_fn(sr, |t| {
            let cy = spec.cyc_t(t);
            0.82 + 0.1 * sin_cyc(9.0 * cy + ph[1]) + 0.08 * sin_cyc(13.0 * cy + ph[2])
        });
    let hiss = c.white(secs).hp(sr, 4500.0).lp(sr, 9500.0).norm(1.0);
    let low = c.brown(secs).lp(sr, 220.0).hp(sr, 40.0).norm(1.0);
    let mut raw = spec.fit(flow);
    for ((r, h), l) in raw.iter_mut().zip(&hiss).zip(&low) {
        *r += 0.18 * h + 0.2 * l;
    }
    spec.fold(&raw)
}

// ---------------------------------------------------------------------------
// 4.9 UI and jingles
// ---------------------------------------------------------------------------

/// Clean soft click/pop (~0.08 s).
fn ui_click(c: &mut Ctx) -> Vec<f32> {
    let sr = c.sr;
    let p = c.p;
    let mut out = c.silence(0.08);
    chirp(&mut out, sr, 0.0, 0.05, 1100.0 * p, 760.0 * p, 0.014, 0.8);
    let k = c.white(0.01).lp(sr, 6000.0).env(sr, 0.0001, 0.0012);
    mix(&mut out, &k, 0, 0.3);
    thunk(c, &mut out, 0.0, 0.3, 450.0, 300.0, 0.008);
    out
}

/// Very quiet tick (~0.04 s).
fn ui_hover(c: &mut Ctx) -> Vec<f32> {
    let sr = c.sr;
    let p = c.p;
    let mut out = c.silence(0.04);
    chirp(&mut out, sr, 0.0, 0.02, 1900.0 * p, 1750.0 * p, 0.006, 1.0);
    let k = tick_noise(c, 0.004, 3000.0, 0.0004);
    mix(&mut out, &k, 0, 0.3);
    out
}

/// Lower pitched click for "back" / cancel (~0.1 s).
fn ui_back(c: &mut Ctx) -> Vec<f32> {
    let sr = c.sr;
    let p = c.p;
    let mut out = c.silence(0.1);
    chirp(&mut out, sr, 0.0, 0.07, 640.0 * p, 420.0 * p, 0.02, 0.8);
    thunk(c, &mut out, 0.0, 0.35, 300.0, 180.0, 0.01);
    let k = c.white(0.01).lp(sr, 4500.0).env(sr, 0.0001, 0.0012);
    mix(&mut out, &k, 0, 0.25);
    out
}

/// Victory fanfare: bright major-key brass arpeggio and answer phrase, a plagal-to-tonic
/// landing on a big sustained C major chord with bell cascade, timpani, cymbal and a long
/// reverb tail (~3.5 s).
fn victory(c: &mut Ctx) -> Vec<f32> {
    let sr = c.sr;
    let p = c.p;
    let f = |m: f32| hz(m) * p;
    let mut out = c.silence(3.2);
    // opening arpeggio G4 C5 E5 G5, then the answer E5 G5 C6: (midi, start, gate, release)
    let lead: [(f32, f32, f32, f32); 7] = [
        (67.0, 0.00, 0.12, 0.10),
        (72.0, 0.14, 0.12, 0.10),
        (76.0, 0.28, 0.12, 0.10),
        (79.0, 0.42, 0.30, 0.16),
        (76.0, 0.84, 0.11, 0.08),
        (79.0, 0.97, 0.11, 0.08),
        (84.0, 1.10, 0.36, 0.20),
    ];
    for &(m, t, g, r) in &lead {
        brass(&mut out, sr, t, f(m), g, r, 0.85, 1.35);
        bell(&mut out, sr, t, f(m + 12.0), 0.14, 0.2, 0.7);
    }
    // harmony: C major under the held G5, F major under the C6 (plagal), then the final C
    for &m in &[60.0f32, 64.0, 67.0] {
        brass(&mut out, sr, 0.42, f(m), 0.30, 0.2, 0.30, 0.8);
    }
    for &m in &[53.0f32, 57.0, 60.0, 65.0] {
        brass(&mut out, sr, 1.10, f(m), 0.36, 0.2, 0.30, 0.8);
    }
    let chord: [(f32, f32); 9] = [
        (36.0, 0.25),
        (48.0, 0.3),
        (55.0, 0.3),
        (60.0, 0.33),
        (64.0, 0.36),
        (67.0, 0.38),
        (72.0, 0.36),
        (76.0, 0.34),
        (79.0, 0.32),
    ];
    for (i, &(m, a)) in chord.iter().enumerate() {
        brass(&mut out, sr, 1.55 + 0.006 * i as f32, f(m), 0.9, 0.7, a, 1.2);
    }
    brass(&mut out, sr, 1.55, f(84.0), 0.9, 0.7, 0.8, 1.35);
    // bell cascade over the final chord
    for (i, &m) in [84.0f32, 88.0, 91.0, 96.0, 100.0, 103.0].iter().enumerate() {
        bell(&mut out, sr, 1.6 + 0.07 * i as f32, f(m), 0.3, 0.5, 1.0);
    }
    bell(&mut out, sr, 2.25, f(108.0), 0.18, 0.6, 0.8);
    // percussion: timpani, snare accents, riser and crash
    boom(c, &mut out, 0.0, 0.4, 140.0 * p, 75.0 * p, 0.2);
    snare(c, &mut out, 0.42, 0.5);
    snare(c, &mut out, 1.10, 0.5);
    cymbal(c, &mut out, 1.15, 0.25, 0.4, 0.02);
    boom(c, &mut out, 1.55, 0.55, 110.0 * p, 55.0 * p, 0.35);
    cymbal(c, &mut out, 1.55, 0.4, 0.005, 0.7);
    let room = Room { rt60: 1.7, damp: 0.35, size: 1.2, wet: 0.25, tail: 0.3, pre: 0.01, hp: 200.0 };
    reverb(&out, sr, room)
}

/// Defeat: a descending, sombre minor phrase (E D C A) with soft pads and a fading
/// A minor chord (~2.0 s).
fn defeat(c: &mut Ctx) -> Vec<f32> {
    let sr = c.sr;
    let p = c.p;
    let mut out = c.silence(1.85);
    for &(m, t, d) in &[(76.0f32, 0.0f32, 0.42f32), (74.0, 0.4, 0.42), (72.0, 0.8, 0.42)] {
        pad(&mut out, sr, t, hz(m) * p, d, 0.25, 0.9, 0.03, 0.006);
    }
    // the last note droops a little, like a sigh
    let last = osc_buf(sr, 0.55, Wave::Tri, move |t| hz(69.0) * p * (1.0 - 0.035 * smoothstep(0.25, 0.55, t)))
        .env_fn(sr, |t| adsr(t, 0.03, 0.2, 0.8, 0.3, 0.25));
    mix_at(&mut out, sr, &last, 1.2, 0.35);
    pad(&mut out, sr, 1.2, hz(69.0) * p, 0.3, 0.3, 0.5, 0.03, 0.006);
    // sad A minor chord underneath
    for &m in &[45.0f32, 52.0, 57.0, 60.0] {
        pad(&mut out, sr, 1.2, hz(m) * p, 0.3, 0.35, 0.55, 0.12, 0.003);
    }
    let dry = out.lp(sr, 2200.0);
    let room = Room { rt60: 1.4, damp: 0.5, size: 1.1, wet: 0.3, tail: 0.15, pre: 0.015, hp: 200.0 };
    reverb(&dry, sr, room)
}

/// Elimination sting: low hit, dissonant brass stab and rising shimmer (~0.7 s).
fn elimination(c: &mut Ctx) -> Vec<f32> {
    let sr = c.sr;
    let p = c.p;
    let mut out = c.silence(0.7);
    boom(c, &mut out, 0.0, 0.7, 120.0 * p, 50.0 * p, 0.13);
    let k = c.white(0.05).bp(sr, 2200.0, 0.6).env(sr, 0.0001, 0.008).clip(2.0);
    mix(&mut out, &k, 0, 0.7);
    for &m in &[57.0f32, 64.0, 69.0] {
        brass(&mut out, sr, 0.0, hz(m) * p, 0.12, 0.35, 0.5, 0.9);
    }
    // metallic minor-second bell hit
    bell(&mut out, sr, 0.0, hz(88.0) * p, 0.35, 0.3, 0.8);
    bell(&mut out, sr, 0.0, hz(89.0) * p, 0.3, 0.3, 0.8);
    // rising shimmer
    let k = whoosh(c, 0.6, 1.5, |t| geo(500.0, 6000.0, t / 0.55), |t| hump(t, 0.5, 0.1));
    mix_at(&mut out, sr, &k, 0.05, 0.35);
    for det in [1.0f32, cents(11.0)] {
        let k = osc_buf(sr, 0.5, Wave::Saw, move |t| p * det * geo(500.0, 2500.0, t / 0.5))
            .lp(sr, 3500.0)
            .env_fn(sr, |t| hump(t, 0.4, 0.1));
        mix_at(&mut out, sr, &k, 0.1, 0.12);
    }
    bell(&mut out, sr, 0.52, 1760.0 * p, 0.3, 0.15, 0.8);
    out.clip(1.1)
}

/// Leaving the bus: a whoosh with a rising airy sweep and a low swoop (~1.0 s).
fn drop_in(c: &mut Ctx) -> Vec<f32> {
    let sr = c.sr;
    let (p, b) = (c.p, c.b);
    let mut out = c.silence(1.0);
    // door pop
    thunk(c, &mut out, 0.0, 0.7, 150.0, 70.0, 0.04);
    let k = c.white(0.1).lp(sr, 500.0).env(sr, 0.001, 0.04);
    mix(&mut out, &k, 0, 0.5);
    // rising airy sweep
    let k = whoosh(c, 1.0, 1.1, move |t| b * geo(250.0, 3800.0, (t / 0.85).min(1.0).powf(1.2)), |t| smoothstep(0.0, 0.08, t) * hump(t, 0.5, 0.45));
    mix(&mut out, &k, 0, 1.3);
    let k = whoosh(c, 0.9, 0.7, move |t| b * geo(3000.0, 9000.0, t / 0.8), |t| hump(t, 0.6, 0.3));
    mix(&mut out, &k, 0, 0.3);
    // low swoop (downward, like passing air)
    let k = osc_buf(sr, 1.0, Wave::Saw, move |t| geo(240.0 * p, 60.0 * p, (t / 0.9).min(1.0)))
        .lp(sr, 600.0)
        .env_fn(sr, |t| hump(t, 0.2, 0.75));
    mix(&mut out, &k, 0, 0.3);
    out
}

// ============================================================================
// 5. Mastering
// ============================================================================

/// Per-sound loudness target (peak) and end fade (seconds).
fn master_spec(sfx: Sfx) -> (f32, f32) {
    match sfx {
        // weapons: loud
        Sfx::ShotAr | Sfx::ShotSmg | Sfx::ShotPistol => (0.9, 0.02),
        Sfx::ShotShotgun => (0.9, 0.08),
        Sfx::ShotSniper => (0.9, 0.2),
        Sfx::RocketFire => (0.85, 0.12),
        Sfx::Explosion => (0.9, 0.25),
        // handling
        Sfx::ReloadMag => (0.5, 0.05),
        Sfx::ReloadShell => (0.45, 0.04),
        Sfx::PumpAction => (0.6, 0.04),
        Sfx::EmptyClick => (0.45, 0.02),
        Sfx::WeaponSwap => (0.45, 0.02),
        // movement
        Sfx::StepGrass | Sfx::StepSand | Sfx::StepStone | Sfx::StepWood | Sfx::StepMetal | Sfx::StepWater => (0.35, 0.02),
        Sfx::Jump => (0.3, 0.03),
        Sfx::Land => (0.45, 0.03),
        // pickups
        Sfx::PickupWeapon => (0.6, 0.05),
        Sfx::PickupAmmo => (0.5, 0.03),
        Sfx::PickupHeal => (0.5, 0.05),
        Sfx::ChestOpen => (0.7, 0.2),
        Sfx::ChestHum => (0.35, 0.0),
        // combat feedback
        Sfx::HitMarker => (0.5, 0.01),
        Sfx::HitHead => (0.55, 0.02),
        Sfx::HitShield => (0.55, 0.03),
        Sfx::PlayerHurt => (0.7, 0.04),
        Sfx::Kill => (0.65, 0.08),
        // building
        Sfx::BuildWood | Sfx::BuildStone => (0.7, 0.04),
        Sfx::BuildMetal => (0.7, 0.06),
        Sfx::PieceDestroy => (0.8, 0.12),
        Sfx::HarvestHit => (0.7, 0.03),
        Sfx::HarvestBreak => (0.75, 0.06),
        Sfx::TreeFall => (0.75, 0.12),
        // healing
        Sfx::HealBandage => (0.5, 0.06),
        Sfx::HealPotion => (0.5, 0.1),
        Sfx::ShieldUse => (0.55, 0.06),
        // storm
        Sfx::StormAmbient => (0.4, 0.0),
        Sfx::StormWarning => (0.5, 0.25),
        Sfx::StormDamage => (0.5, 0.05),
        // air
        Sfx::BusLoop => (0.45, 0.0),
        Sfx::WindLoop => (0.4, 0.0),
        Sfx::GliderDeploy => (0.6, 0.08),
        Sfx::GliderLoop => (0.35, 0.0),
        // UI / jingles
        Sfx::UiClick | Sfx::UiBack => (0.4, 0.015),
        Sfx::UiHover => (0.2, 0.01),
        Sfx::Victory => (0.6, 0.3),
        Sfx::Defeat => (0.5, 0.25),
        Sfx::Elimination => (0.7, 0.1),
        Sfx::DropIn => (0.55, 0.15),
    }
}

/// Final stage: scrub non-finite samples, de-click (14 kHz roll-off, fade-in and fade-out for
/// one-shots; DC removal + seam polish for loops), peak-normalise to the sound's target loudness and
/// clamp to +-0.98.
fn master(sfx: Sfx, sr: f32, buf: &mut Vec<f32>) {
    for x in buf.iter_mut() {
        if !x.is_finite() {
            *x = 0.0;
        }
    }
    let (target, fade) = master_spec(sfx);
    let mut v = std::mem::take(buf);
    if sfx.is_loop() {
        polish_loop(&mut v);
    } else {
        // a gentle 14 kHz roll-off takes the edge off noise bursts without dulling sparkle
        v = v.lp1(sr, 14_000.0).fade_in(sr, 0.0003).fade_out(sr, fade.max(0.004));
    }
    v = v.norm(target);
    for x in v.iter_mut() {
        *x = x.clamp(-0.98, 0.98);
    }
    *buf = v;
}

// ============================================================================
// 6. Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    const RATES: [u32; 2] = [44_100, 48_000];

    fn peak_rms(x: &[f32]) -> (f32, f32) {
        let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        let rms = (x.iter().map(|v| (*v as f64) * (*v as f64)).sum::<f64>() / x.len().max(1) as f64).sqrt() as f32;
        (peak, rms)
    }

    /// In-place radix-2 FFT (test helper).
    fn fft(re: &mut [f64], im: &mut [f64]) {
        let n = re.len();
        let mut j = 0;
        for i in 1..n {
            let mut bit = n >> 1;
            while j & bit != 0 {
                j ^= bit;
                bit >>= 1;
            }
            j ^= bit;
            if i < j {
                re.swap(i, j);
                im.swap(i, j);
            }
        }
        let mut len = 2;
        while len <= n {
            let ang = -2.0 * std::f64::consts::PI / len as f64;
            for i in (0..n).step_by(len) {
                for k in 0..len / 2 {
                    let (wr, wi) = ((ang * k as f64).cos(), (ang * k as f64).sin());
                    let (ur, ui) = (re[i + k], im[i + k]);
                    let (xr, xi) = (re[i + k + len / 2], im[i + k + len / 2]);
                    let (vr, vi) = (xr * wr - xi * wi, xr * wi + xi * wr);
                    re[i + k] = ur + vr;
                    im[i + k] = ui + vi;
                    re[i + k + len / 2] = ur - vr;
                    im[i + k + len / 2] = ui - vi;
                }
            }
            len <<= 1;
        }
    }

    /// (energy-weighted spectral centroid in Hz, fraction of energy below `split_hz`) of the
    /// first 4096 samples.
    fn spectrum(x: &[f32], sr: f32, split_hz: f32) -> (f32, f32) {
        let n = 4096;
        let m = x.len().min(n);
        let mut re = vec![0.0f64; n];
        let mut im = vec![0.0f64; n];
        for i in 0..m {
            let w = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * (i as f64 + 0.5) / m as f64).cos();
            re[i] = x[i] as f64 * w;
        }
        fft(&mut re, &mut im);
        let (mut num, mut den, mut low) = (0.0f64, 0.0f64, 0.0f64);
        for k in 1..n / 2 {
            let f = k as f64 * sr as f64 / n as f64;
            let e = re[k] * re[k] + im[k] * im[k];
            num += f * e;
            den += e;
            if f < split_hz as f64 {
                low += e;
            }
        }
        ((num / den.max(1e-30)) as f32, (low / den.max(1e-30)) as f32)
    }

    /// Fraction of the total energy contained in the first `frac` of the buffer.
    fn early_energy(x: &[f32], frac: f32) -> f32 {
        let cut = ((x.len() as f32 * frac) as usize).clamp(1, x.len());
        let e = |s: &[f32]| s.iter().map(|v| (*v as f64) * (*v as f64)).sum::<f64>();
        (e(&x[..cut]) / e(x).max(1e-30)) as f32
    }

    // ------------------------------------------------------------------ tables

    #[test]
    fn enum_tables_are_consistent() {
        assert_eq!(Sfx::ALL.len(), 54);
        assert_eq!(Sfx::ALL.len(), Sfx::DropIn as usize + 1, "ALL must list every variant");
        for (i, s) in Sfx::ALL.iter().enumerate() {
            assert_eq!(*s as usize, i, "ALL out of order at {:?}", s);
        }
        // names: unique, non-empty snake_case
        let mut names: Vec<&str> = Sfx::ALL.iter().map(|s| s.name()).collect();
        for n in &names {
            assert!(!n.is_empty());
            assert!(n.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'), "bad name {}", n);
            assert!(!n.starts_with('_') && !n.ends_with('_') && !n.contains("__"), "bad name {}", n);
        }
        names.sort();
        names.dedup();
        assert_eq!(names.len(), Sfx::ALL.len(), "names must be unique");
        assert_eq!(Sfx::ShotAr.name(), "shot_ar");
        assert_eq!(Sfx::DropIn.name(), "drop_in");
        // loops
        let loops: Vec<Sfx> = Sfx::ALL.iter().copied().filter(|s| s.is_loop()).collect();
        assert_eq!(loops, vec![Sfx::ChestHum, Sfx::StormAmbient, Sfx::BusLoop, Sfx::WindLoop, Sfx::GliderLoop]);
        // variant counts
        for s in Sfx::ALL {
            let v = s.variants();
            assert!((1..=4).contains(&v), "{:?} has {} variants", s, v);
        }
        for s in [Sfx::StepGrass, Sfx::StepSand, Sfx::StepStone, Sfx::StepWood, Sfx::StepMetal, Sfx::StepWater] {
            assert!(s.variants() >= 3);
        }
        for s in [
            Sfx::Land,
            Sfx::ShotAr,
            Sfx::ShotSmg,
            Sfx::ShotPistol,
            Sfx::ShotShotgun,
            Sfx::HitMarker,
            Sfx::HarvestHit,
            Sfx::BuildWood,
            Sfx::BuildStone,
            Sfx::BuildMetal,
            Sfx::PlayerHurt,
        ] {
            assert!(s.variants() >= 3, "{:?}", s);
        }
        assert_eq!(Sfx::Victory.variants(), 1);
        assert_eq!(Sfx::ChestHum.variants(), 1);
    }

    // ------------------------------------------------------------- validity

    #[test]
    fn every_sound_is_valid() {
        for &sr in &RATES {
            for &sfx in Sfx::ALL {
                for v in 0..sfx.variants() {
                    let x = synth(sfx, sr, v);
                    let tag = format!("{:?} sr={} v={}", sfx, sr, v);
                    assert!(x.iter().all(|s| s.is_finite()), "{}: non-finite sample", tag);
                    let (peak, rms) = peak_rms(&x);
                    assert!(peak <= 0.98 + 1e-6, "{}: peak {}", tag, peak);
                    assert!(peak >= 0.15, "{}: too quiet, peak {}", tag, peak);
                    assert!(rms > 0.008, "{}: silent? rms {}", tag, rms);
                    let secs = x.len() as f32 / sr as f32;
                    assert!((0.03..=8.0).contains(&secs), "{}: length {} s", tag, secs);
                    // no DC offset to speak of
                    let mean = x.iter().map(|s| *s as f64).sum::<f64>() / x.len() as f64;
                    assert!(mean.abs() < 0.03 * peak as f64 + 1e-4, "{}: DC offset {}", tag, mean);
                    // non-loops start from ~0 and end on exactly ~0 (no clicks)
                    if !sfx.is_loop() {
                        assert!(x[0].abs() < 0.02, "{}: starts at {}", tag, x[0]);
                        assert!(x[x.len() - 1].abs() < 1e-4, "{}: ends at {}", tag, x[x.len() - 1]);
                        let tail = &x[x.len() - 8..];
                        assert!(tail.iter().all(|s| s.abs() < 0.03), "{}: loud last samples {:?}", tag, tail);
                    }
                }
            }
        }
    }

    #[test]
    fn loudness_targets_are_respected() {
        for &sfx in Sfx::ALL {
            let x = synth(sfx, 48_000, 0);
            let (peak, _) = peak_rms(&x);
            let (target, _) = master_spec(sfx);
            assert!((peak - target).abs() < 0.01, "{:?}: peak {} vs target {}", sfx, peak, target);
        }
        // coarse loudness classes from the design brief
        let peak = |s: Sfx| peak_rms(&synth(s, 48_000, 0)).0;
        assert!(peak(Sfx::ShotAr) >= 0.85 && peak(Sfx::Explosion) >= 0.85);
        assert!(peak(Sfx::StepGrass) <= 0.4 && peak(Sfx::StepMetal) <= 0.4);
        assert!(peak(Sfx::UiClick) <= 0.45 && peak(Sfx::UiHover) < peak(Sfx::UiClick));
        for l in [Sfx::ChestHum, Sfx::StormAmbient, Sfx::BusLoop, Sfx::WindLoop, Sfx::GliderLoop] {
            assert!((0.3..=0.5).contains(&peak(l)), "{:?}", l);
        }
        assert!((0.5..=0.7).contains(&peak(Sfx::Victory)));
    }

    #[test]
    fn other_sample_rates_and_variants_work() {
        for &sr in &[22_050u32, 32_000, 96_000] {
            for &sfx in Sfx::ALL {
                let x = synth(sfx, sr, 0);
                let (peak, rms) = peak_rms(&x);
                assert!(x.iter().all(|s| s.is_finite()), "{:?} sr={}", sfx, sr);
                assert!(peak <= 0.98 + 1e-6 && rms > 0.005, "{:?} sr={}: peak {} rms {}", sfx, sr, peak, rms);
                // duration is sample-rate independent
                let secs = x.len() as f32 / sr as f32;
                let ref_secs = synth(sfx, 48_000, 0).len() as f32 / 48_000.0;
                assert!((secs - ref_secs).abs() < 0.02, "{:?}: {} s at {} Hz vs {} s", sfx, secs, sr, ref_secs);
            }
        }
        // out-of-range variants are still valid and deterministic
        for &sfx in &[Sfx::StepGrass, Sfx::ShotAr, Sfx::HitMarker, Sfx::BuildMetal, Sfx::ChestHum] {
            let a = synth(sfx, 48_000, 1234);
            let b = synth(sfx, 48_000, 1234);
            assert_eq!(a, b);
            assert!(a.iter().all(|s| s.is_finite()) && peak_rms(&a).0 <= 0.98 + 1e-6);
        }
    }

    // ---------------------------------------------------------------- loops

    /// (|last - first|, |slope after seam - slope before seam|, rms of neighbour differences,
    /// 99th percentile of neighbour differences)
    fn seam_metrics(x: &[f32]) -> (f32, f32, f32, f32) {
        let n = x.len();
        let jump = (x[0] - x[n - 1]).abs();
        let slope_jump = ((x[1] - x[0]) - (x[n - 1] - x[n - 2])).abs();
        let mut d: Vec<f32> = x.windows(2).map(|w| (w[1] - w[0]).abs()).collect();
        let rms = (d.iter().map(|v| (*v as f64) * (*v as f64)).sum::<f64>() / d.len() as f64).sqrt() as f32;
        d.sort_by(|a, b| a.partial_cmp(b).unwrap());
        (jump, slope_jump, rms, d[d.len() * 99 / 100])
    }

    #[test]
    fn loops_are_seamless() {
        for &sr in &RATES {
            for &sfx in Sfx::ALL.iter().filter(|s| s.is_loop()) {
                let x = synth(sfx, sr, 0);
                let (jump, slope_jump, rms, p99) = seam_metrics(&x);
                let tag = format!("{:?} sr={}", sfx, sr);
                // |last - first| is tiny, in absolute terms and compared to normal sample steps
                assert!(jump < 0.002, "{}: |last-first| = {}", tag, jump);
                assert!(jump <= 0.25 * rms + 1e-4, "{}: jump {} vs rms step {}", tag, jump, rms);
                assert!(jump <= p99, "{}: seam step {} > p99 step {}", tag, jump, p99);
                // the slope across the seam matches the slope inside the loop
                assert!(slope_jump <= rms + 1e-4, "{}: slope jump {} vs rms step {}", tag, slope_jump, rms);
                // looping twice: the junction is indistinguishable from any other sample pair
                let mut twice = x.clone();
                twice.extend_from_slice(&x);
                let junction = (twice[x.len()] - twice[x.len() - 1]).abs();
                assert!(junction <= p99, "{}: junction {}", tag, junction);
                // steady level: no segment is wildly louder than another
                let seg = x.len() / 8;
                let rmss: Vec<f32> = (0..8).map(|k| peak_rms(&x[k * seg..(k + 1) * seg]).1).collect();
                let (lo, hi) = rmss.iter().fold((f32::MAX, 0.0f32), |(l, h), r| (l.min(*r), h.max(*r)));
                assert!(lo / hi > 0.3, "{}: unsteady loop, segment rms {:?}", tag, rmss);
            }
        }
    }

    #[test]
    fn loop_machinery() {
        // cross-fade fold keeps the seam continuous and the level steady for uncorrelated noise
        let sr = 48_000.0;
        let spec = LoopSpec::new(sr, 1.0, 0.1);
        let mut rng = Rng::new(7);
        let raw: Vec<f32> = (0..spec.total()).map(|_| rng.bi()).collect();
        let out = spec.fold(&raw);
        assert_eq!(out.len(), spec.n);
        assert_eq!(out[0], raw[spec.warm + spec.n] * cos_cyc(0.5 / spec.xf as f32 * 0.25) + raw[spec.warm] * sin_cyc(0.5 / spec.xf as f32 * 0.25));
        let (_, r_mid) = peak_rms(&out[spec.xf * 2..spec.n - 1]);
        let (_, r_fade) = peak_rms(&out[..spec.xf]);
        assert!((r_fade / r_mid - 1.0).abs() < 0.08, "cross-fade level {} vs {}", r_fade, r_mid);
        // snap() gives whole cycles per loop
        let f = spec.snap(123.4);
        assert!(((f * spec.period()).round() - f * spec.period()).abs() < 1e-3);
        // a rotated loop contains the same samples
        let mut a: Vec<f32> = (0..1000).map(|i| (i as f32 * 0.1).sin() + 0.3).collect();
        let mut sorted_before = a.clone();
        polish_loop(&mut a);
        let mean_before: f32 = sorted_before.iter().sum::<f32>() / 1000.0;
        for v in sorted_before.iter_mut() {
            *v -= mean_before;
        }
        let mut sorted_after = a.clone();
        sorted_before.sort_by(|x, y| x.partial_cmp(y).unwrap());
        sorted_after.sort_by(|x, y| x.partial_cmp(y).unwrap());
        for (x, y) in sorted_before.iter().zip(&sorted_after) {
            assert!((x - y).abs() < 1e-4);
        }
    }

    // ---------------------------------------------------------- determinism

    #[test]
    fn synthesis_is_deterministic() {
        for &sfx in Sfx::ALL {
            for v in 0..sfx.variants() {
                let a = synth(sfx, 48_000, v);
                let b = synth(sfx, 48_000, v);
                assert_eq!(a, b, "{:?} v{} differs between runs", sfx, v);
            }
        }
    }

    #[test]
    fn variants_differ() {
        for &sfx in Sfx::ALL.iter().filter(|s| s.variants() > 1) {
            let base = synth(sfx, 48_000, 0);
            for v in 1..sfx.variants() {
                let other = synth(sfx, 48_000, v);
                let same = base.len() == other.len() && base.iter().zip(&other).all(|(a, b)| a == b);
                assert!(!same, "{:?} variant {} is identical to variant 0", sfx, v);
                // ...but still the same kind of sound: similar length and loudness class
                let ratio = other.len() as f32 / base.len() as f32;
                assert!((0.9..1.1).contains(&ratio), "{:?} v{}: length ratio {}", sfx, v, ratio);
            }
        }
    }

    #[test]
    fn synthesizing_everything_is_fast_enough() {
        let t0 = std::time::Instant::now();
        let mut samples = 0usize;
        for &sfx in Sfx::ALL {
            for v in 0..sfx.variants() {
                samples += synth(sfx, 48_000, v).len();
            }
        }
        let secs = t0.elapsed().as_secs_f32();
        assert!(samples > 48_000 * 30, "expected a good amount of audio, got {} samples", samples);
        // ~0.3 s natively in release; very loose bound so only real regressions trip it
        assert!(secs < 6.0, "synthesizing all sounds took {} s", secs);
    }

    // ------------------------------------------------------- spectral sanity

    #[test]
    fn sounds_have_the_intended_character() {
        let sr = 48_000.0;
        let get = |s: Sfx| synth(s, 48_000, 0);
        // gunshots put most of their energy early
        for s in [Sfx::ShotAr, Sfx::ShotSmg, Sfx::ShotPistol] {
            let e = early_energy(&get(s), 0.35);
            assert!(e > 0.8, "{:?}: only {} of the energy in the first 35 %", s, e);
        }
        assert!(early_energy(&get(Sfx::Explosion), 0.5) > 0.7);
        // big booms are low, ticks and tinks are bright
        let (c_expl, low_expl) = spectrum(&get(Sfx::Explosion), sr, 300.0);
        let (c_marker, low_marker) = spectrum(&get(Sfx::HitMarker), sr, 300.0);
        let (c_head, _) = spectrum(&get(Sfx::HitHead), sr, 300.0);
        let (c_shield, _) = spectrum(&get(Sfx::HitShield), sr, 300.0);
        assert!(low_expl > 0.4, "explosion low-frequency share {}", low_expl);
        assert!(c_expl < 1500.0, "explosion centroid {}", c_expl);
        assert!(c_marker > 1500.0 && low_marker < 0.1, "hit marker centroid {} low {}", c_marker, low_marker);
        assert!(c_head > c_marker && c_shield > 2500.0, "head {} marker {} shield {}", c_head, c_marker, c_shield);
        let (c_ar, _) = spectrum(&get(Sfx::ShotAr), sr, 300.0);
        let (c_shotgun, low_shotgun) = spectrum(&get(Sfx::ShotShotgun), sr, 300.0);
        assert!(c_shotgun < c_ar, "shotgun ({}) should be darker than the AR ({})", c_shotgun, c_ar);
        assert!(low_shotgun > 0.3);
        // step materials: wood is low and hollow, metal rings higher
        let (c_wood, _) = spectrum(&get(Sfx::StepWood), sr, 300.0);
        let (c_metal, _) = spectrum(&get(Sfx::StepMetal), sr, 300.0);
        assert!(c_wood < 600.0 && c_metal > 800.0 && c_metal > c_wood, "wood {} metal {}", c_wood, c_metal);
        // sounds from the brief keep their lengths
        let secs = |s: Sfx| get(s).len() as f32 / sr;
        assert!((secs(Sfx::ShotSniper) - 1.4).abs() < 0.05);
        assert!((secs(Sfx::Explosion) - 2.0).abs() < 0.05);
        assert!((secs(Sfx::Victory) - 3.5).abs() < 0.1);
        assert!((secs(Sfx::StormAmbient) - 4.0).abs() < 0.01);
        assert!((secs(Sfx::WindLoop) - 3.0).abs() < 0.01);
        assert!((secs(Sfx::ChestHum) - 2.0).abs() < 0.01);
        assert!(secs(Sfx::UiHover) < 0.06 && secs(Sfx::HitMarker) < 0.1);
    }

    // ------------------------------------------------------------- toolkit

    #[test]
    fn toolkit_primitives() {
        // polynomial sine
        let mut worst = 0.0f32;
        for i in -2000..2000 {
            let p = i as f32 * 0.0137;
            worst = worst.max((sin_cyc(p) - (p * TAU).sin()).abs());
        }
        assert!(worst < 2e-5, "sin_cyc error {}", worst);
        assert!((cos_cyc(0.0) - 1.0).abs() < 1e-5);
        // soft clip: bounded, odd, monotonic, C1 at the knee
        let mut last = -2.0;
        for i in -400..=400 {
            let x = i as f32 * 0.02;
            let y = soft(x);
            assert!(y.abs() <= 1.0 && y >= last - 1e-6);
            assert!((soft(-x) + y).abs() < 1e-6);
            last = y;
        }
        assert_eq!(soft(3.0), 1.0);
        assert!(((soft(2.999) - 1.0) / 0.001).abs() < 0.01, "slope at the knee");
        // rng: deterministic, roughly uniform
        let (mut a, mut b) = (Rng::new(5), Rng::new(5));
        let mut sum = 0.0;
        for _ in 0..10_000 {
            let x = a.f();
            assert_eq!(x, b.f());
            assert!((0.0..1.0).contains(&x));
            sum += x;
        }
        assert!((sum / 10_000.0 - 0.5).abs() < 0.02);
        assert_ne!(Rng::new(1).next_u32(), Rng::new(2).next_u32());
        // geometric interpolation and midi
        assert!((geo(100.0, 400.0, 0.5) - 200.0).abs() < 1e-3);
        assert!((hz(69.0) - 440.0).abs() < 1e-3 && (hz(81.0) - 880.0).abs() < 1e-2);
    }

    #[test]
    fn filters_behave() {
        let sr = 48_000.0;
        let tone = |f: f32| -> Vec<f32> { (0..9600).map(|i| sin_cyc(f * i as f32 / sr)).collect() };
        let level = |x: &[f32]| peak_rms(&x[4800..]).1;
        // low-pass: passes 200 Hz, kills 8 kHz
        let lo = tone(200.0).lp(sr, 1000.0);
        let hi = tone(8000.0).lp(sr, 1000.0);
        assert!(level(&lo) > 0.65 && level(&hi) < 0.02, "lp {} {}", level(&lo), level(&hi));
        // high-pass: the opposite
        assert!(level(&tone(200.0).hp(sr, 1000.0)) < 0.05 && level(&tone(8000.0).hp(sr, 1000.0)) > 0.65);
        // one-pole sections: -3 dB at the corner, ~0 dB / strongly attenuated on either side
        let l1 = level(&tone(1000.0).lp1(sr, 1000.0));
        assert!((l1 - 0.5).abs() < 0.08, "lp1 corner level {}", l1);
        assert!(level(&tone(100.0).lp1(sr, 1000.0)) > 0.65 && level(&tone(10_000.0).lp1(sr, 1000.0)) < 0.12);
        assert!(level(&tone(50.0).hp1(sr, 1000.0)) < 0.06 && level(&tone(10_000.0).hp1(sr, 1000.0)) > 0.65);
        // band-pass has ~unity gain at the centre
        let c = level(&tone(2000.0).bp(sr, 2000.0, 4.0));
        assert!((c - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.03, "bp centre gain {}", c);
        // notch removes its centre
        assert!(level(&tone(1000.0).notch(sr, 1000.0, 4.0)) < 0.05);
        // peaking EQ +12 dB boosts the centre by ~4x
        let g = level(&tone(1000.0).peak(sr, 1000.0, 1.0, 12.0)) / level(&tone(1000.0));
        assert!((g - 3.98).abs() < 0.2, "peak gain {}", g);
        // shelves
        assert!(level(&tone(100.0).low_shelf(sr, 500.0, 12.0)) > 3.0 * level(&tone(100.0)));
        assert!(level(&tone(10_000.0).high_shelf(sr, 4000.0, 12.0)) > 3.0 * level(&tone(10_000.0)));
        // stable at very low cut-offs
        let mut rng = Rng::new(3);
        let noise: Vec<f32> = (0..48_000).map(|_| rng.bi()).collect();
        let y = noise.lp(sr, 30.0);
        assert!(y.iter().all(|v| v.is_finite() && v.abs() < 1.0));
        // sweeping filters stay finite and bounded
        let z = (0..48_000).map(|_| rng.bi()).collect::<Vec<_>>().sweep(sr, Kind::Bp, 8.0, |t| 200.0 + 6000.0 * t);
        assert!(z.iter().all(|v| v.is_finite() && v.abs() < 20.0));
    }

    #[test]
    fn resonators_envelopes_and_reverb() {
        let sr = 48_000.0;
        // damped sine: decays by 1/e after tau
        let mut buf = vec![0.0f32; 48_000];
        ring(&mut buf, sr, 0, 1000.0, 0.02, 1.0);
        let env = |from: usize| buf[from..from + 48].iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!((env(960) / env(0) - (-1.0f32).exp()).abs() < 0.08, "ring decay");
        assert!(buf[30_000..].iter().all(|v| *v == 0.0), "ring must stop at ~-100 dB");
        // modes above 0.45 sr are skipped
        let mut b2 = vec![0.0f32; 100];
        ring(&mut b2, sr, 0, 30_000.0, 0.01, 1.0);
        assert!(b2.iter().all(|v| *v == 0.0));
        // exponential envelope with linear attack
        let e = vec![1.0f32; 4800].env(sr, 0.01, 0.05);
        assert!(e[0] == 0.0 && e[240] < 0.6 && e[480] > 0.8);
        assert!((e[2400] - (-0.05f32 / 0.05).exp()).abs() < 0.01);
        // fades end exactly at zero
        let f = vec![1.0f32; 1000].fade_out(sr, 0.005);
        assert_eq!(f[999], 0.0);
        assert!(f[999 - 300] == 1.0 && f[999 - 100] < 1.0);
        // adsr reaches zero exactly at the end of the release
        assert!(adsr(0.0, 0.01, 0.1, 0.7, 0.5, 0.2) == 0.0);
        assert!((adsr(0.3, 0.01, 0.1, 0.7, 0.5, 0.2) - 0.7).abs() < 0.05);
        assert!(adsr(0.7, 0.01, 0.1, 0.7, 0.5, 0.2) < 1e-6 && adsr(0.8, 0.01, 0.1, 0.7, 0.5, 0.2) == 0.0);
        // reverb: finite, adds a decaying tail of the requested length
        let mut imp = vec![0.0f32; 4800];
        imp[0] = 1.0;
        let room = Room { rt60: 1.0, damp: 0.4, size: 1.0, wet: 1.0, tail: 1.5, pre: 0.0, hp: 0.0 };
        let r = reverb(&imp, sr, room);
        assert_eq!(r.len(), 4800 + 72_000);
        assert!(r.iter().all(|v| v.is_finite()));
        let early = peak_rms(&r[2000..12_000]).1;
        let late = peak_rms(&r[60_000..72_000]).1;
        assert!(late < early * 0.3 && late > 0.0, "reverb should decay: {} -> {}", early, late);
        // mix() truncates instead of growing and fades layers that end by themselves
        let mut d = vec![0.0f32; 10];
        mix(&mut d, &[1.0; 20], 5, 1.0);
        assert_eq!(d.len(), 10);
        assert_eq!(d[5], 1.0);
        let mut d = vec![0.0f32; 1000];
        mix(&mut d, &[1.0; 400], 100, 1.0);
        assert!(d[499] < 0.01 && d[300] == 1.0);
    }
}
