use bevy::prelude::*;

pub use crate::balance::Kind;
use crate::balance::{self, Attack, ARENA_RADIUS, GRAVITY, PLAYER_H, PLAYER_RADIUS};
use crate::fx::{Billboard, Fx, Particle};
use crate::pickup::{spawn_pickup, PickKind};
use crate::player::{Player, PlayerHit};
use crate::projectile::spawn_projectile;
use crate::{flat, rand_range, GameState, RunBegin, RunClock, RunEntity};

#[derive(Component)]
pub struct Enemy {
    pub kind: Kind,
    pub hp: f32,
    speed: f32,
    cooldown: f32,
    hurt: f32,
    vel: Vec3,
    airborne: bool,
    strafe: f32,
    phase: f32,
    anim: f32,
    mat: Handle<StandardMaterial>,
}

#[derive(Component)]
pub struct Dying {
    t: f32,
}

#[derive(Message)]
pub struct EnemyHit {
    pub entity: Entity,
    pub point: Vec3,
    pub dir: Vec3,
    pub damage: f32,
    /// launch speed away from `dir`, 0 for a plain bullet
    pub knock: f32,
}

#[derive(Resource, Default)]
struct Director {
    budget: f32,
}

pub struct EnemyPlugin;

impl Plugin for EnemyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Director>()
            .add_message::<EnemyHit>()
            .add_systems(Update, reset)
            .add_systems(
                Update,
                (spawn_enemies, ai, take_hits, animate).chain().run_if(in_state(GameState::Playing)),
            )
            .add_systems(Update, dying.run_if(in_state(GameState::Playing).or_else(in_state(GameState::Dead))));
    }
}

fn reset(mut msgs: MessageReader<RunBegin>, mut d: ResMut<Director>, mut fx: Fx) {
    if msgs.read().count() == 0 {
        return;
    }
    d.budget = balance::START_BUDGET;
    let start = crate::player::START_POS;
    for (kind, dist) in balance::OPENING_WAVE {
        // in front of the player's starting view, spread out
        let a = std::f32::consts::FRAC_PI_2 + rand_range(-0.9, 0.9);
        spawn_enemy(&mut fx, *kind, balance::spawn_position(start, a, *dist), 0.0);
    }
}

fn spawn_enemies(
    time: Res<Time>,
    clock: Res<RunClock>,
    mut dir: ResMut<Director>,
    mut fx: Fx,
    players: Query<&Transform, With<Player>>,
    alive: Query<(), (With<Enemy>, Without<Dying>)>,
) {
    let Ok(pt) = players.single() else { return };
    let t = clock.0;
    dir.budget += balance::spawn_rate(t) * time.delta_secs();
    while dir.budget >= 1.0 {
        dir.budget -= 1.0;
        if alive.iter().count() >= balance::alive_cap(t) {
            continue;
        }
        let kind = balance::pick_kind(t, fastrand::f32());
        let d = rand_range(balance::SPAWN_DIST.0, balance::SPAWN_DIST.1);
        let pos = balance::spawn_position(pt.translation, rand_range(0.0, std::f32::consts::TAU), d);
        spawn_enemy(&mut fx, kind, pos, t);
    }
}

