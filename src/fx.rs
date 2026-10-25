use bevy::audio::Volume;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use crate::arena::GameCamera;
use crate::balance::GRAVITY;
use crate::assets::GameAssets;
use crate::{rand_range, GameState, RunEntity};

pub struct FxPlugin;

impl Plugin for FxPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (update_particles, update_flashes, update_shards, animate_frames)
                .run_if(not(in_state(GameState::Paused))),
        )
        .add_systems(PostUpdate, billboard.before(TransformSystems::Propagate));
    }
}

/// Rotates a quad (normal +Z) to face the camera.
#[derive(Component, Default)]
pub struct Billboard {
    /// Only rotate around Y (sprites standing on the floor).
    pub upright: bool,
    /// Extra in-plane rotation, e.g. a rolling ball.
    pub spin: f32,
}

#[derive(Component, Clone)]
pub struct Particle {
    pub vel: Vec3,
    pub gravity: f32,
    pub drag: f32,
    pub life: f32,
    pub max_life: f32,
    pub size: (f32, f32),
    pub alpha: f32,
    pub color: Color,
    pub spin_speed: f32,
}

impl Default for Particle {
    fn default() -> Self {
        Self {
            vel: Vec3::ZERO,
            gravity: 0.0,
            drag: 0.0,
            life: 0.0,
            max_life: 0.5,
            size: (1.0, 1.0),
            alpha: 1.0,
            color: Color::WHITE,
            spin_speed: 0.0,
        }
    }
}

#[derive(Component)]
pub struct LightFlash {
    pub life: f32,
    pub max_life: f32,
    pub intensity: f32,
}

#[derive(Component)]
pub struct Shard {
    pub vel: Vec3,
    pub ang_vel: Vec3,
    pub resting: bool,
}

/// Flipbook animation that swaps a material's texture.
#[derive(Component)]
pub struct Frames {
    pub frames: Vec<Handle<Image>>,
    pub fps: f32,
    pub t: f32,
    pub looping: bool,
    pub despawn_at_end: bool,
}

#[derive(SystemParam)]
pub struct Fx<'w, 's> {
    pub commands: Commands<'w, 's>,
    pub materials: ResMut<'w, Assets<StandardMaterial>>,
    pub assets: Res<'w, GameAssets>,
}

impl Fx<'_, '_> {
    pub fn unlit(&mut self, image: &Handle<Image>, color: Color) -> Handle<StandardMaterial> {
        self.materials.add(StandardMaterial {
            base_color: color,
            base_color_texture: Some(image.clone()),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            cull_mode: None,
            fog_enabled: true,
            ..default()
        })
    }

    pub fn sound(&mut self, sound: &Handle<AudioSource>, volume: f32) {
        self.sound_speed(sound, volume, rand_range(0.94, 1.06));
    }

    pub fn sound_speed(&mut self, sound: &Handle<AudioSource>, volume: f32, speed: f32) {
        self.commands.spawn((
            AudioPlayer::new(sound.clone()),
            PlaybackSettings::DESPAWN
                .with_volume(Volume::Linear(volume))
                .with_speed(speed),
        ));
    }

    /// A camera-facing quad that moves, grows and fades out.
    pub fn particle(&mut self, image: &Handle<Image>, pos: Vec3, p: Particle) -> Entity {
        let mat = self.unlit(image, p.color.with_alpha(p.color.alpha() * p.alpha));
        let quad = self.assets.quad.clone();
        self.commands
            .spawn((
                RunEntity,
                Mesh3d(quad),
                MeshMaterial3d(mat),
                Transform::from_translation(pos).with_scale(Vec3::splat(p.size.0)),
                Billboard {
                    upright: false,
                    spin: rand_range(0.0, std::f32::consts::TAU),
                },
                bevy::light::NotShadowCaster,
                p,
            ))
            .id()
    }

