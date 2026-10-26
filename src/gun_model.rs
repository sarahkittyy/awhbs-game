//! The 3D battle rifle (Vuthakral's Halo 3 BR55) on awhbs's right side, with
//! procedural animation driving the model's own moving parts: recoil, trigger,
//! ejection port, magazine swap and charging handle on reload, plus sprint, zoom
//! and walk poses.

use bevy::gltf::GltfMeshName;
use bevy::prelude::*;
use bevy::world_serialization::WorldInstanceReady;

use crate::assets::GameAssets;
use crate::balance::RELOAD_TIME;
use crate::gun::Gun;
use crate::player::{CamRig, Player};
use crate::GameState;

/// The scene is Y-up and ~0.88 m long with the barrel along -X; scale it to 1.3 m.
const MODEL_SCALE: f32 = 1.3 / 0.88;
/// Barrel tip in the scene's own space.
const MUZZLE: Vec3 = Vec3::new(-0.282, -0.091, 0.0);
/// Pistol grip in the scene's own space, placed at the hold point.
const GRIP: Vec3 = Vec3::new(0.246, -0.204, 0.0);
/// Where the pistol grip sits relative to the player's feet, facing -Z.
const HOLD: Vec3 = Vec3::new(0.95, 0.95, -0.2);

/// Pose root of the gun; procedural offsets are applied here.
#[derive(Component)]
pub struct GunModel;

/// Empty child at the barrel tip, for muzzle flash and tracers.
#[derive(Component)]
pub struct GunMuzzle;

#[derive(Component)]
struct GunScene;

#[derive(Clone, Copy)]
struct Part {
    entity: Entity,
    rest: Transform,
}

/// The model's animated joints, found once the scene has spawned.
#[derive(Component)]
struct GunParts {
    mag: Part,
    ophandle: Part,
    eject: Part,
    trigger: Part,
}

#[derive(Component, Default)]
pub struct GunAnim {
    kick: f32,
    kick_vel: f32,
    eject: f32,
    sprint: f32,
    zoom: f32,
    bob: f32,
    shots_seen: u32,
}

pub struct GunModelPlugin;

impl Plugin for GunModelPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            animate
                .run_if(in_state(GameState::Playing).or_else(in_state(GameState::Paused)))
                .before(TransformSystems::Propagate),
        );
    }
}

/// Spawns the rifle as a child of the player root.
pub fn spawn_gun(parent: &mut ChildSpawnerCommands, assets: &GameAssets) {
    // barrel from -X to forward (-Z), grip at the pose root
    let to_world = Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2);
    let offset = -(to_world * GRIP) * MODEL_SCALE;
    parent
        .spawn((GunModel, GunAnim::default(), Transform::from_translation(HOLD), Visibility::default()))
        .with_children(|g| {
            g.spawn((
                GunScene,
                WorldAssetRoot(assets.gun_scene.clone()),
                Transform::from_translation(offset)
                    .with_rotation(to_world)
                    .with_scale(Vec3::splat(MODEL_SCALE)),
            ))
            .with_child((GunMuzzle, Transform::from_translation(MUZZLE)))
            .observe(find_parts);
        });
}

fn find_parts(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    children: Query<&Children>,
    parents: Query<&ChildOf>,
    names: Query<(&Name, &Transform)>,
    meshes: Query<&GltfMeshName>,
    models: Query<(), With<GunModel>>,
) {
    let found = |n: &str| {
        children.iter_descendants(ready.entity).find_map(|e| {
            let (name, t) = names.get(e).ok()?;
            (name.as_str() == n).then_some(Part { entity: e, rest: *t })
        })
    };
    let (Some(mag), Some(ophandle), Some(eject), Some(trigger)) =
        (found("magazine_01"), found("ophandle_02"), found("eject_04"), found("trigger_05"))
    else {
        warn!("battle rifle joints not found");
        return;
    };
    // the foregrip is the model author's addition; the reference BR has none
    for e in children.iter_descendants(ready.entity) {
        if meshes.get(e).is_ok_and(|m| m.0.contains("Foregrip")) {
            commands.entity(e).insert(Visibility::Hidden);
        }
    }
    if let Some(model) = parents.iter_ancestors(ready.entity).find(|e| models.contains(*e)) {
        commands.entity(model).insert(GunParts { mag, ophandle, eject, trigger });
    }
}

fn smooth(cur: &mut f32, target: f32, rate: f32, dt: f32) {
    *cur += (target - *cur) * (1.0 - (-rate * dt).exp());
}

/// 0..1 ramp of `t` across `[a, b]`, eased.
fn ramp(t: f32, a: f32, b: f32) -> f32 {
    let k = ((t - a) / (b - a)).clamp(0.0, 1.0);
    k * k * (3.0 - 2.0 * k)
}

