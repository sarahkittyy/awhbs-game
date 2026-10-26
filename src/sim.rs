//! Headless balance simulation. Runs the real steering / attack / projectile /
//! director / pickup rules from `balance.rs` against a scripted player bot at
//! several aim accuracies and reports survival times and where damage comes from.
//!
//! cargo test --release balance_report -- --nocapture

use bevy::math::Vec3;
use fastrand::Rng;

use crate::balance::*;

const DT: f32 = 1.0 / 60.0;
const MAX_TIME: f32 = 1200.0;

fn flat(v: Vec3) -> Vec3 {
    Vec3::new(v.x, 0.0, v.z)
}

fn range(rng: &mut Rng, a: f32, b: f32) -> f32 {
    a + (b - a) * rng.f32()
}

struct Enemy {
    kind: Kind,
    pos: Vec3,
    vel: Vec3,
    hp: f32,
    speed: f32,
    cooldown: f32,
    airborne: bool,
    strafe: f32,
    phase: f32,
}

struct Proj {
    id: u32,
    kind: ProjKind,
    pos: Vec3,
    vel: Vec3,
    life: f32,
    radius: f32,
}

#[derive(Clone, Copy, PartialEq)]
enum Pick {
    Ammo,
    Heart,
    Power(Powerup),
}

struct Pickup {
    kind: Pick,
    pos: Vec3,
    life: f32,
}

/// Where a hit came from.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Source {
    Contact(Kind),
    Proj(ProjKind),
}

#[derive(Default, Clone)]
pub struct RunResult {
    pub time: f32,
    pub kills: u32,
    pub fired: u32,
    pub hits: u32,
    pub dry_time: f32,
    pub flee_time: f32,
    pub damage: Vec<(Source, f32)>,
    pub first_contact: f32,
    /// first time any enemy got within 10 m
    pub first_engage: f32,
    pub items_used: u32,
    pub shield_saves: u32,
}

pub struct Bot {
    /// probability a round hits the intended target at close range
    pub accuracy: f32,
    /// seconds of reaction delay before switching targets
    pub reaction: f32,
    /// chance per incoming projectile that the bot notices and sidesteps it
    pub dodge: f32,
}

struct World {
    rng: Rng,
    t: f32,
    budget: f32,
    enemies: Vec<Enemy>,
    projs: Vec<Proj>,
    pickups: Vec<Pickup>,
    ammo_timer: f32,
    heart_timer: f32,
    powerup_timer: f32,
    // player
    pos: Vec3,
    vel: Vec3,
    hp: i32,
    max_hp: i32,
    invuln: f32,
    mag: u32,
    reserve: u32,
    reload: f32,
    burst_left: u32,
    burst_timer: f32,
    gun_cd: f32,
    speed_buff: f32,
    rate_buff: f32,
    pierce_buff: f32,
    fleeing: bool,
    inventory: Vec<Powerup>,
    shield_left: f32,
    shield_points: i32,
    dodge_skill: f32,
    strafe: f32,
    strafe_timer: f32,
    target_timer: f32,
    target: Option<usize>,
    /// sidestep direction while a projectile we noticed is incoming
    dodge_dir: Vec3,
    dodge_timer: f32,
    noticed: Vec<u32>,
    next_proj_id: u32,
    res: RunResult,
}

impl World {
    fn new(seed: u64) -> Self {
        let mut rng = Rng::with_seed(seed);
        let heart_timer = range(&mut rng, HEART_INTERVAL.0, HEART_INTERVAL.1);
        let powerup_timer = range(&mut rng, POWERUP_INTERVAL.0 * 0.9, POWERUP_INTERVAL.1 * 0.9);
        let mut w = Self {
            rng,
            t: 0.0,
            budget: START_BUDGET,
            enemies: vec![],
            projs: vec![],
            pickups: vec![],
            ammo_timer: 4.0,
            heart_timer,
            powerup_timer,
            pos: Vec3::new(0.0, 0.0, -9.6),
            vel: Vec3::ZERO,
            hp: START_HP,
            max_hp: START_HP,
            invuln: 0.0,
            mag: MAG_SIZE,
            reserve: START_RESERVE,
            reload: 0.0,
            burst_left: 0,
            burst_timer: 0.0,
            gun_cd: 0.0,
            speed_buff: 0.0,
            rate_buff: 0.0,
            pierce_buff: 0.0,
            fleeing: false,
            inventory: vec![],
            shield_left: 0.0,
            shield_points: 0,
            dodge_skill: 0.0,
            strafe: 1.0,
            strafe_timer: 2.0,
            target_timer: 0.0,
            target: None,
            dodge_dir: Vec3::ZERO,
            dodge_timer: 0.0,
            noticed: vec![],
            next_proj_id: 0,
            res: RunResult {
                first_contact: f32::NAN,
                first_engage: f32::NAN,
                ..default_result()
            },
        };
        for (kind, d) in OPENING_WAVE {
            let a = w.rng.f32() * std::f32::consts::TAU;
            let pos = spawn_position(w.pos, a, *d);
            w.spawn(*kind, pos);
        }
        w
    }