    /// Flat decal lying on the floor (scorch marks, glows).
    pub fn decal(&mut self, image: &Handle<Image>, pos: Vec3, size: f32, color: Color, life: f32) {
        let mat = self.unlit(image, color);
        let quad = self.assets.quad.clone();
        self.commands.spawn((
            RunEntity,
            Mesh3d(quad),
            MeshMaterial3d(mat),
            Transform::from_translation(Vec3::new(pos.x, 0.03 + fastrand::f32() * 0.01, pos.z))
                .with_rotation(
                    Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2) * Quat::from_rotation_z(rand_range(0.0, 6.28)),
                )
                .with_scale(Vec3::splat(size)),
            bevy::light::NotShadowCaster,
            Particle {
                max_life: life,
                size: (size, size),
                color,
                ..default()
            },
        ));
    }

    pub fn flash(&mut self, pos: Vec3, color: Color, intensity: f32, life: f32) {
        self.commands.spawn((
            RunEntity,
            PointLight {
                color,
                intensity,
                range: 12.0,
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::from_translation(pos),
            LightFlash {
                life: 0.0,
                max_life: life,
                intensity,
            },
        ));
    }

    /// Radial spray of small particles.
    pub fn burst(&mut self, image: &Handle<Image>, pos: Vec3, count: usize, speed: f32, size: f32, color: Color, life: f32) {
        for _ in 0..count {
            let dir = Vec3::new(rand_range(-1.0, 1.0), rand_range(-0.2, 1.0), rand_range(-1.0, 1.0)).normalize_or_zero();
            self.particle(
                image,
                pos,
                Particle {
                    vel: dir * speed * rand_range(0.5, 1.0),
                    gravity: 9.0,
                    drag: 2.0,
                    max_life: life * rand_range(0.7, 1.2),
                    size: (size, size * 0.3),
                    color,
                    spin_speed: rand_range(-6.0, 6.0),
                    ..default()
                },
            );
        }
    }

    pub fn explosion(&mut self, pos: Vec3, radius: f32) {
        let a = self.assets.clone_handles();
        self.sound(&self.assets.sfx.explosion.clone(), 0.9);
        self.flash(pos + Vec3::Y, Color::srgb(1.0, 0.6, 0.25), 400_000.0, 0.35);
        for i in 0..6 {
            let off = Vec3::new(rand_range(-0.6, 0.6), rand_range(0.2, 1.0), rand_range(-0.6, 0.6));
            self.particle(
                if i % 2 == 0 { &a.0 } else { &a.1 },
                pos + off,
                Particle {
                    vel: off * 3.0,
                    max_life: 0.45,
                    size: (radius * 0.6, radius * 1.4),
                    color: Color::srgb(4.0, 1.8, 0.5),
                    spin_speed: 2.0,
                    ..default()
                },
            );
        }
        for _ in 0..7 {
            let off = Vec3::new(rand_range(-1.0, 1.0), rand_range(0.3, 1.2), rand_range(-1.0, 1.0));
            self.particle(
                &a.2,
                pos + off,
                Particle {
                    vel: off * 1.6 + Vec3::Y * 1.5,
                    drag: 1.0,
                    max_life: rand_range(1.2, 2.0),
                    size: (radius * 0.6, radius * 1.3),
                    alpha: 0.75,
                    color: Color::srgb(0.25, 0.24, 0.24),
                    spin_speed: rand_range(-1.0, 1.0),
                    ..default()
                },
            );
        }
        self.burst(&a.3, pos + Vec3::Y * 0.5, 14, 14.0, 0.5, Color::srgb(3.0, 1.6, 0.4), 0.5);
        self.decal(&a.4, pos, radius * 1.4, Color::srgba(0.1, 0.08, 0.08, 0.85), 8.0);
    }
}

impl GameAssets {
    fn clone_handles(&self) -> (Handle<Image>, Handle<Image>, Handle<Image>, Handle<Image>, Handle<Image>) {
        (
            self.p_flame.clone(),
            self.p_fire.clone(),
            self.p_smoke.clone(),
            self.p_spark.clone(),
            self.p_scorch.clone(),
        )
    }
}

fn billboard(
    cam: Query<&GlobalTransform, With<GameCamera>>,
    mut q: Query<(&mut Transform, &GlobalTransform, &Billboard)>,
) {
    let Ok(cam) = cam.single() else { return };
    let cam_pos = cam.translation();
    for (mut t, gt, b) in &mut q {
        let mut dir = gt.translation() - cam_pos;
        if b.upright {
            dir.y = 0.0;
        }
        if dir.length_squared() < 1e-6 {
            continue;
        }
        t.rotation = Transform::IDENTITY.looking_to(dir, Vec3::Y).rotation * Quat::from_rotation_z(b.spin);
    }
}

