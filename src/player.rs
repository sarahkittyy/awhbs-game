use bevy::asset::RenderAssetUsages;
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

use crate::arena::{GameCamera, ARENA_SCALE};
use crate::balance::{
    ARENA_RADIUS, GRAVITY, INVULN, JUMP_VEL, KNOCKBACK, PLAYER_H, PLAYER_W, SPRINT_MULT, START_HP, WALK_SPEED,
};
use crate::assets::{play, GameAssets};
use crate::fx::Shard;
use crate::pickup::Buffs;
use crate::{flat, rand_range, GameState, RunClock, RunEntity, Settings, RunBegin};

/// Where every run begins, looking toward +Z at the white space rug.
pub const START_POS: Vec3 = Vec3::new(0.0, 0.0, -6.0 * ARENA_SCALE);
const FOV: f32 = 70.0;
const SPRINT_LEAN: f32 = 0.42;
const ZOOM_FOV: f32 = 30.0;

#[derive(Component)]
pub struct Player {
    pub hp: i32,
    pub max_hp: i32,
    pub vel: Vec3,
    pub grounded: bool,
    pub invuln: f32,
    pub sprinting: bool,
    /// forward lean while sprinting, radians
    lean: f32,
    step: f32,
}

impl Player {
}

#[derive(Component)]
struct PlayerBody;

#[derive(Component)]
struct PlayerCard;


#[derive(Resource, Default)]
pub struct CamRig {
    pub yaw: f32,
    pub pitch: f32,
    pub zoom: f32,
    pub zoomed: bool,
    pub orbit: f32,
}

#[derive(Resource, Default)]
pub struct DeathSpot(pub Vec3);

/// Something hurt the player; `from` is used for knockback.
#[derive(Message)]
pub struct PlayerHit {
    pub from: Vec3,
}

/// Brief red flash on the HUD, read by the UI.
#[derive(Resource, Default)]
pub struct HurtFlash(pub f32);

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CamRig>()
            .init_resource::<DeathSpot>()
            .init_resource::<HurtFlash>()
            .add_message::<PlayerHit>()
            .add_systems(Update, spawn_player)
            .add_systems(
                Update,
                (movement, lean_visuals, take_hits, blink)
                    .chain()
                    .run_if(in_state(GameState::Playing)),
            )
            .add_systems(
                PostUpdate,
                (
                    camera_play.run_if(in_state(GameState::Playing)),
                    camera_orbit.run_if(in_state(GameState::Title)),
                    camera_dead.run_if(in_state(GameState::Dead)),
                )
                    .before(TransformSystems::Propagate),
            )
            .add_systems(OnEnter(GameState::Playing), grab_cursor)
            .add_systems(OnExit(GameState::Playing), release_cursor)
            .add_systems(Update, slowmo_recover.run_if(in_state(GameState::Dead)));
    }
}

fn spawn_player(
    mut msgs: MessageReader<RunBegin>,
    mut commands: Commands,
    assets: Res<GameAssets>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut rig: ResMut<CamRig>,
) {
    if msgs.read().count() == 0 {
        return;
    }
    *rig = CamRig {
        yaw: std::f32::consts::PI,
        pitch: -0.12,
        ..default()
    };
    let card = materials.add(StandardMaterial {
        base_color_texture: Some(assets.awhbs.clone()),
        emissive: LinearRgba::rgb(0.45, 0.45, 0.45),
        emissive_texture: Some(assets.awhbs.clone()),
        perceptual_roughness: 0.9,
        reflectance: 0.1,
        ..default()
    });
    let quad = assets.quad.clone();
    let card_t = |rot: f32| {
        Transform::from_xyz(0.0, PLAYER_H / 2.0, 0.0)
            .with_rotation(Quat::from_rotation_y(rot))
            .with_scale(Vec3::new(PLAYER_W, PLAYER_H, 1.0))
    };
    // start in front of the white space rug, facing it
    let start = START_POS;
    commands
        .spawn((
            RunEntity,
            Player {
                hp: START_HP,
                max_hp: START_HP,
                vel: Vec3::ZERO,
                grounded: true,
                invuln: 0.0,
                sprinting: false,
                lean: 0.0,
                step: 0.0,
            },
            Transform::from_translation(start),
            Visibility::default(),
        ))
        .with_children(|p| {
            p.spawn((PlayerBody, Transform::default(), Visibility::default()))
                .with_children(|b| {
                    // two single-sided cards so neither side shows mirrored
                    b.spawn((PlayerCard, Mesh3d(quad.clone()), MeshMaterial3d(card.clone()), card_t(0.0)));
                    b.spawn((PlayerCard, Mesh3d(quad.clone()), MeshMaterial3d(card.clone()), card_t(std::f32::consts::PI)));
                });
            crate::gun_model::spawn_gun(p, &assets);
        });
}