    fn spawn(&mut self, kind: Kind, pos: Vec3) {
        let info = kind.info();
        let speed = info.speed * speed_mult(self.t) * range(&mut self.rng, 0.9, 1.1);
        let cooldown = range(&mut self.rng, 1.0, 2.5);
        let strafe = if self.rng.bool() { 1.0 } else { -1.0 };
        let phase = self.rng.f32() * 6.28;
        self.enemies.push(Enemy {
            kind,
            pos: pos + Vec3::Y * info.hover,
            vel: Vec3::ZERO,
            hp: info.hp * hp_mult(self.t),
            speed,
            cooldown,
            airborne: false,
            strafe,
            phase,
        });
    }

    fn hurt(&mut self, from: Vec3, src: Source) {
        if self.invuln > 0.0 || self.hp <= 0 {
            return;
        }
        self.invuln = INVULN;
        let away = flat(self.pos - from).normalize_or(Vec3::Z);
        self.vel += away * KNOCKBACK;
        if self.shield_left > 0.0 && self.shield_points > 0 {
            self.shield_points -= 1;
            self.res.shield_saves += 1;
            return;
        }
        self.hp -= 1;
        let t = self.t;
        self.res.damage.push((src, t));
    }

    fn rate_mult(&self) -> f32 {
        if self.rate_buff > 0.0 { FIRE_RATE_BUFF } else { 1.0 }
    }

    fn step(&mut self, bot: &Bot) {
        let dt = DT;
        self.t += dt;
        let t = self.t;

        // --- director
        self.budget += spawn_rate(t) * dt;
        while self.budget >= 1.0 {
            self.budget -= 1.0;
            if self.enemies.len() >= alive_cap(t) {
                continue;
            }
            let kind = pick_kind(t, self.rng.f32());
            let d = range(&mut self.rng, SPAWN_DIST.0, SPAWN_DIST.1);
            let a = self.rng.f32() * std::f32::consts::TAU;
            let pos = spawn_position(self.pos, a, d);
            self.spawn(kind, pos);
        }

        // --- pickups
        self.ammo_timer -= dt;
        if self.ammo_timer <= 0.0 {
            self.ammo_timer = range(&mut self.rng, AMMO_INTERVAL.0, AMMO_INTERVAL.1);
            if self.pickups.iter().filter(|p| p.kind == Pick::Ammo).count() < AMMO_MAX_ON_FIELD {
                self.place_pickup(Pick::Ammo);
            }
        }
        self.heart_timer -= dt;
        if self.heart_timer <= 0.0 {
            self.heart_timer = range(&mut self.rng, HEART_INTERVAL.0, HEART_INTERVAL.1);
            if !self.pickups.iter().any(|p| p.kind == Pick::Heart) {
                self.place_pickup(Pick::Heart);
            }
        }
        self.powerup_timer -= dt;
        if self.powerup_timer <= 0.0 {
            self.powerup_timer = range(&mut self.rng, POWERUP_INTERVAL.0, POWERUP_INTERVAL.1);
            let n = self.pickups.iter().filter(|p| matches!(p.kind, Pick::Power(_))).count();
            if n < POWERUP_MAX_ON_FIELD {
                let k = Pick::Power(Powerup::pick(self.rng.f32()));
                self.place_pickup(k);
            }
        }
        self.speed_buff = (self.speed_buff - dt).max(0.0);
        self.rate_buff = (self.rate_buff - dt).max(0.0);
        self.pierce_buff = (self.pierce_buff - dt).max(0.0);
        self.shield_left = (self.shield_left - dt).max(0.0);
        self.use_items();

        self.bot_move(bot);
        self.bot_shoot(bot);
        self.enemies_step();
        self.projectiles_step();
        self.collect();
        self.invuln -= dt;
        if self.mag == 0 && self.reserve == 0 && self.reload <= 0.0 {
            self.res.dry_time += dt;
        }
        if self.fleeing {
            self.res.flee_time += dt;
        }
    }

