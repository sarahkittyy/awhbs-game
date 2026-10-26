use std::collections::VecDeque;

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use crate::arena::{ArenaRay, GameCamera};
pub use crate::balance::{MAG_SIZE, MAX_RESERVE, RELOAD_TIME};
use crate::balance::{self, BURST, BURST_COOLDOWN, BURST_GAP, GUN_RANGE as RANGE, SPREAD, START_RESERVE};
use crate::enemy::{Dying, Enemy, EnemyHit};
use crate::fx::{faded, Billboard, Fx, Look, MatCache, Particle, ALPHA_STEPS};
use crate::pickup::Buffs;
use crate::gun_model::GunMuzzle;
use crate::player::{CamRig, Player};
use crate::projectile::{ProjKind, Projectile};
use crate::{rand_range, GameState, RunEntity, RunBegin};

#[derive(Resource)]
pub struct Gun {
    pub mag: u32,
    pub reserve: u32,
    burst_left: u32,
    burst_timer: f32,
    cooldown: f32,
    /// Seconds left on the current reload, 0 when not reloading.
    pub reloading: f32,
    /// HUD hit marker timer.
    pub hit_marker: f32,
    /// Rounds fired this run; the gun model animates on changes.
    pub shots_fired: u32,
}

impl Default for Gun {
    fn default() -> Self {
        Self {
            mag: MAG_SIZE,
            reserve: START_RESERVE,
            burst_left: 0,
            burst_timer: 0.0,
            cooldown: 0.0,
            reloading: 0.0,
            hit_marker: 0.0,
            shots_fired: 0,
        }
    }
}

const MAX_DECALS: usize = 160;
const DECAL_LIFE: f32 = 25.0;

/// Bullet holes, oldest first, so the count stays bounded.
#[derive(Resource, Default)]
struct Decals(VecDeque<Entity>);

#[derive(Component)]
struct Decal {
    life: f32,
    step: u8,
}

const DECAL_COLOR: Color = Color::srgba(0.07, 0.07, 0.08, 0.9);

#[derive(SystemParam)]
struct WorldRay<'w, 's> {
    arena: ArenaRay<'w, 's>,
    decals: ResMut<'w, Decals>,
}

impl WorldRay<'_, '_> {
    fn cast(&mut self, origin: Vec3, dir: Vec3) -> Option<(f32, Vec3)> {
        self.arena.cast(origin, dir)
    }
}

#[derive(Component)]
struct Tracer {
    life: f32,
}

pub struct GunPlugin;

impl Plugin for GunPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Gun>()
            .init_resource::<Decals>()
            .add_systems(Update, reset_gun)
            .add_systems(
                Update,
                (shoot, fade_tracers, fade_decals).run_if(in_state(GameState::Playing).or_else(in_state(GameState::Dead))),
            );
    }
}

fn reset_gun(mut msgs: MessageReader<RunBegin>, mut gun: ResMut<Gun>) {
    if msgs.read().count() > 0 {
        *gun = Gun::default();
    }
}