fn spawn_enemy(fx: &mut Fx, kind: Kind, pos: Vec3, t: f32) {
    let info = kind.info();
    let frame = fx.assets.enemies[&kind].idle[0].clone();
    let mat = fx.materials.add(StandardMaterial {
        base_color_texture: Some(frame.clone()),
        emissive: LinearRgba::rgb(0.2, 0.2, 0.2),
        emissive_texture: Some(frame),
        alpha_mode: AlphaMode::Mask(0.45),
        cull_mode: None,
        perceptual_roughness: 0.85,
        reflectance: 0.15,
        ..default()
    });
    let w = info.height * info.aspect;
    let quad = fx.assets.quad.clone();
    let speed_mult = balance::speed_mult(t);
    fx.commands
        .spawn((
            RunEntity,
            Enemy {
                kind,
                hp: info.hp * balance::hp_mult(t),
                speed: info.speed * speed_mult * rand_range(0.9, 1.1),
                cooldown: rand_range(1.0, 2.5),
                hurt: 0.0,
                vel: Vec3::ZERO,
                airborne: false,
                strafe: if fastrand::bool() { 1.0 } else { -1.0 },
                phase: rand_range(0.0, 6.28),
                anim: rand_range(0.0, 1.0),
                mat: mat.clone(),
            },
            Transform::from_translation(pos + Vec3::Y * info.hover),
            Visibility::default(),
        ))
        .with_child((
            Mesh3d(quad),
            MeshMaterial3d(mat),
            Transform::from_xyz(0.0, info.height / 2.0, 0.0).with_scale(Vec3::new(w, info.height, 1.0)),
            Billboard {
                upright: true,
                spin: 0.0,
            },
        ));
    let smoke = fx.assets.p_smoke.clone();
    for _ in 0..3 {
        fx.particle(
            &smoke,
            pos + Vec3::Y * (info.hover + 0.4),
            Particle {
                vel: Vec3::new(rand_range(-1.0, 1.0), 1.2, rand_range(-1.0, 1.0)),
                drag: 1.5,
                max_life: 0.8,
                size: (0.8, 2.0),
                alpha: 0.5,
                color: Color::srgb(0.4, 0.4, 0.45),
                ..default()
            },
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn ai(
    time: Res<Time>,
    mut fx: Fx,
    players: Query<(&Transform, &Player), Without<Enemy>>,
    mut enemies: Query<(Entity, &mut Transform, &mut Enemy), Without<Dying>>,
    mut hit_player: MessageWriter<PlayerHit>,
) {
    let Ok((pt, player)) = players.single() else { return };
    let dt = time.delta_secs();
    let now = time.elapsed_secs();
    let ppos = pt.translation;
    let chest = ppos + Vec3::Y * PLAYER_H * player.height_scale() * 0.6;

    // gentle separation so crowds don't stack into one sprite
    let snapshot: Vec<(Entity, Vec3, f32, bool)> = enemies
        .iter()
        .map(|(e, t, en)| (e, flat(t.translation), en.kind.info().radius(), en.kind.info().hover > 0.0))
        .collect();

    for (e, mut t, mut en) in &mut enemies {
        let info = en.kind.info();
        let pos = t.translation;
        let to = flat(ppos - pos);
        let dist = to.length().max(0.01);
        let dir = to / dist;
        let side = Vec3::new(-dir.z, 0.0, dir.x) * en.strafe;
        en.cooldown -= dt;
        en.hurt -= dt;
        let mv = if en.airborne { Vec3::ZERO } else { balance::steer(en.kind, en.speed, dist, dir, side) };
        if en.cooldown <= 0.0 && !en.airborne {
            match info.attack {
                Attack::Melee => {}
                Attack::Pounce { trigger } => {
                    // pounce from a fixed distance, no telegraph
                    if dist < trigger {
                        en.vel = balance::pounce_velocity(dir, dist);
                        en.airborne = true;
                        en.cooldown = rand_range(info.cooldown.0, info.cooldown.1);
                        let s = fx.assets.sfx.pounce.clone();
                        fx.sound(&s, 0.5);
                    }
                }
                Attack::Shoot { max_dist, .. } => {
                    if dist < max_dist {
                        en.cooldown = rand_range(info.cooldown.0, info.cooldown.1);
                        if let Some((kind, from, vel)) = balance::launch(en.kind, pos, ppos, player.vel, chest) {
                            spawn_projectile(&mut fx, kind, from, vel);
                            let (sound, vol) = match kind {
                                balance::ProjKind::Linear => (fx.assets.sfx.enemy_shoot.clone(), 0.35),
                                balance::ProjKind::Homing => (fx.assets.sfx.homing_fire.clone(), 0.4),
                                _ => (fx.assets.sfx.enemy_lob.clone(), 0.33),
                            };
                            fx.sound(&sound, vol);
                        }
                    }
                }
            }
        }

        // occasionally flip strafe direction
        if fastrand::f32() < dt * 0.25 {
            en.strafe = -en.strafe;
        }

        let my = flat(pos);
        let r = info.radius();
        let flyer = info.hover > 0.0;
        let mut push = Vec3::ZERO;
        for (oe, op, or, ofly) in &snapshot {
            if *oe == e || *ofly != flyer {
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
            let v = en.vel;
            t.translation += v * dt;
            if t.translation.y <= info.hover && en.vel.y < 0.0 {
                t.translation.y = info.hover;
                en.airborne = false;
                en.vel = Vec3::ZERO;
            }
        } else {
            t.translation += (mv + push * 4.0) * dt;
            if flyer {
                t.translation.y = info.hover + (now * 2.0 + en.phase).sin() * 0.25;
            }
        }
        let h = flat(t.translation).clamp_length_max(ARENA_RADIUS + 5.0);
        t.translation.x = h.x;
        t.translation.z = h.z;
        if dist > balance::RELOCATE_DIST {
            let d = rand_range(balance::SPAWN_DIST.0, balance::SPAWN_DIST.1);
            let a = rand_range(0.0, std::f32::consts::TAU);
            t.translation = balance::spawn_position(ppos, a, d) + Vec3::Y * info.hover;
        }

        // contact damage
        let pos = t.translation;
        let horiz = flat(ppos - pos).length();
        let top = ppos.y + PLAYER_H * player.height_scale();
        if horiz < r + PLAYER_RADIUS && pos.y < top && pos.y + info.height > ppos.y {
            hit_player.write(PlayerHit { from: pos });
            // bounce off her, unless already flying away (a pounce that connected bounces too)
            let away = flat(pos - ppos).normalize_or(-dir);
            if !en.airborne || en.vel.dot(away) < 0.0 {
                en.vel = balance::melee_recoil(away);
                en.airborne = true;
            }
        }
    }
}

fn take_hits(
    mut hits: MessageReader<EnemyHit>,
    mut commands: Commands,
    mut fx: Fx,
    mut enemies: Query<(&mut Enemy, &mut Transform), Without<Dying>>,
) {
    for hit in hits.read() {
        let Ok((mut en, mut t)) = enemies.get_mut(hit.entity) else { continue };
        en.hp -= hit.damage;
        en.hurt = 0.22;
        if hit.knock > 0.0 {
            en.vel = flat(hit.dir).normalize_or_zero() * hit.knock + Vec3::Y * hit.knock * 0.45;
            en.airborne = true;
        } else {
            t.translation += flat(hit.dir) * 0.12;
        }
        let spark = fx.assets.p_spark.clone();
        fx.burst(&spark, hit.point, 3, 5.0, 0.3, Color::srgb(2.0, 1.2, 0.5), 0.25);
        let s = fx.assets.sfx.enemy_hit.clone();
        fx.sound(&s, 0.3);
        if en.hp <= 0.0 {
            commands.entity(hit.entity).insert(Dying { t: 0.0 });
            let s = fx.assets.sfx.enemy_die.clone();
            fx.sound(&s, 0.5);
            let star = fx.assets.p_star.clone();
            let c = t.translation + Vec3::Y * en.kind.info().height * 0.5;
            fx.burst(&star, c, 8, 6.0, 0.45, Color::srgb(1.6, 1.4, 2.4), 0.6);
            if fastrand::f32() < balance::AMMO_DROP_CHANCE {
                spawn_pickup(&mut fx, PickKind::Ammo, flat(t.translation));
            }
        }
    }
}

fn animate(
    time: Res<Time>,
    assets: Res<crate::assets::GameAssets>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut enemies: Query<&mut Enemy, Without<Dying>>,
) {
    for mut en in &mut enemies {
        en.anim += time.delta_secs();
        let frames = &assets.enemies[&en.kind];
        let (set, tint) = if en.hurt > 0.0 {
            (&frames.hurt, Color::srgb(1.0, 0.6, 0.6))
        } else {
            (&frames.idle, Color::WHITE)
        };
        let img = &set[(en.anim * 6.0) as usize % set.len()];
        if let Some(mut m) = materials.get_mut(&en.mat) {
            if m.base_color_texture.as_ref() != Some(img) {
                m.base_color_texture = Some(img.clone());
                m.emissive_texture = Some(img.clone());
            }
            if m.base_color != tint {
                m.base_color = tint;
            }
        }
    }
}

fn dying(
    mut commands: Commands,
    time: Res<Time>,
    assets: Res<crate::assets::GameAssets>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut q: Query<(Entity, &Enemy, &mut Dying, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (e, en, mut d, mut t) in &mut q {
        d.t += dt;
        if t.translation.y > 0.0 {
            t.translation.y = (t.translation.y - dt * 6.0).max(0.0);
        }
        let frames = &assets.enemies[&en.kind].dead;
        let i = ((d.t * 8.0) as usize).min(frames.len() - 1);
        let fade = (1.0 - (d.t - 0.6) / 0.5).clamp(0.0, 1.0);
        if let Some(mut m) = materials.get_mut(&en.mat) {
            m.base_color_texture = Some(frames[i].clone());
            m.emissive_texture = Some(frames[i].clone());
            m.base_color = Color::WHITE.with_alpha(fade);
            m.emissive = LinearRgba::rgb(0.2, 0.2, 0.2) * fade;
        }
        if d.t > 1.1 {
            commands.entity(e).despawn();
        }
    }
}