    /// Bot item use: blast when crowded, shield when hurt, blink out when cornered.
    fn use_items(&mut self) {
        let near = |w: &World, r: f32| w.enemies.iter().filter(|e| flat(e.pos - w.pos).length() < r).count();
        if self.inventory.contains(&Powerup::Blast) && (near(self, BLAST_RADIUS * 0.8) >= 4 || near(self, 2.5) >= 1) {
            self.inventory.retain(|p| *p != Powerup::Blast);
            self.res.items_used += 1;
            let damage = BLAST_DAMAGE * hp_mult(self.t);
            let me = self.pos;
            for e in &mut self.enemies {
                let off = flat(e.pos - me);
                let d = off.length();
                if d < BLAST_RADIUS {
                    e.hp -= damage;
                    e.vel = off.normalize_or(Vec3::X) * BLAST_KNOCK * (1.0 - 0.5 * d / BLAST_RADIUS)
                        + Vec3::Y * BLAST_KNOCK * 0.45;
                    e.airborne = true;
                }
            }
            let before = self.enemies.len();
            self.enemies.retain(|e| e.hp > 0.0);
            self.res.kills += (before - self.enemies.len()) as u32;
            self.target = None;
            self.projs.retain(|p| p.pos.distance(me) >= BLAST_RADIUS);
        }
        if self.inventory.contains(&Powerup::Shield) && self.hp < self.max_hp {
            self.inventory.retain(|p| *p != Powerup::Shield);
            self.res.items_used += 1;
            self.shield_left = range(&mut self.rng, BUFF_DURATION.0, BUFF_DURATION.1);
            self.shield_points = self.max_hp;
        }
        if self.inventory.contains(&Powerup::Blink) && self.fleeing && near(self, 2.5) >= 1 {
            self.inventory.retain(|p| *p != Powerup::Blink);
            self.res.items_used += 1;
            let mut away = Vec3::ZERO;
            for e in &self.enemies {
                let d = flat(self.pos - e.pos);
                away += d.normalize_or_zero() / d.length().max(0.5);
            }
            let center = -flat(self.pos).normalize_or_zero() * 0.5;
            let dir = (away.normalize_or(Vec3::X) + center).normalize_or(Vec3::X);
            self.pos = (flat(self.pos) + dir * BLINK_RANGE).clamp_length_max(ARENA_RADIUS);
            self.invuln = self.invuln.max(BLINK_INVULN);
        }
    }

    fn place_pickup(&mut self, kind: Pick) {
        for _ in 0..20 {
            let (a, b) = (self.rng.f32(), self.rng.f32());
            if let Some(pos) = pickup_spot(self.pos, a, b) {
                self.pickups.push(Pickup { kind, pos, life: PICKUP_LIFE });
                return;
            }
        }
    }

    /// Chasers that can hurt us soon: (position, closing speed).
    fn threats(&self) -> Vec<(Vec3, f32)> {
        self.enemies
            .iter()
            .filter_map(|e| {
                let d = flat(e.pos - self.pos).length();
                let info = e.kind.info();
                let danger = match info.attack {
                    Attack::Pounce { trigger } => d < trigger + 3.0,
                    _ => info.movement == Movement::Chase && d < 5.5 + e.speed * 0.4,
                };
                danger.then_some((e.pos, e.speed))
            })
            .collect()
    }