fn movement(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    rig: Res<CamRig>,
    buffs: Res<Buffs>,
    assets: Res<GameAssets>,
    mut commands: Commands,
    mut q: Query<(&mut Player, &mut Transform)>,
) {
    let Ok((mut p, mut t)) = q.single_mut() else { return };
    let dt = time.delta_secs();
    t.rotation = Quat::from_rotation_y(rig.yaw);

    let w = keys.pressed(KeyCode::KeyW);
    let a = keys.pressed(KeyCode::KeyA);
    let s = keys.pressed(KeyCode::KeyS);
    let d = keys.pressed(KeyCode::KeyD);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    // sprint is forward-only: any strafe or backpedal cancels it, but it resumes once released
    p.sprinting = shift && w && !a && !d && !s;

    let target_lean = if p.sprinting { SPRINT_LEAN } else { 0.0 };
    p.lean += (target_lean - p.lean) * (1.0 - (-12.0 * dt).exp());

    let mut input = Vec3::ZERO;
    if w {
        input.z -= 1.0;
    }
    if s {
        input.z += 1.0;
    }
    if a {
        input.x -= 1.0;
    }
    if d {
        input.x += 1.0;
    }
    let mut speed = WALK_SPEED * buffs.speed_mult();
    if p.sprinting {
        speed *= SPRINT_MULT;
    }
    let wish = t.rotation * input.normalize_or_zero() * speed;
    let horiz = flat(p.vel);
    let accel = if p.grounded { 14.0 } else { 4.0 };
    let new_h = horiz + (wish - horiz) * (1.0 - (-accel * dt).exp());
    p.vel.x = new_h.x;
    p.vel.z = new_h.z;

    if keys.just_pressed(KeyCode::Space) && p.grounded {
        p.vel.y = JUMP_VEL;
        p.grounded = false;
        play(&mut commands, &assets.sfx.jump, 0.35);
    }
    p.vel.y -= GRAVITY * dt;
    let vel = p.vel;
    t.translation += vel * dt;
    if t.translation.y <= 0.0 {
        if !p.grounded && vel.y < -4.0 {
            play(&mut commands, &assets.sfx.land, 0.3);
        }
        t.translation.y = 0.0;
        p.vel.y = 0.0;
        p.grounded = true;
    }
    let h = flat(t.translation);
    if h.length() > ARENA_RADIUS {
        let c = h.normalize() * ARENA_RADIUS;
        t.translation.x = c.x;
        t.translation.z = c.z;
    }

    if p.grounded && new_h.length() > 1.0 {
        p.step -= dt * new_h.length() / 2.4;
        if p.step <= 0.0 {
            p.step = 1.0;
            let i = fastrand::usize(..assets.sfx.footsteps.len());
            play(&mut commands, &assets.sfx.footsteps[i], 0.22);
        }
    }
    if p.invuln > 0.0 {
        p.invuln -= dt;
    }
}

fn lean_visuals(players: Query<&Player>, mut body: Query<&mut Transform, With<PlayerBody>>) {
    let Ok(p) = players.single() else { return };
    for mut t in &mut body {
        // tip forward (toward -Z) around her feet
        t.rotation = Quat::from_rotation_x(-p.lean);
    }
}