#[allow(clippy::too_many_arguments)]
fn shoot(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    state: Res<State<GameState>>,
    buffs: Res<Buffs>,
    mut gun: ResMut<Gun>,
    mut rig: ResMut<CamRig>,
    mut fx: Fx,
    cam: Query<&GlobalTransform, With<GameCamera>>,
    muzzle: Query<&GlobalTransform, With<GunMuzzle>>,
    enemies: Query<(Entity, &GlobalTransform, &Enemy), Without<Dying>>,
    projectiles: Query<(Entity, &GlobalTransform, &Projectile)>,
    players: Query<&Player>,
    mut hits: MessageWriter<EnemyHit>,
    mut world: WorldRay,
) {
    let dt = time.delta_secs();
    gun.hit_marker = (gun.hit_marker - dt).max(0.0);
    if *state.get() != GameState::Playing || players.single().is_err() {
        return;
    }
    let rate = buffs.fire_rate_mult();
    gun.cooldown -= dt;

    if gun.reloading > 0.0 {
        gun.reloading -= dt;
        if gun.reloading <= 0.0 {
            gun.reloading = 0.0;
            let take = (MAG_SIZE - gun.mag).min(gun.reserve);
            gun.mag += take;
            gun.reserve -= take;
        }
        return;
    }

    let wants_reload = keys.just_pressed(KeyCode::KeyR) && gun.mag < MAG_SIZE && gun.reserve > 0;
    if wants_reload && gun.burst_left == 0 {
        start_reload(&mut gun, &mut fx);
        return;
    }

    if gun.burst_left == 0 && gun.cooldown <= 0.0 && mouse.pressed(MouseButton::Left) {
        if gun.mag == 0 {
            if gun.reserve > 0 {
                start_reload(&mut gun, &mut fx);
            } else if mouse.just_pressed(MouseButton::Left) {
                let s = fx.assets.sfx.dry_fire.clone();
                fx.sound(&s, 0.6);
                gun.cooldown = 0.25;
            }
            return;
        }
        gun.burst_left = BURST;
        gun.burst_timer = 0.0;
        // the ripped Halo 3 burst already contains all three rounds
        let s = fx.assets.sfx.br_burst.clone();
        let vol = if gun.mag >= BURST { 0.55 } else { 0.0 };
        if vol > 0.0 {
            fx.sound_speed(&s, vol, rate);
        } else {
            let s = fx.assets.sfx.br_shot.clone();
            fx.sound_speed(&s, 0.55, rate);
        }
    }

    if gun.burst_left == 0 {
        return;
    }
    gun.burst_timer -= dt;
    if gun.burst_timer > 0.0 {
        return;
    }
    gun.burst_timer = BURST_GAP / rate;
    gun.burst_left -= 1;
    gun.mag -= 1;
    gun.shots_fired = gun.shots_fired.wrapping_add(1);
    if gun.burst_left == 0 || gun.mag == 0 {
        gun.burst_left = 0;
        gun.cooldown = BURST_COOLDOWN / rate;
    }

    let Ok(cam) = cam.single() else { return };
    let Ok(muzzle_t) = muzzle.single() else { return };
    let origin = cam.translation();
    let spread = Vec3::new(rand_range(-1.0, 1.0), rand_range(-1.0, 1.0), 0.0) * SPREAD;
    let dir = (cam.rotation() * (Vec3::NEG_Z + spread)).normalize();

    // arena geometry stops the round; past the floor mesh the void floor is still y = 0
    let mut wall = RANGE;
    let mut surface = None;
    if let Some((d, n)) = world.cast(origin, dir) {
        wall = d;
        surface = Some(n);
    } else if dir.y < 0.0 {
        wall = wall.min(-origin.y / dir.y);
        surface = Some(Vec3::Y);
    }
    // everything shootable in front of the wall, nearest first
    let mut along: Vec<(f32, Entity, bool)> = Vec::new();
    for (e, t, enemy) in &enemies {
        let info = enemy.kind.info();
        if let Some(d) = ray_cylinder(origin, dir, t.translation(), info.radius(), info.height) {
            if d < wall {
                along.push((d, e, true));
            }
        }
    }
    for (e, t, p) in &projectiles {
        if p.kind != ProjKind::Homing {
            continue;
        }
        if let Some(d) = ray_sphere(origin, dir, t.translation(), p.radius + 0.25) {
            if d < wall {
                along.push((d, e, false));
            }
        }
    }
    along.sort_by(|a, b| a.0.total_cmp(&b.0));

    // a normal round stops at the first thing it hits; a piercing one carries on through up to
    // PIERCE_MAX enemies (orbs it passes on the way pop for free)
    let pierce = buffs.pierce();
    let mut enemies_hit = 0;
    let mut end = wall;
    let mut stopped = false;
    for (d, e, is_enemy) in along {
        let point = origin + dir * d;
        end = d;
        if is_enemy {
            hits.write(EnemyHit {
                entity: e,
                point,
                dir,
                damage: balance::ROUND_DAMAGE,
                knock: 0.0,
            });
            enemies_hit += 1;
        } else {
            fx.commands.entity(e).despawn();
            let s = fx.assets.sfx.projectile_pop.clone();
            fx.sound(&s, 0.6);
            let img = fx.assets.p_star.clone();
            fx.burst(&img, point, 10, 6.0, 0.35, Color::srgb(2.5, 0.8, 0.3), 0.4);
        }
        gun.hit_marker = 0.12;
        if pierce == 1 || enemies_hit >= pierce {
            stopped = true;
            break;
        }
    }
    if !stopped {
        end = wall;
        if let Some(normal) = surface.filter(|_| wall < RANGE) {
            let point = origin + dir * wall;
            let img = fx.assets.p_dirt.clone();
            fx.burst(&img, point, 4, 3.0, 0.25, Color::srgba(0.3, 0.3, 0.32, 0.8), 0.35);
            spawn_decal(&mut fx, &mut world.decals, point, normal);
        }
    }
    let end = origin + dir * end;

    let muzzle = muzzle_t.translation();
    spawn_tracer(&mut fx, muzzle, end, pierce > 1);
    let img = fx.assets.p_muzzle.clone();
    fx.particle(
        &img,
        muzzle + dir * 0.15,
        Particle {
            max_life: 0.05,
            size: (0.55, 0.7),
            color: Color::srgb(5.0, 3.4, 1.4),
            ..default()
        },
    );
    fx.flash(muzzle + dir * 0.3, Color::srgb(1.0, 0.75, 0.4), 8_000.0, 0.05);
    rig.pitch += 0.0035;
    rig.yaw += rand_range(-0.0015, 0.0015);
}