    fn bot_move(&mut self, _bot: &Bot) {
        let dt = DT;
        let speed_mult = if self.speed_buff > 0.0 { SPEED_BUFF } else { 1.0 };
        let threats = self.threats();
        // only flee from things we can't comfortably backpedal away from
        let fast_threat = threats.iter().any(|(_, s)| *s > WALK_SPEED * 0.85);
        let close_threat = threats.iter().any(|(p, _)| flat(*p - self.pos).length() < 3.0);
        if !threats.is_empty() && (fast_threat || close_threat) {
            self.fleeing = true;
        } else if threats.is_empty() {
            self.fleeing = false;
        }

        let mut away = Vec3::ZERO;
        for (p, _) in &threats {
            let d = flat(self.pos - *p);
            away += d.normalize_or_zero() / d.length().max(0.5);
        }
        // pull back toward the middle near the edge
        let edge = (flat(self.pos).length() / ARENA_RADIUS - 0.6).max(0.0) * 4.0;
        let center = -flat(self.pos).normalize_or_zero() * edge;

        let wish = if self.fleeing {
            // sprint forward (facing away, so no shooting)
            let dir = (away.normalize_or(Vec3::X) + center).normalize_or(Vec3::X);
            dir * WALK_SPEED * SPRINT_MULT * speed_mult
        } else {
            self.strafe_timer -= dt;
            if self.strafe_timer <= 0.0 {
                self.strafe = -self.strafe;
                self.strafe_timer = range(&mut self.rng, 1.5, 3.5);
            }
            let aim = self.target.and_then(|i| self.enemies.get(i)).map(|e| flat(e.pos - self.pos).normalize_or(Vec3::Z));
            let fwd = aim.unwrap_or(Vec3::Z);
            let side = Vec3::new(-fwd.z, 0.0, fwd.x) * self.strafe;
            let mut dir = side + away * 1.5 + center;
            if let Some(d) = self.incoming() {
                self.dodge_dir = d;
                self.dodge_timer = 0.4;
            }
            if self.dodge_timer > 0.0 {
                self.dodge_timer -= dt;
                dir = self.dodge_dir * 3.0 + center;
            }
            // detour for pickups we need
            if let Some(p) = self.wanted_pickup() {
                dir += flat(p - self.pos).normalize_or_zero() * 1.2;
            }
            dir.normalize_or_zero() * WALK_SPEED * speed_mult
        };
        let h = flat(self.vel);
        let nh = h + (wish - h) * (1.0 - (-14.0 * dt).exp());
        self.vel = Vec3::new(nh.x, 0.0, nh.z);
        self.pos += self.vel * dt;
        let hp = flat(self.pos);
        if hp.length() > ARENA_RADIUS {
            self.pos = hp.normalize() * ARENA_RADIUS;
        }
    }

    /// A noticed projectile that will pass within 1.2 m in the next 1.2 s: sidestep direction.
    fn incoming(&self) -> Option<Vec3> {
        for p in &self.projs {
            if !self.noticed.contains(&p.id) {
                continue;
            }
            if matches!(p.kind, ProjKind::Lob | ProjKind::Bomb) {
                // read the arc: where and when does it land?
                let (y, vy) = (p.pos.y, p.vel.y);
                let t_land = (vy + (vy * vy + 2.0 * LOB_GRAVITY * y).sqrt()) / LOB_GRAVITY;
                let land = flat(p.pos + p.vel * t_land);
                let me = flat(self.pos + self.vel * t_land);
                let danger = if p.kind == ProjKind::Bomb { EXPLOSION_RADIUS + 1.0 } else { 1.3 };
                if t_land < 1.6 && land.distance(me) < danger {
                    return Some((me - land).normalize_or(Vec3::X));
                }
                continue;
            }
            let rel = flat(p.pos - self.pos);
            let v = flat(p.vel);
            let vv = v.length_squared();
            if vv < 1e-3 {
                continue;
            }
            let tca = (-rel.dot(v) / vv).clamp(0.0, 1.2);
            if tca <= 0.0 {
                continue;
            }
            let miss = rel + v * tca;
            if miss.length() < 1.2 {
                let perp = Vec3::new(-v.z, 0.0, v.x).normalize_or_zero();
                // step to whichever side the shot is already missing toward
                return Some(if perp.dot(-miss) > 0.0 { perp } else { -perp });
            }
        }
        None
    }

    fn wanted_pickup(&self) -> Option<Vec3> {
        self.pickups
            .iter()
            .filter(|p| match p.kind {
                Pick::Ammo => self.reserve + self.mag < MAX_RESERVE - 20,
                Pick::Heart => self.hp < self.max_hp,
                Pick::Power(p) if p.is_item() => !self.inventory.contains(&p),
                _ => true,
            })
            .map(|p| p.pos)
            .filter(|p| p.distance(self.pos) < 30.0)
            .min_by(|a, b| a.distance(self.pos).total_cmp(&b.distance(self.pos)))
    }