#[allow(clippy::type_complexity)]
fn animate(
    time: Res<Time>,
    gun: Res<Gun>,
    rig: Res<CamRig>,
    mouse: Res<ButtonInput<MouseButton>>,
    players: Query<&Player>,
    mut models: Query<(&mut Transform, &mut GunAnim, Option<&GunParts>), With<GunModel>>,
    mut joints: Query<&mut Transform, Without<GunModel>>,
) {
    let Ok(player) = players.single() else { return };
    let Ok((mut t, mut a, parts)) = models.single_mut() else { return };
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }

    // a new round fired: kick the spring and flick the ejection port
    if gun.shots_fired != a.shots_seen {
        let n = gun.shots_fired.wrapping_sub(a.shots_seen).min(3);
        a.shots_seen = gun.shots_fired;
        a.kick_vel += 9.0 * n as f32;
        a.eject = 1.0;
    }
    // damped spring back to rest
    let accel = -260.0 * a.kick - 22.0 * a.kick_vel;
    a.kick_vel += accel * dt;
    a.kick += a.kick_vel * dt;
    a.eject = (a.eject - dt * 18.0).max(0.0);

    smooth(&mut a.sprint, if player.sprinting { 1.0 } else { 0.0 }, 9.0, dt);
    smooth(&mut a.zoom, rig.zoom, 14.0, dt);
    let speed = Vec3::new(player.vel.x, 0.0, player.vel.z).length();
    if player.grounded {
        a.bob += dt * speed * 1.4;
    }
    let bob_amt = (speed / 6.5).min(1.6) * (1.0 - a.zoom * 0.7);

    // reload progress 0..1 (0 when not reloading)
    let r = if gun.reloading > 0.0 { 1.0 - gun.reloading / RELOAD_TIME } else { 0.0 };
    let reloading = gun.reloading > 0.0;
    let tilt = if reloading { ramp(r, 0.0, 0.15) * (1.0 - ramp(r, 0.85, 1.0)) } else { 0.0 };

    // --- whole-gun pose
    let mut pos = HOLD;
    // barrel splayed outward so the camera behind her sees the rifle side-on
    let mut rot = Quat::from_euler(EulerRot::YXZ, -0.45, 0.04, 0.12);
    // sprint: carried at high port, barrel tipped up at her side
    pos += Vec3::new(0.04, -0.06, 0.1) * a.sprint;
    rot = rot * Quat::from_euler(EulerRot::YXZ, -0.1 * a.sprint, 0.75 * a.sprint, 0.2 * a.sprint);
    // zoom: brought up and in toward the sight line
    pos += Vec3::new(-0.05, 0.12, -0.05) * a.zoom;
    // reload: rolled and lifted to show the magazine well
    pos += Vec3::new(-0.08, 0.08, 0.04) * tilt;
    rot = rot * Quat::from_euler(EulerRot::YXZ, 0.25 * tilt, 0.18 * tilt, -0.5 * tilt);
    // recoil
    pos += Vec3::new(0.0, 0.02, 0.09) * a.kick;
    rot = rot * Quat::from_rotation_x(0.12 * a.kick);
    // walk bob
    pos += Vec3::new((a.bob * 0.5).sin() * 0.012, (a.bob).sin().abs() * 0.025, 0.0) * bob_amt;
    t.translation = pos;
    t.rotation = rot;

    let Some(parts) = parts else { return };

    // --- moving parts, between rest and the model's own BoltBackMagOut pose
    let mag_out = Vec3::new(-7.62, 0.002, -2.895) - parts.mag.rest.translation;
    if let Ok(mut m) = joints.get_mut(parts.mag.entity) {
        // out of the well, gone, new one comes up and seats
        let out = ramp(r, 0.12, 0.32);
        let back_in = ramp(r, 0.55, 0.75);
        let k = if reloading { out * (1.0 - back_in) } else { 0.0 };
        let hidden = reloading && r > 0.32 && r < 0.52;
        m.translation = parts.mag.rest.translation + mag_out * (k * 2.4);
        m.scale = if hidden { Vec3::ZERO } else { Vec3::ONE };
    }
    if let Ok(mut h) = joints.get_mut(parts.ophandle.entity) {
        // charge the handle near the end of the reload
        let k = if reloading { ramp(r, 0.78, 0.86) * (1.0 - ramp(r, 0.9, 0.94)) } else { 0.0 };
        let back = Transform::from_translation(Vec3::new(-2.411, 0.251, 5.641))
            .with_rotation(Quat::from_xyzw(-0.341, 0.0, 0.0, 0.94).normalize());
        h.translation = parts.ophandle.rest.translation.lerp(back.translation, k);
        h.rotation = parts.ophandle.rest.rotation.slerp(back.rotation, k);
    }
    if let Ok(mut e) = joints.get_mut(parts.eject.entity) {
        let back = Vec3::new(-7.053, 0.002, 5.648);
        let k = a.eject.max(if reloading { ramp(r, 0.78, 0.86) * (1.0 - ramp(r, 0.9, 0.94)) } else { 0.0 });
        e.translation = parts.eject.rest.translation.lerp(back, k);
    }
    if let Ok(mut tr) = joints.get_mut(parts.trigger.entity) {
        let pulled = mouse.pressed(MouseButton::Left) && !reloading && gun.mag > 0;
        let target = if pulled { 1.0 } else { 0.0 };
        // trigger pivots back around the lateral (model Y) axis
        let cur = tr.rotation.angle_between(parts.trigger.rest.rotation) / 0.35;
        let mut k = cur;
        smooth(&mut k, target, 30.0, dt);
        tr.rotation = parts.trigger.rest.rotation * Quat::from_rotation_y(-0.35 * k);
    }
}