fn spawn_decal(fx: &mut Fx, decals: &mut Decals, at: Vec3, normal: Vec3) {
    let n = normal.normalize_or(Vec3::Y);
    let img = fx.assets.p_hole.clone();
    let mat = fx.cached(&img, DECAL_COLOR, Look::Decal);
    // quad normal is +Z; turn it to the surface normal, random spin around it
    let rot = Quat::from_rotation_arc(Vec3::Z, n) * Quat::from_rotation_z(rand_range(0.0, std::f32::consts::TAU));
    let size = rand_range(0.16, 0.24);
    let quad = fx.assets.quad.clone();
    let e = fx
        .commands
        .spawn((
            RunEntity,
            Decal { life: DECAL_LIFE, step: ALPHA_STEPS },
            Mesh3d(quad),
            MeshMaterial3d(mat),
            Transform::from_translation(at + n * 0.012).with_rotation(rot).with_scale(Vec3::splat(size)),
            bevy::light::NotShadowCaster,
        ))
        .id();
    decals.0.push_back(e);
    while decals.0.len() > MAX_DECALS {
        if let Some(old) = decals.0.pop_front() {
            fx.commands.entity(old).try_despawn();
        }
    }
}

fn fade_decals(
    mut commands: Commands,
    time: Res<Time>,
    mut decals: ResMut<Decals>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut cache: ResMut<MatCache>,
    assets: Res<crate::assets::GameAssets>,
    mut q: Query<(Entity, &mut Decal, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    for (e, mut d, mut mat) in &mut q {
        d.life -= time.delta_secs();
        if d.life <= 0.0 {
            commands.entity(e).despawn();
            decals.0.retain(|x| *x != e);
        } else if d.life < 3.0 {
            // fade over the last 3 s in steps of shared materials
            let step = ((d.life / 3.0) * ALPHA_STEPS as f32).ceil().clamp(1.0, ALPHA_STEPS as f32) as u8;
            if step != d.step {
                d.step = step;
                mat.0 = cache.get(&mut materials, &assets.p_hole, faded(DECAL_COLOR, step), Look::Decal);
            }
        }
    }
}

fn start_reload(gun: &mut Gun, fx: &mut Fx) {
    gun.reloading = RELOAD_TIME;
    gun.burst_left = 0;
    let s = fx.assets.sfx.br_reload.clone();
    fx.sound(&s, 0.7);
}

fn spawn_tracer(fx: &mut Fx, from: Vec3, to: Vec3, piercing: bool) {
    let len = from.distance(to);
    if len < 0.5 {
        return;
    }
    let img = fx.assets.p_trace.clone();
    // piercing rounds leave a cold blue-white streak
    let color = if piercing { Color::srgb(1.6, 2.4, 3.6) } else { Color::srgb(3.0, 2.0, 0.7) };
    let mat = fx.unlit(&img, color);
    let quad = fx.assets.quad.clone();
    let mid = (from + to) / 2.0;
    let dir = (to - from) / len;
    // the trace texture runs vertically; align its Y with the shot, face it roughly at the camera
    let rot = Quat::from_rotation_arc(Vec3::Y, dir);
    fx.commands.spawn((
        RunEntity,
        Tracer { life: 0.07 },
        Mesh3d(quad),
        MeshMaterial3d(mat),
        Transform::from_translation(mid)
            .with_rotation(rot)
            .with_scale(Vec3::new(0.09, len, 1.0)),
        bevy::light::NotShadowCaster,
    ));
}

fn fade_tracers(
    mut commands: Commands,
    time: Res<Time>,
    cam: Query<&GlobalTransform, With<GameCamera>>,
    mut q: Query<(Entity, &mut Tracer, &mut Transform), Without<Billboard>>,
) {
    let cam_pos = cam.single().map(|c| c.translation()).unwrap_or_default();
    for (e, mut tr, mut t) in &mut q {
        tr.life -= time.delta_secs();
        if tr.life <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        // axial billboard: spin around the shot axis so the flat quad faces the camera
        let axis = t.rotation * Vec3::Y;
        let to_cam = (cam_pos - t.translation).normalize_or_zero();
        let side = axis.cross(to_cam).normalize_or_zero();
        if side != Vec3::ZERO {
            let normal = side.cross(axis);
            t.rotation = Quat::from_mat3(&Mat3::from_cols(side, axis, normal));
        }
        t.scale.x *= 0.8;
    }
}

pub fn ray_sphere(o: Vec3, d: Vec3, c: Vec3, r: f32) -> Option<f32> {
    let oc = o - c;
    let b = oc.dot(d);
    let cc = oc.length_squared() - r * r;
    let disc = b * b - cc;
    if disc < 0.0 {
        return None;
    }
    let t = -b - disc.sqrt();
    (t > 0.0).then_some(t)
}

/// Ray vs. upright cylinder standing on `base`.
pub fn ray_cylinder(o: Vec3, d: Vec3, base: Vec3, r: f32, h: f32) -> Option<f32> {
    let ox = o.x - base.x;
    let oz = o.z - base.z;
    let a = d.x * d.x + d.z * d.z;
    if a < 1e-8 {
        return None;
    }
    let b = ox * d.x + oz * d.z;
    let c = ox * ox + oz * oz - r * r;
    let disc = b * b - a * c;
    if disc < 0.0 {
        return None;
    }
    let sq = disc.sqrt();
    for t in [(-b - sq) / a, (-b + sq) / a] {
        if t > 0.0 {
            let y = o.y + d.y * t - base.y;
            if (0.0..=h).contains(&y) {
                return Some(t);
            }
        }
    }
    None
}
