//! Every gameplay number, plus the pure steering / attack / projectile maths.
//!
//! The game systems and the headless balance simulation (`sim.rs`, run with
//! `cargo test --release balance_report -- --nocapture`) both use this module,
//! so tuning here is what the sim measures.
#![cfg_attr(not(test), allow(dead_code))]

use bevy::math::Vec3;

// ---------------------------------------------------------------- arena

pub const ARENA_RADIUS: f32 = 55.0;
pub const GRAVITY: f32 = 22.0;

// ---------------------------------------------------------------- player

pub const PLAYER_H: f32 = 1.7;
pub const PLAYER_W: f32 = 1.7;
pub const PLAYER_RADIUS: f32 = 0.45;
pub const START_HP: i32 = 3;
pub const WALK_SPEED: f32 = 6.5;
pub const SPRINT_MULT: f32 = 1.65;
pub const JUMP_VEL: f32 = 7.8;
pub const INVULN: f32 = 1.0;
/// Horizontal knockback speed when hit.
pub const KNOCKBACK: f32 = 9.0;

pub fn sprint_speed() -> f32 {
    WALK_SPEED * SPRINT_MULT
}

/// Point-vs-player test against the upright card volume.
pub fn hits_player(player_pos: Vec3, point: Vec3, radius: f32) -> bool {
    let top = player_pos.y + PLAYER_H - 0.1;
    let bottom = player_pos.y + 0.1;
    let y = point.y.clamp(bottom, top);
    let closest = Vec3::new(player_pos.x, y, player_pos.z);
    closest.distance(point) < radius + PLAYER_RADIUS
}

// ---------------------------------------------------------------- gun

pub const MAG_SIZE: u32 = 36;
pub const START_RESERVE: u32 = 108;
pub const MAX_RESERVE: u32 = 216;
pub const BURST: u32 = 3;
pub const BURST_GAP: f32 = 0.065;
pub const BURST_COOLDOWN: f32 = 0.36;
pub const RELOAD_TIME: f32 = 1.9;
pub const ROUND_DAMAGE: f32 = 1.0;
pub const GUN_RANGE: f32 = 150.0;
pub const SPREAD: f32 = 0.0045;

/// Seconds per burst cycle (first round to first round of the next burst).
pub fn burst_cycle(rate_mult: f32) -> f32 {
    ((BURST - 1) as f32 * BURST_GAP + BURST_COOLDOWN) / rate_mult
}

/// Rounds per second while the trigger is held, ignoring reloads.
pub fn trigger_rps(rate_mult: f32) -> f32 {
    BURST as f32 / burst_cycle(rate_mult)
}

/// Rounds per second averaged over magazine + reload.
pub fn sustained_rps(rate_mult: f32) -> f32 {
    let mag_time = MAG_SIZE as f32 / trigger_rps(rate_mult);
    MAG_SIZE as f32 / (mag_time + RELOAD_TIME)
}