fn update_particles(
    mut commands: Commands,
    time: Res<Time>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut q: Query<(Entity, &mut Particle, &mut Transform, &MeshMaterial3d<StandardMaterial>, Option<&mut Billboard>)>,
) {
    let dt = time.delta_secs();
    for (e, mut p, mut t, mat, bb) in &mut q {
        p.life += dt;
        if p.life >= p.max_life {
            commands.entity(e).despawn();
            continue;
        }
        let k = p.life / p.max_life;
        p.vel.y -= p.gravity * dt;
        let drag = (1.0 - p.drag * dt).max(0.0);
        p.vel *= drag;
        t.translation += p.vel * dt;
        if p.gravity > 0.0 && t.translation.y < 0.05 {
            t.translation.y = 0.05;
            p.vel.y = p.vel.y.abs() * 0.3;
            p.vel *= 0.6;
        }
        let s = p.size.0 + (p.size.1 - p.size.0) * k;
        t.scale = Vec3::splat(s);
        if let Some(mut bb) = bb {
            bb.spin += p.spin_speed * dt;
        }
        if let Some(mut m) = materials.get_mut(&mat.0) {
            // hold full opacity for the first half, then fade
            let fade = if k < 0.5 { 1.0 } else { 1.0 - (k - 0.5) * 2.0 };
            m.base_color = p.color.with_alpha(p.color.alpha() * p.alpha * fade);
        }
    }
}

fn update_flashes(mut commands: Commands, time: Res<Time>, mut q: Query<(Entity, &mut LightFlash, &mut PointLight)>) {
    for (e, mut f, mut light) in &mut q {
        f.life += time.delta_secs();
        if f.life >= f.max_life {
            commands.entity(e).despawn();
        } else {
            light.intensity = f.intensity * (1.0 - f.life / f.max_life).powi(2);
        }
    }
}

fn update_shards(time: Res<Time>, mut q: Query<(&mut Shard, &mut Transform)>) {
    let dt = time.delta_secs();
    for (mut s, mut t) in &mut q {
        if s.resting {
            continue;
        }
        s.vel.y -= GRAVITY * 0.8 * dt;
        t.translation += s.vel * dt;
        let av = s.ang_vel * dt;
        t.rotation = Quat::from_euler(EulerRot::XYZ, av.x, av.y, av.z) * t.rotation;
        if t.translation.y < 0.02 {
            t.translation.y = 0.02;
            if s.vel.y.abs() < 1.2 {
                // settle flat on the floor, keeping only the spin around Y
                let (yaw, _, _) = t.rotation.to_euler(EulerRot::YXZ);
                let face = if fastrand::bool() { 1.0 } else { -1.0 };
                t.rotation = Quat::from_rotation_y(yaw) * Quat::from_rotation_x(face * std::f32::consts::FRAC_PI_2);
                s.resting = true;
            } else {
                s.vel.y = -s.vel.y * 0.35;
                s.vel.x *= 0.55;
                s.vel.z *= 0.55;
                s.ang_vel *= 0.5;
            }
        }
    }
}

fn animate_frames(
    mut commands: Commands,
    time: Res<Time>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut q: Query<(Entity, &mut Frames, &MeshMaterial3d<StandardMaterial>)>,
) {
    for (e, mut f, mat) in &mut q {
        f.t += time.delta_secs();
        let mut i = (f.t * f.fps) as usize;
        if i >= f.frames.len() {
            if f.despawn_at_end {
                commands.entity(e).despawn();
                continue;
            }
            i = if f.looping { i % f.frames.len() } else { f.frames.len() - 1 };
        }
        if let Some(mut m) = materials.get_mut(&mat.0) {
            if m.base_color_texture.as_ref() != Some(&f.frames[i]) {
                m.base_color_texture = Some(f.frames[i].clone());
            }
        }
    }
}