    fn bot_shoot(&mut self, bot: &Bot) {
        let dt = DT;
        let rate = self.rate_mult();
        self.gun_cd -= dt;
        if self.reload > 0.0 {
            self.reload -= dt;
            if self.reload <= 0.0 {
                let take = (MAG_SIZE - self.mag).min(self.reserve);
                self.mag += take;
                self.reserve -= take;
            }
            return;
        }
        if self.fleeing {
            // tactical reload while running
            if self.mag < MAG_SIZE / 2 && self.reserve > 0 && self.burst_left == 0 {
                self.reload = RELOAD_TIME;
            }
            self.burst_left = 0;
            return;
        }

        // pick the target: incoming homing orbs first, then soonest-to-arrive enemy
        self.target_timer -= dt;
        if self.target_timer <= 0.0 || self.target.is_none_or(|i| i >= self.enemies.len()) {
            self.target_timer = bot.reaction;
            self.target = self
                .enemies
                .iter()
                .enumerate()
                .filter(|(_, e)| flat(e.pos - self.pos).length() < 45.0)
                .min_by(|(_, a), (_, b)| {
                    let score = |e: &Enemy| {
                        let d = flat(e.pos - self.pos).length();
                        if e.kind.info().movement == Movement::Chase { d / e.speed.max(0.5) } else { d / 4.0 + 1.5 }
                    };
                    score(a).total_cmp(&score(b))
                })
                .map(|(i, _)| i);
        }
        let orb = self
            .projs
            .iter()
            .position(|p| p.kind == ProjKind::Homing && p.pos.distance(self.pos) < 12.0);

        if self.burst_left == 0 && self.gun_cd <= 0.0 && (self.target.is_some() || orb.is_some()) {
            if self.mag == 0 {
                if self.reserve > 0 {
                    self.reload = RELOAD_TIME;
                }
                return;
            }
            self.burst_left = BURST;
            self.burst_timer = 0.0;
        }
        if self.burst_left == 0 {
            return;
        }
        self.burst_timer -= dt;
        if self.burst_timer > 0.0 {
            return;
        }
        self.burst_timer = BURST_GAP / rate;
        self.burst_left -= 1;
        self.mag -= 1;
        self.res.fired += 1;
        if self.burst_left == 0 || self.mag == 0 {
            self.burst_left = 0;
            self.gun_cd = BURST_COOLDOWN / rate;
        }
        let falloff = |d: f32| (1.15 - d / 45.0).clamp(0.35, 1.0);
        if let Some(i) = orb {
            let d = self.projs[i].pos.distance(self.pos);
            if self.rng.f32() < bot.accuracy * falloff(d) * 0.8 {
                self.projs.swap_remove(i);
                self.res.hits += 1;
            }
            return;
        }
        let Some(i) = self.target else { return };
        let to = flat(self.enemies[i].pos - self.pos);
        let d = to.length();
        if self.rng.f32() < bot.accuracy * falloff(d) {
            self.res.hits += 1;
            let mut struck = vec![i];
            if self.pierce_buff > 0.0 {
                // the round carries on along the same line through enemies behind the target
                let dir = to.normalize_or(Vec3::X);
                let mut behind: Vec<(f32, usize)> = self
                    .enemies
                    .iter()
                    .enumerate()
                    .filter(|(j, _)| *j != i)
                    .filter_map(|(j, e)| {
                        let rel = flat(e.pos - self.pos);
                        let along = rel.dot(dir);
                        let off = (rel - dir * along).length();
                        (along > d && off < e.kind.info().radius()).then_some((along, j))
                    })
                    .collect();
                behind.sort_by(|a, b| a.0.total_cmp(&b.0));
                struck.extend(behind.into_iter().take(PIERCE_MAX - 1).map(|(_, j)| j));
            }
            for &j in &struck {
                self.enemies[j].hp -= ROUND_DAMAGE;
            }
            let before = self.enemies.len();
            let mut drops = vec![];
            self.enemies.retain(|e| {
                if e.hp <= 0.0 {
                    drops.push(flat(e.pos));
                }
                e.hp > 0.0
            });
            self.res.kills += (before - self.enemies.len()) as u32;
            if self.enemies.len() != before {
                self.target = None;
            }
            for pos in drops {
                if self.rng.f32() < AMMO_DROP_CHANCE {
                    self.pickups.push(Pickup { kind: Pick::Ammo, pos, life: PICKUP_LIFE });
                }
            }
        }
    }