fn blink(time: Res<Time>, players: Query<(&Player, &Children)>, mut vis: Query<&mut Visibility, With<PlayerBody>>) {
    let Ok((p, children)) = players.single() else { return };
    for c in children.iter() {
        if let Ok(mut v) = vis.get_mut(c) {
            let on = p.invuln <= 0.0 || (time.elapsed_secs() * 14.0) as i32 % 2 == 0;
            *v = if on { Visibility::Inherited } else { Visibility::Hidden };
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn take_hits(
    mut hits: MessageReader<PlayerHit>,
    mut commands: Commands,
    assets: Res<GameAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut q: Query<(&mut Player, &Transform, &mut Visibility)>,
    mut next: ResMut<NextState<GameState>>,
    mut death: ResMut<DeathSpot>,
    mut flash: ResMut<HurtFlash>,
    mut time: ResMut<Time<Virtual>>,
    mut buffs: ResMut<Buffs>,
) {
    let Ok((mut p, t, mut vis)) = q.single_mut() else {
        hits.clear();
        return;
    };
    for hit in hits.read() {
        if p.invuln > 0.0 || p.hp <= 0 {
            continue;
        }
        p.invuln = INVULN;
        let away = flat(t.translation - hit.from).normalize_or(Vec3::Z);
        p.vel += away * KNOCKBACK + Vec3::Y * 3.0;
        p.grounded = false;
        if buffs.shield_up() > 0 {
            buffs.shield_points -= 1;
            if buffs.shield_points <= 0 {
                buffs.shield.remaining = 0.0;
            }
            play(&mut commands, &assets.sfx.shield_hit, 0.8);
            continue;
        }
        p.hp -= 1;
        flash.0 = 1.0;
        play(&mut commands, &assets.sfx.player_hurt, 0.8);
        if p.hp <= 0 {
            *vis = Visibility::Hidden;
            death.0 = t.translation;
            shatter(&mut commands, &assets, &mut meshes, &mut materials, t);
            play(&mut commands, &assets.sfx.glass_shatter, 1.0);
            time.set_relative_speed(0.35);
            next.set(GameState::Dead);
            break;
        }
    }
}

/// Breaks the card into jittered triangles that keep their slice of her texture.
fn shatter(
    commands: &mut Commands,
    assets: &GameAssets,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    t: &Transform,
) {
    let mat = materials.add(StandardMaterial {
        base_color_texture: Some(assets.awhbs.clone()),
        emissive: LinearRgba::rgb(0.45, 0.45, 0.45),
        emissive_texture: Some(assets.awhbs.clone()),
        cull_mode: None,
        double_sided: true,
        reflectance: 0.6,
        perceptual_roughness: 0.15,
        ..default()
    });
    const N: usize = 6;
    // grid of uv points, interior ones jittered so the cracks look irregular
    let mut grid = [[Vec2::ZERO; N + 1]; N + 1];
    for (j, row) in grid.iter_mut().enumerate() {
        for (i, g) in row.iter_mut().enumerate() {
            let mut uv = Vec2::new(i as f32 / N as f32, j as f32 / N as f32);
            if i > 0 && i < N && j > 0 && j < N {
                uv += Vec2::new(rand_range(-0.35, 0.35), rand_range(-0.35, 0.35)) / N as f32;
            }
            *g = uv;
        }
    }
    let h = PLAYER_H;
    let to_world = |uv: Vec2| {
        // uv (0,0) is the top-left of the image; the back card faces +Z
        let local = Vec3::new((uv.x - 0.5) * PLAYER_W, (1.0 - uv.y) * h, 0.0);
        t.transform_point(local)
    };
    let center = to_world(Vec2::splat(0.5));
    for j in 0..N {
        for i in 0..N {
            let (a, b, c, d) = (grid[j][i], grid[j][i + 1], grid[j + 1][i + 1], grid[j + 1][i]);
            let tris = if fastrand::bool() { [[a, b, c], [a, c, d]] } else { [[a, b, d], [b, c, d]] };
            for tri in tris {
                let world: Vec<Vec3> = tri.iter().map(|&uv| to_world(uv)).collect();
                let centroid = (world[0] + world[1] + world[2]) / 3.0;
                let positions: Vec<[f32; 3]> = world.iter().map(|w| (*w - centroid).to_array()).collect();
                let normal = (world[1] - world[0]).cross(world[2] - world[0]).normalize_or(Vec3::Z);
                let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
                    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
                    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![normal.to_array(); 3])
                    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, tri.iter().map(|uv| uv.to_array()).collect::<Vec<_>>())
                    .with_inserted_indices(Indices::U16(vec![0, 1, 2]));
                let out = (centroid - center).normalize_or(Vec3::Y);
                let vel = out * rand_range(2.5, 6.5)
                    + Vec3::Y * rand_range(2.0, 6.0)
                    + (t.rotation * Vec3::Z) * rand_range(-2.0, 2.5)
                    + Vec3::new(rand_range(-1.0, 1.0), 0.0, rand_range(-1.0, 1.0));
                commands.spawn((
                    RunEntity,
                    Mesh3d(meshes.add(mesh)),
                    MeshMaterial3d(mat.clone()),
                    Transform::from_translation(centroid),
                    Shard {
                        vel,
                        ang_vel: Vec3::new(rand_range(-9.0, 9.0), rand_range(-9.0, 9.0), rand_range(-9.0, 9.0)),
                        resting: false,
                    },
                ));
            }
        }
    }
}

fn slowmo_recover(real: Res<Time<Real>>, mut time: ResMut<Time<Virtual>>) {
    let s = time.relative_speed();
    if s < 1.0 {
        time.set_relative_speed((s + real.delta_secs() * 0.25).min(1.0));
    }
}

fn camera_play(
    motion: Res<AccumulatedMouseMotion>,
    mouse: Res<ButtonInput<MouseButton>>,
    settings: Res<Settings>,
    time: Res<Time>,
    assets: Res<GameAssets>,
    mut commands: Commands,
    mut rig: ResMut<CamRig>,
    players: Query<&Transform, (With<Player>, Without<GameCamera>)>,
    mut cam: Query<(&mut Transform, &mut Projection), With<GameCamera>>,
) {
    let Ok(pt) = players.single() else { return };
    let Ok((mut ct, mut proj)) = cam.single_mut() else { return };
    let dt = time.delta_secs();

    let zoomed = mouse.pressed(MouseButton::Right);
    if zoomed != rig.zoomed {
        rig.zoomed = zoomed;
        play(&mut commands, if zoomed { &assets.sfx.zoom_in } else { &assets.sfx.zoom_out }, 0.5);
    }
    let zt = if zoomed { 1.0 } else { 0.0 };
    rig.zoom += (zt - rig.zoom) * (1.0 - (-16.0 * dt).exp());

    let fov_scale = 1.0 - rig.zoom * (1.0 - ZOOM_FOV / FOV);
    let sens = 0.0022 * settings.sensitivity * fov_scale;
    rig.yaw -= motion.delta.x * sens;
    rig.pitch = (rig.pitch - motion.delta.y * sens).clamp(-1.25, 1.1);

    let rot = Quat::from_euler(EulerRot::YXZ, rig.yaw, rig.pitch, 0.0);
    let pivot = pt.translation + Vec3::Y * 1.6;
    let offset = Vec3::new(1.15, 0.35, 3.6).lerp(Vec3::new(1.05, 0.3, 2.0), rig.zoom);
    ct.translation = pivot + rot * offset;
    ct.rotation = rot;
    #[cfg(debug_assertions)]
    if let Ok(side) = std::env::var("AWHBS_CAMSIDE") {
        // debug: look at her from the right side to inspect the gun
        let d: f32 = side.parse().unwrap_or(2.6);
        let side_pos = pt.translation + rot * Vec3::new(d, 1.1, -0.4);
        *ct = Transform::from_translation(side_pos).looking_at(pt.translation + rot * Vec3::new(0.6, 0.9, -0.4), Vec3::Y);
    }
    if let Projection::Perspective(pp) = proj.as_mut() {
        pp.fov = (FOV + (ZOOM_FOV - FOV) * rig.zoom).to_radians();
    }
}

fn camera_orbit(time: Res<Time<Real>>, mut rig: ResMut<CamRig>, mut cam: Query<(&mut Transform, &mut Projection), With<GameCamera>>) {
    let Ok((mut ct, mut proj)) = cam.single_mut() else { return };
    rig.orbit += time.delta_secs() * 0.07;
    let r = 11.0;
    let pos = Vec3::new(rig.orbit.cos() * r, 3.2, rig.orbit.sin() * r);
    *ct = Transform::from_translation(pos).looking_at(Vec3::new(0.0, 1.2, 0.0), Vec3::Y);
    if let Projection::Perspective(pp) = proj.as_mut() {
        pp.fov = FOV.to_radians();
    }
}

fn camera_dead(
    time: Res<Time<Real>>,
    death: Res<DeathSpot>,
    mut rig: ResMut<CamRig>,
    mut cam: Query<(&mut Transform, &mut Projection), With<GameCamera>>,
) {
    let Ok((mut ct, mut proj)) = cam.single_mut() else { return };
    let dt = time.delta_secs();
    rig.orbit += dt * 0.12;
    let target = death.0 + Vec3::Y * 0.4;
    let angle = rig.yaw + rig.orbit;
    let desired = target + Vec3::new(angle.sin() * 4.5, 2.6, angle.cos() * 4.5);
    let k = 1.0 - (-2.5 * dt).exp();
    ct.translation = ct.translation.lerp(desired, k);
    let look = Transform::from_translation(ct.translation).looking_at(target, Vec3::Y);
    ct.rotation = ct.rotation.slerp(look.rotation, k);
    if let Projection::Perspective(pp) = proj.as_mut() {
        pp.fov += (FOV.to_radians() - pp.fov) * k;
    }
}

fn grab_cursor(mut cursor: Query<&mut CursorOptions, With<PrimaryWindow>>) {
    if let Ok(mut c) = cursor.single_mut() {
        c.grab_mode = CursorGrabMode::Locked;
        c.visible = false;
    }
}

fn release_cursor(mut cursor: Query<&mut CursorOptions, With<PrimaryWindow>>, mut rig: ResMut<CamRig>) {
    if let Ok(mut c) = cursor.single_mut() {
        c.grab_mode = CursorGrabMode::None;
        c.visible = true;
    }
    rig.orbit = 0.0;
    rig.zoomed = false;
}

/// Seconds survived, formatted as mm:ss.
pub fn format_time(clock: &RunClock) -> String {
    let s = clock.0.max(0.0) as u32;
    format!("{:02}:{:02}", s / 60, s % 60)
}