// ---------------------------------------------------------------- enemies

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Kind {
    SproutMole,
    ForestBunny,
    StrayDog,
    GatorGuy,
    BunBunny,
    SnowPile,
    SharkPlane,
    Ufo,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ProjKind {
    /// slow straight shot
    Linear,
    /// arcing snowball
    Lob,
    /// ball rolling along the floor
    Roll,
    /// arcing dynamite that explodes on landing
    Bomb,
    /// tracking orb; can be shot down
    Homing,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Attack {
    /// contact damage only
    Melee,
    /// leaps at the player from `trigger` metres
    Pounce { trigger: f32 },
    /// fires a projectile from up to `max_dist`
    Shoot { proj: ProjKind, max_dist: f32 },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Movement {
    Chase,
    /// hold a preferred distance, strafing inside the band
    KeepRange(i32),
    /// circle the player at a radius
    Orbit(i32),
}

pub struct KindInfo {
    pub key: &'static str,
    /// sprite width / height
    pub aspect: f32,
    pub height: f32,
    pub hp: f32,
    pub speed: f32,
    pub hover: f32,
    /// seconds into a run before this kind can spawn
    pub unlock: f32,
    pub weight: f32,
    pub movement: Movement,
    pub attack: Attack,
    /// seconds between attacks (min, max)
    pub cooldown: (f32, f32),
}

impl KindInfo {
    pub fn radius(&self) -> f32 {
        self.height * self.aspect * 0.36
    }
}

impl Kind {
    pub const ALL: [Kind; 8] = [
        Kind::SproutMole,
        Kind::ForestBunny,
        Kind::StrayDog,
        Kind::GatorGuy,
        Kind::BunBunny,
        Kind::SnowPile,
        Kind::SharkPlane,
        Kind::Ufo,
    ];

    pub fn info(self) -> &'static KindInfo {
        use Attack::*;
        use Movement::*;
        const T: [KindInfo; 8] = [
            KindInfo {
                key: "sprout_mole",
                aspect: 140.0 / 218.0,
                height: 1.7,
                hp: 6.0,
                speed: 2.6,
                hover: 0.0,
                unlock: 0.0,
                weight: 3.0,
                movement: Chase,
                attack: Melee,
                cooldown: (1.0, 1.0),
            },
            KindInfo {
                key: "forest_bunny",
                aspect: 133.0 / 149.0,
                height: 1.25,
                hp: 3.0,
                speed: 6.4,
                hover: 0.0,
                unlock: 0.0,
                weight: 2.5,
                movement: Chase,
                attack: Melee,
                cooldown: (1.0, 1.0),
            },
            KindInfo {
                key: "stray_dog",
                aspect: 239.0 / 303.0,
                height: 1.9,
                hp: 6.0,
                speed: 3.3,
                hover: 0.0,
                unlock: 20.0,
                weight: 2.0,
                movement: Chase,
                attack: Pounce { trigger: 7.5 },
                cooldown: (2.2, 2.2),
            },
            KindInfo {
                key: "gator_guy",
                aspect: 300.0 / 413.0,
                height: 2.6,
                hp: 9.0,
                speed: 2.3,
                hover: 0.0,
                unlock: 40.0,
                weight: 1.5,
                movement: KeepRange(13),
                attack: Shoot { proj: ProjKind::Linear, max_dist: 28.0 },
                cooldown: (2.6, 3.4),
            },
            KindInfo {
                key: "bun_bunny",
                aspect: 169.0 / 125.0,
                height: 1.1,
                hp: 6.0,
                speed: 2.7,
                hover: 0.0,
                unlock: 60.0,
                weight: 1.5,
                movement: KeepRange(11),
                attack: Shoot { proj: ProjKind::Roll, max_dist: 24.0 },
                cooldown: (2.8, 3.6),
            },
            KindInfo {
                key: "snow_pile",
                aspect: 332.0 / 159.0,
                height: 1.2,
                hp: 10.0,
                speed: 1.6,
                hover: 0.0,
                unlock: 80.0,
                weight: 1.3,
                movement: KeepRange(14),
                attack: Shoot { proj: ProjKind::Lob, max_dist: 26.0 },
                cooldown: (2.6, 3.4),
            },
            KindInfo {
                key: "shark_plane",
                aspect: 447.0 / 206.0,
                height: 1.25,
                hp: 8.0,
                speed: 4.4,
                hover: 3.2,
                unlock: 120.0,
                weight: 1.0,
                movement: Orbit(15),
                attack: Shoot { proj: ProjKind::Bomb, max_dist: 28.0 },
                cooldown: (3.4, 4.2),
            },
            KindInfo {
                key: "ufo",
                aspect: 90.0 / 118.0,
                height: 1.1,
                hp: 5.0,
                speed: 3.6,
                hover: 2.4,
                unlock: 150.0,
                weight: 0.9,
                movement: KeepRange(16),
                attack: Shoot { proj: ProjKind::Homing, max_dist: 30.0 },
                cooldown: (4.0, 5.0),
            },
        ];
        &T[self as usize]
    }
}

/// Horizontal velocity an enemy wants this frame.
/// `dir` points at the player (unit, XZ), `side` is the current strafe direction.
pub fn steer(kind: Kind, speed: f32, dist: f32, dir: Vec3, side: Vec3) -> Vec3 {
    match kind.info().movement {
        Movement::Chase => dir * speed,
        Movement::KeepRange(want) => {
            let want = want as f32;
            if dist > want + 2.5 {
                dir * speed
            } else if dist < want - 3.0 {
                -dir * speed * 0.8
            } else {
                side * speed * 0.6
            }
        }
        Movement::Orbit(r) => {
            let radial = (dist - r as f32).clamp(-1.0, 1.0);
            (side + dir * radial).normalize_or_zero() * speed
        }
    }
}

/// How far an enemy bounces back after touching the player (one awhbs height).
pub const MELEE_RECOIL_DIST: f32 = PLAYER_H;
const MELEE_RECOIL_UP: f32 = 4.0;

/// Hop velocity that carries an enemy `MELEE_RECOIL_DIST` away from the player
/// (`away` is a unit XZ direction), so one touch can't chain into another.
pub fn melee_recoil(away: Vec3) -> Vec3 {
    let air_time = 2.0 * MELEE_RECOIL_UP / GRAVITY;
    away * (MELEE_RECOIL_DIST / air_time) + Vec3::Y * MELEE_RECOIL_UP
}

pub fn pounce_velocity(dir: Vec3, dist: f32) -> Vec3 {
    dir * (dist / 0.6).clamp(9.0, 16.0) + Vec3::Y * 6.5
}

/// Where and how fast an enemy's projectile leaves. `None` for non-shooters.
pub fn launch(kind: Kind, enemy_pos: Vec3, player_pos: Vec3, player_vel: Vec3, chest: Vec3) -> Option<(ProjKind, Vec3, Vec3)> {
    let info = kind.info();
    let Attack::Shoot { proj, .. } = info.attack else { return None };
    let to = player_pos - enemy_pos;
    let flat_to = Vec3::new(to.x, 0.0, to.z);
    let dist = flat_to.length().max(0.01);
    let dir = flat_to / dist;
    let pvel = Vec3::new(player_vel.x, 0.0, player_vel.z);
    Some(match proj {
        ProjKind::Linear => {
            let from = enemy_pos + Vec3::Y * info.height * 0.62 + dir * 0.4;
            (proj, from, (chest - from).normalize_or(dir) * LINEAR_SPEED)
        }
        ProjKind::Roll => {
            let from = Vec3::new(enemy_pos.x, 0.0, enemy_pos.z) + dir * 0.9 + Vec3::Y * 0.4;
            (proj, from, dir * ROLL_SPEED)
        }
        ProjKind::Lob => {
            let from = enemy_pos + Vec3::Y * info.height * 0.9;
            let flight = 1.0 + dist / 25.0;
            let target = player_pos + pvel * flight * 0.5 + Vec3::Y * 0.4;
            (proj, from, lob_velocity(from, target, flight))
        }
        ProjKind::Bomb => {
            let from = enemy_pos + Vec3::Y * info.height * 0.3;
            let flight = 1.2 + dist / 22.0;
            let target = player_pos + pvel * flight * 0.4;
            (proj, from, lob_velocity(from, target, flight))
        }
        ProjKind::Homing => {
            let from = enemy_pos + Vec3::Y * info.height * 0.5;
            (proj, from, (chest - from).normalize_or(dir) * HOMING_SPEED)
        }
    })
}

// ---------------------------------------------------------------- director

/// Enemies spawned per second at run time `t`.
///
/// Fitted so incoming enemy hp/s (rate x average hp x hp_mult) is ~1.4 at the
/// start, matches a good player's sustained dps (~3) around 3:00 and doubles it
/// by 6:00; see the sim report.
pub fn spawn_rate(t: f32) -> f32 {
    (0.3 + t / 1500.0).min(1.5)
}

pub fn alive_cap(t: f32) -> usize {
    (22.0 + t / 4.0).min(90.0) as usize
}

pub fn hp_mult(t: f32) -> f32 {
    1.0 + t / 600.0
}

pub fn speed_mult(t: f32) -> f32 {
    (1.0 + t / 600.0).min(1.35)
}

/// Spawn budget at the start of a run (1.0 = one enemy immediately).
pub const START_BUDGET: f32 = 0.0;
/// Enemies left this far behind are moved back onto the spawn ring, so kiting
/// can't fill the alive cap with harmless stragglers.
pub const RELOCATE_DIST: f32 = 50.0;
pub const SPAWN_DIST: (f32, f32) = (24.0, 36.0);

// ---------------------------------------------------------------- projectiles

pub const LOB_GRAVITY: f32 = 14.0;
pub const LINEAR_SPEED: f32 = 7.5;
pub const ROLL_SPEED: f32 = 9.5;
pub const HOMING_SPEED: f32 = 5.5;
pub const HOMING_TURN: f32 = 1.9;
pub const EXPLOSION_RADIUS: f32 = 3.2;

/// (radius, lifetime)
pub fn proj_stats(kind: ProjKind) -> (f32, f32) {
    match kind {
        ProjKind::Linear => (0.22, 6.0),
        ProjKind::Lob => (0.3, 6.0),
        ProjKind::Roll => (0.4, 5.0),
        ProjKind::Bomb => (0.3, 8.0),
        ProjKind::Homing => (0.32, 7.0),
    }
}

/// Launch velocity that lands on `to` after `flight` seconds.
pub fn lob_velocity(from: Vec3, to: Vec3, flight: f32) -> Vec3 {
    (to - from) / flight + Vec3::Y * 0.5 * LOB_GRAVITY * flight
}

/// Advance a projectile's velocity one step (gravity / homing). Position is integrated by the caller.
pub fn step_projectile(kind: ProjKind, pos: Vec3, vel: Vec3, chest: Vec3, dt: f32) -> Vec3 {
    match kind {
        ProjKind::Homing => {
            let want = (chest - pos).normalize_or(vel.normalize_or(Vec3::X));
            let cur = vel.normalize_or(want);
            let angle = cur.angle_between(want);
            let step = (HOMING_TURN * dt).min(angle);
            let axis = cur.cross(want).normalize_or(Vec3::Y);
            (bevy::math::Quat::from_axis_angle(axis, step) * cur) * HOMING_SPEED
        }
        ProjKind::Lob | ProjKind::Bomb => vel - Vec3::Y * LOB_GRAVITY * dt,
        ProjKind::Linear | ProjKind::Roll => vel,
    }
}

// ---------------------------------------------------------------- pickups

pub const AMMO_AMOUNT: u32 = 36;
pub const AMMO_DROP_CHANCE: f32 = 0.25;
pub const PICKUP_LIFE: f32 = 30.0;
pub const PICKUP_RADIUS: f32 = 1.3;
pub const AMMO_INTERVAL: (f32, f32) = (4.0, 6.0);
pub const AMMO_MAX_ON_FIELD: usize = 8;
pub const HEART_INTERVAL: (f32, f32) = (45.0, 70.0);
pub const POWERUP_INTERVAL: (f32, f32) = (25.0, 40.0);
pub const BUFF_DURATION: (f32, f32) = (15.0, 30.0);
pub const SPEED_BUFF: f32 = 1.4;
pub const FIRE_RATE_BUFF: f32 = 1.6;

/// Random pickup location at least 8 m from the player; `a`, `b` uniform in 0..1.
pub fn pickup_spot(player: Vec3, a: f32, b: f32) -> Option<Vec3> {
    let angle = a * std::f32::consts::TAU;
    let r = ARENA_RADIUS * 0.75 * b.sqrt();
    let p = Vec3::new(angle.cos() * r, 0.0, angle.sin() * r);
    (p.distance(Vec3::new(player.x, 0.0, player.z)) > 8.0).then_some(p)
}

/// Weighted choice among kinds unlocked at `t`; `roll` is uniform in 0..1.
pub fn pick_kind(t: f32, roll: f32) -> Kind {
    let unlocked = Kind::ALL.into_iter().filter(|k| k.info().unlock <= t);
    let total: f32 = unlocked.clone().map(|k| k.info().weight).sum();
    let mut r = roll * total;
    let mut last = Kind::SproutMole;
    for k in unlocked {
        last = k;
        r -= k.info().weight;
        if r <= 0.0 {
            return k;
        }
    }
    last
}

/// Spawn point `dist` from the player at `angle`, flipped / clamped to stay in the arena.
pub fn spawn_position(player: Vec3, angle: f32, dist: f32) -> Vec3 {
    let p = Vec3::new(player.x, 0.0, player.z);
    let off = Vec3::new(angle.cos(), 0.0, angle.sin()) * dist;
    let mut pos = p + off;
    if pos.length() > ARENA_RADIUS - 2.0 {
        pos = (p - off).clamp_length_max(ARENA_RADIUS - 2.0);
    }
    pos
}

/// Enemies placed at the very start of a run: (kind, distance).
pub const OPENING_WAVE: &[(Kind, f32)] = &[
    (Kind::SproutMole, 15.0),
    (Kind::SproutMole, 17.0),
    (Kind::SproutMole, 19.0),
    (Kind::ForestBunny, 24.0),
];

// ---------------------------------------------------------------- powerups & one-shot items

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Powerup {
    Speed,
    FireRate,
    /// permanent +1 heart
    MaxHealth,
    /// one-shot: explosion around the player
    Blast,
    /// one-shot: teleport to the aimed spot
    Blink,
    /// one-shot: temporary shield covering every heart
    Shield,
}

impl Powerup {
    pub const ALL: [Powerup; 6] =
        [Powerup::Speed, Powerup::FireRate, Powerup::MaxHealth, Powerup::Blast, Powerup::Blink, Powerup::Shield];

    pub fn weight(self) -> f32 {
        match self {
            Powerup::Speed | Powerup::FireRate => 2.0,
            Powerup::MaxHealth => 1.0,
            Powerup::Blast | Powerup::Blink => 2.0,
            Powerup::Shield => 1.5,
        }
    }

    /// Held in the 1/2/3 slots rather than applied on pickup.
    pub fn is_item(self) -> bool {
        matches!(self, Powerup::Blast | Powerup::Blink | Powerup::Shield)
    }

    pub fn pick(roll: f32) -> Powerup {
        let total: f32 = Self::ALL.iter().map(|p| p.weight()).sum();
        let mut r = roll * total;
        for p in Self::ALL {
            r -= p.weight();
            if r <= 0.0 {
                return p;
            }
        }
        Powerup::Shield
    }
}

pub const POWERUP_MAX_ON_FIELD: usize = 2;
pub const BLAST_RADIUS: f32 = 8.0;
/// Scaled by `hp_mult` so it stays a clear-the-room button late in a run.
pub const BLAST_DAMAGE: f32 = 12.0;
pub const BLAST_KNOCK: f32 = 15.0;
pub const BLINK_RANGE: f32 = 14.0;
pub const BLINK_INVULN: f32 = 0.35;