    fn enemies_step(&mut self) {
        let dt = DT;
        let ppos = self.pos;
        let chest = ppos + Vec3::Y * PLAYER_H * 0.6;
        let snapshot: Vec<(Vec3, f32, bool)> =
            self.enemies.iter().map(|e| (flat(e.pos), e.kind.info().radius(), e.kind.info().hover > 0.0)).collect();
        let mut hits = vec![];
        let mut new_projs = vec![];
        for (idx, en) in self.enemies.iter_mut().enumerate() {
            let info = en.kind.info();
            let pos = en.pos;
            let to = flat(ppos - pos);
            let dist = to.length().max(0.01);
            let dir = to / dist;
            let side = Vec3::new(-dir.z, 0.0, dir.x) * en.strafe;
            en.cooldown -= dt;
            let mv = if en.airborne { Vec3::ZERO } else { steer(en.kind, en.speed, dist, dir, side) };
            if en.cooldown <= 0.0 && !en.airborne {
                match info.attack {
                    Attack::Melee => {}
                    Attack::Pounce { trigger } => {
                        if dist < trigger {
                            en.vel = pounce_velocity(dir, dist);
                            en.airborne = true;
                            en.cooldown = range(&mut self.rng, info.cooldown.0, info.cooldown.1);
                        }
                    }
                    Attack::Shoot { max_dist, .. } => {
                        if dist < max_dist {
                            en.cooldown = range(&mut self.rng, info.cooldown.0, info.cooldown.1);
                            if let Some((kind, from, vel)) = launch(en.kind, pos, ppos, self.vel, chest) {
                                let (radius, life) = proj_stats(kind);
                                new_projs.push(Proj { id: 0, kind, pos: from, vel, life, radius });
                            }
                        }
                    }
                }
            }
            if self.rng.f32() < dt * 0.25 {
                en.strafe = -en.strafe;
            }
            let my = flat(pos);
            let r = info.radius();
            let flyer = info.hover > 0.0;
            let mut push = Vec3::ZERO;
            for (j, (op, or, ofly)) in snapshot.iter().enumerate() {
                if j == idx || *ofly != flyer {
                    continue;
                }
                let d = my - *op;
                let l = d.length();
                let min = (r + or) * 0.8;
                if l < min && l > 1e-4 {
                    push += d / l * (min - l);
                }
            }
            if en.airborne {
                en.vel.y -= GRAVITY * dt;
                en.pos += en.vel * dt;
                if en.pos.y <= info.hover && en.vel.y < 0.0 {
                    en.pos.y = info.hover;
                    en.airborne = false;
                    en.vel = Vec3::ZERO;
                }
            } else {
                en.pos += (mv + push * 4.0) * dt;
                if flyer {
                    en.pos.y = info.hover + (self.t * 2.0 + en.phase).sin() * 0.25;
                }
            }
            let h = flat(en.pos).clamp_length_max(ARENA_RADIUS + 5.0);
            en.pos.x = h.x;
            en.pos.z = h.z;
            if dist > RELOCATE_DIST {
                let a = self.rng.f32() * std::f32::consts::TAU;
                let d = range(&mut self.rng, SPAWN_DIST.0, SPAWN_DIST.1);
                en.pos = spawn_position(ppos, a, d) + Vec3::Y * info.hover;
            }
            let horiz = flat(ppos - en.pos).length();
            if horiz < r + PLAYER_RADIUS && en.pos.y < ppos.y + PLAYER_H && en.pos.y + info.height > ppos.y {
                hits.push((en.pos, Source::Contact(en.kind)));
                let away = flat(en.pos - ppos).normalize_or(-dir);
                if !en.airborne || en.vel.dot(away) < 0.0 {
                    en.vel = melee_recoil(away);
                    en.airborne = true;
                }
            }
        }
        for mut p in new_projs {
            p.id = self.next_proj_id;
            self.next_proj_id += 1;
            // decide once per projectile whether the bot will notice it
            if self.rng.f32() < self.dodge_skill {
                self.noticed.push(p.id);
            }
            self.projs.push(p);
        }
        if self.res.first_engage.is_nan() && self.enemies.iter().any(|e| flat(e.pos - ppos).length() < 10.0) {
            self.res.first_engage = self.t;
        }
        for (from, src) in hits {
            if self.res.first_contact.is_nan() {
                self.res.first_contact = self.t;
            }
            self.hurt(from, src);
        }
    }

    fn projectiles_step(&mut self) {
        let dt = DT;
        let ppos = self.pos;
        let chest = ppos + Vec3::Y * PLAYER_H * 0.55;
        let mut hits = vec![];
        let noticed = &self.noticed;
        self.projs.retain_mut(|p| {
            p.life -= dt;
            p.vel = step_projectile(p.kind, p.pos, p.vel, chest, dt);
            p.pos += p.vel * dt;
            let mut gone = p.life <= 0.0 || flat(p.pos).length() > ARENA_RADIUS + 12.0;
            let mut explode = false;
            // rolling balls the bot noticed get jumped over
            if p.kind == ProjKind::Roll && noticed.contains(&p.id) {
                return !gone;
            }
            if hits_player(ppos, p.pos, p.radius) {
                if p.kind == ProjKind::Bomb {
                    explode = true;
                } else {
                    hits.push((p.pos - p.vel.normalize_or_zero(), Source::Proj(p.kind)));
                }
                gone = true;
            } else if p.pos.y <= p.radius * 0.8 && p.kind != ProjKind::Roll {
                gone = true;
                explode = p.kind == ProjKind::Bomb;
            }
            if explode {
                let center = ppos + Vec3::Y * PLAYER_H * 0.5;
                if center.distance(Vec3::new(p.pos.x, 0.3, p.pos.z)) < EXPLOSION_RADIUS {
                    hits.push((p.pos, Source::Proj(ProjKind::Bomb)));
                }
            }
            !gone
        });
        for (from, src) in hits {
            self.hurt(from, src);
        }
    }

    fn collect(&mut self) {
        let dt = DT;
        let mut i = 0;
        while i < self.pickups.len() {
            self.pickups[i].life -= dt;
            let p = &self.pickups[i];
            if p.life <= 0.0 {
                self.pickups.swap_remove(i);
                continue;
            }
            if flat(p.pos - self.pos).length() <= PICKUP_RADIUS {
                let taken = match p.kind {
                    Pick::Ammo if self.reserve < MAX_RESERVE => {
                        self.reserve = (self.reserve + AMMO_AMOUNT).min(MAX_RESERVE);
                        true
                    }
                    Pick::Heart if self.hp < self.max_hp => {
                        self.hp += 1;
                        true
                    }
                    Pick::Power(p) if p.is_item() => {
                        let have = self.inventory.contains(&p);
                        if !have {
                            self.inventory.push(p);
                        }
                        !have
                    }
                    Pick::Power(Powerup::MaxHealth) => {
                        self.max_hp += 1;
                        self.hp += 1;
                        true
                    }
                    Pick::Power(Powerup::Speed) => {
                        self.speed_buff = range(&mut self.rng, BUFF_DURATION.0, BUFF_DURATION.1);
                        true
                    }
                    Pick::Power(Powerup::Piercing) => {
                        self.pierce_buff = range(&mut self.rng, BUFF_DURATION.0, BUFF_DURATION.1);
                        true
                    }
                    Pick::Power(_) => {
                        self.rate_buff = range(&mut self.rng, BUFF_DURATION.0, BUFF_DURATION.1);
                        true
                    }
                    _ => false,
                };
                if taken {
                    self.pickups.swap_remove(i);
                    continue;
                }
            }
            i += 1;
        }
    }
}

fn default_result() -> RunResult {
    RunResult::default()
}

pub fn run(seed: u64, bot: &Bot) -> RunResult {
    let mut w = World::new(seed);
    w.dodge_skill = bot.dodge;
    while w.hp > 0 && w.t < MAX_TIME {
        w.step(bot);
    }
    w.res.time = w.t;
    w.res
}

fn pct(v: &mut [f32], p: f32) -> f32 {
    v.sort_by(|a, b| a.total_cmp(b));
    v[((v.len() - 1) as f32 * p).round() as usize]
}

fn mmss(s: f32) -> String {
    format!("{:02}:{:02}", s as u32 / 60, s as u32 % 60)
}

/// Static numbers that don't need simulation.
fn static_report() {
    println!("\n== gun");
    println!(
        "trigger {:.2} rps, sustained {:.2} rps (with reload), with fire-rate buff {:.2} / {:.2}",
        trigger_rps(1.0),
        sustained_rps(1.0),
        trigger_rps(FIRE_RATE_BUFF),
        sustained_rps(FIRE_RATE_BUFF)
    );
    println!("walk {:.1} m/s, sprint {:.1} m/s, speed buff sprint {:.1} m/s", WALK_SPEED, sprint_speed(), sprint_speed() * SPEED_BUFF);

    println!("\n== enemies: hp / speed / seconds to kill at 60% accuracy (sustained) / seconds to reach you from spawn");
    for t in [0.0f32, 120.0, 300.0, 600.0] {
        println!("-- t = {}", mmss(t));
        for k in Kind::ALL {
            let i = k.info();
            if i.unlock > t {
                continue;
            }
            let hp = i.hp * hp_mult(t);
            let speed = i.speed * speed_mult(t);
            let ttk = hp / (sustained_rps(1.0) * 0.6);
            let reach = SPAWN_DIST.0 / speed;
            let escape = if speed * 1.1 < sprint_speed() { "sprint escapes" } else { "CAN'T OUTRUN" };
            let walk = if speed * 1.1 < WALK_SPEED { ", walk escapes" } else { "" };
            println!(
                "  {:<12} hp {:>5.1}  speed {:>4.1}  ttk {:>4.1}s  reach {:>4.1}s  {}{}",
                i.key, hp, speed, ttk, reach, escape, walk
            );
        }
        let avg_hp: f32 = {
            let ks: Vec<_> = Kind::ALL.iter().filter(|k| k.info().unlock <= t).collect();
            let w: f32 = ks.iter().map(|k| k.info().weight).sum();
            ks.iter().map(|k| k.info().hp * k.info().weight).sum::<f32>() / w * hp_mult(t)
        };
        let demand = spawn_rate(t) * avg_hp;
        let supply_rounds = AMMO_AMOUNT as f32 / ((AMMO_INTERVAL.0 + AMMO_INTERVAL.1) / 2.0);
        println!(
            "  incoming {:.2} enemies/s x {:.1} hp = {:.1} hp/s; you deal {:.1} dps at 60%; field ammo {:.1} rounds/s + drops",
            spawn_rate(t),
            avg_hp,
            demand,
            sustained_rps(1.0) * 0.6,
            supply_rounds
        );
    }
}

#[test]
fn balance_report() {
    static_report();
    let runs = std::env::var("SIM_RUNS").ok().and_then(|v| v.parse().ok()).unwrap_or(40u64);
    println!("\n== simulated runs ({runs} per skill level)");
    // (name, accuracy, target-switch delay, dodge chance)
    let levels = [
        ("bad (35% aim)", 0.35, 0.45, 0.3),
        ("ok (55% aim)", 0.55, 0.3, 0.5),
        ("good (75% aim)", 0.75, 0.2, 0.7),
        ("great (90% aim)", 0.9, 0.12, 0.85),
    ];
    let handles: Vec<_> = levels
        .iter()
        .map(|&(name, acc, react, dodge)| {
            std::thread::spawn(move || {
                let bot = Bot { accuracy: acc, reaction: react, dodge };
                let results: Vec<RunResult> = (0..runs).map(|s| run(s * 7919 + 13, &bot)).collect();
                (name, results)
            })
        })
        .collect();
    for h in handles {
        let (name, rs) = h.join().unwrap();
        let mut times: Vec<f32> = rs.iter().map(|r| r.time).collect();
        let mut contact: Vec<f32> = rs.iter().map(|r| r.first_contact).filter(|v| v.is_finite()).collect();
        let mut engage: Vec<f32> = rs.iter().map(|r| r.first_engage).filter(|v| v.is_finite()).collect();
        let total_time: f32 = rs.iter().map(|r| r.time).sum();
        let dry: f32 = rs.iter().map(|r| r.dry_time).sum::<f32>() / total_time;
        let flee: f32 = rs.iter().map(|r| r.flee_time).sum::<f32>() / total_time;
        let fired: u32 = rs.iter().map(|r| r.fired).sum();
        let hits: u32 = rs.iter().map(|r| r.hits).sum();
        let kills: f32 = rs.iter().map(|r| r.kills as f32).sum::<f32>() / rs.len() as f32;
        let items: f32 = rs.iter().map(|r| r.items_used as f32).sum::<f32>() / rs.len() as f32;
        let saves: f32 = rs.iter().map(|r| r.shield_saves as f32).sum::<f32>() / rs.len() as f32;
        let mut by: std::collections::HashMap<String, u32> = Default::default();
        for r in &rs {
            for (s, _) in &r.damage {
                let k = match s {
                    Source::Contact(k) => format!("touch {}", k.info().key),
                    Source::Proj(p) => format!("{p:?}"),
                };
                *by.entry(k).or_default() += 1;
            }
        }
        let mut by: Vec<_> = by.into_iter().collect();
        by.sort_by(|a, b| b.1.cmp(&a.1));
        let dmg_total: u32 = by.iter().map(|b| b.1).sum();
        println!(
            "{name:<16} survive p10 {} | median {} | p90 {}   first enemy close ~{:.0}s   first touch ~{}   kills {:.0}   hit rate {:.0}%   out of ammo {:.1}% of time   fleeing {:.0}%",
            mmss(pct(&mut times, 0.1)),
            mmss(pct(&mut times, 0.5)),
            mmss(pct(&mut times, 0.9)),
            if engage.is_empty() { f32::NAN } else { pct(&mut engage, 0.5) },
            if contact.is_empty() { "-".into() } else { mmss(pct(&mut contact, 0.5)) },
            kills,
            100.0 * hits as f32 / fired.max(1) as f32,
            dry * 100.0,
            flee * 100.0
        );
        let line: Vec<String> = by.iter().take(8).map(|(k, n)| format!("{k} {:.0}%", 100.0 * *n as f32 / dmg_total as f32)).collect();
        println!("                 damage from: {}", line.join(", "));
        println!("                 items used per run {items:.1}, hits absorbed by shield per run {saves:.1}");
    }
}
