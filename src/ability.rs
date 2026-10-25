//! One-shot items held in the 1/2/3 slots: blast, blink and shield.

use bevy::prelude::*;

use crate::arena::{ArenaRay, GameCamera};
use crate::balance::{
    self, Powerup, ARENA_RADIUS, BLAST_DAMAGE, BLAST_KNOCK, BLAST_RADIUS, BLINK_INVULN, BLINK_RANGE, BUFF_DURATION,
};
use crate::enemy::{Dying, Enemy, EnemyHit};
use crate::fx::{Fx, Particle};
use crate::pickup::{Buff, Buffs, Inventory};
use crate::player::Player;
use crate::projectile::Projectile;
use crate::{flat, rand_range, GameState, RunBegin, RunClock, RunEntity};

pub const SLOT_KEYS: [KeyCode; 3] = [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3];

/// Blink is aimed while its key is held and fires on release.
#[derive(Resource, Default)]
struct BlinkAim {
    key: Option<KeyCode>,
    indicator: Option<Entity>,
    target: Vec3,
}

#[derive(Component)]
struct BlinkIndicator;

pub struct AbilityPlugin;

impl Plugin for AbilityPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BlinkAim>()
            .add_systems(Update, reset)
            .add_systems(Update, (use_items, aim_blink).chain().run_if(in_state(GameState::Playing)));
    }
}

fn reset(mut msgs: MessageReader<RunBegin>, mut aim: ResMut<BlinkAim>) {
    if msgs.read().count() > 0 {
        *aim = BlinkAim::default();
    }
}

#[allow(clippy::too_many_arguments)]
fn use_items(
    keys: Res<ButtonInput<KeyCode>>,
    clock: Res<RunClock>,
    mut inv: ResMut<Inventory>,
    mut aim: ResMut<BlinkAim>,
    mut buffs: ResMut<Buffs>,
    mut fx: Fx,
    players: Query<(&Transform, &Player)>,
    enemies: Query<(Entity, &Transform), (With<Enemy>, Without<Dying>, Without<Player>)>,
    projectiles: Query<(Entity, &Transform), (With<Projectile>, Without<Player>)>,
    mut hits: MessageWriter<EnemyHit>,
) {
    let Ok((pt, player)) = players.single() else { return };
    let Some(slot) = SLOT_KEYS.iter().position(|k| keys.just_pressed(*k)) else { return };
    let Some(&item) = inv.0.get(slot) else { return };
    match item {
        Powerup::Blast => {
            inv.0.remove(slot);
            let center = pt.translation;
            let damage = BLAST_DAMAGE * balance::hp_mult(clock.0);
            for (e, t) in &enemies {
                let off = t.translation - center;
                let d = flat(off).length();
                if d < BLAST_RADIUS && off.y.abs() < 6.0 {
                    hits.write(EnemyHit {
                        entity: e,
                        point: t.translation + Vec3::Y,
                        dir: flat(off).normalize_or(Vec3::X),
                        damage,
                        knock: BLAST_KNOCK * (1.0 - 0.5 * d / BLAST_RADIUS),
                    });
                }
            }
            for (e, t) in &projectiles {
                if t.translation.distance(center) < BLAST_RADIUS {
                    fx.commands.entity(e).despawn();
                }
            }
            blast_fx(&mut fx, center);
        }
        Powerup::Shield => {
            inv.0.remove(slot);
            let dur = rand_range(BUFF_DURATION.0, BUFF_DURATION.1);
            buffs.shield = Buff {
                remaining: dur,
                duration: dur,
            };
            buffs.shield_points = player.max_hp;
            let s = fx.assets.sfx.shield_up.clone();
            fx.sound(&s, 0.6);
            let img = fx.assets.p_ring.clone();
            fx.particle(
                &img,
                pt.translation + Vec3::Y * 0.9,
                Particle {
                    max_life: 0.5,
                    size: (1.0, 3.2),
                    color: Color::srgb(0.6, 1.2, 2.4),
                    ..default()
                },
            );
        }
        Powerup::Blink => {
            if aim.key.is_none() {
                aim.key = Some(SLOT_KEYS[slot]);
                aim.target = pt.translation;
                let img = fx.assets.p_ring.clone();
                let mat = fx.unlit(&img, Color::srgb(0.0, 0.42, 0.4));
                let quad = fx.assets.quad.clone();
                let e = fx
                    .commands
                    .spawn((
                        RunEntity,
                        BlinkIndicator,
                        Mesh3d(quad),
                        MeshMaterial3d(mat),
                        Transform::from_translation(pt.translation)
                            .with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2))
                            .with_scale(Vec3::splat(2.0)),
                        bevy::light::NotShadowCaster,
                    ))
                    .id();
                aim.indicator = Some(e);
            }
        }
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn aim_blink(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut aim: ResMut<BlinkAim>,
    mut inv: ResMut<Inventory>,
    mut fx: Fx,
    mut ray: ArenaRay,
    cam: Query<&GlobalTransform, With<GameCamera>>,
    mut players: Query<(&mut Transform, &mut Player), Without<BlinkIndicator>>,
    mut indicator: Query<&mut Transform, (With<BlinkIndicator>, Without<Player>)>,
) {
    let Some(key) = aim.key else { return };
    let Ok((mut pt, mut player)) = players.single_mut() else { return };
    let Ok(cam) = cam.single() else { return };

    // where the crosshair meets the floor, capped to blink range
    let origin = cam.translation();
    let dir = cam.forward().as_vec3();
    let hit = ray
        .cast(origin, dir)
        .map(|(d, _)| origin + dir * d)
        .or_else(|| (dir.y < -1e-3).then(|| origin + dir * (-origin.y / dir.y)));
    let from = flat(pt.translation);
    let mut target = match hit {
        Some(p) => flat(p),
        None => from + flat(dir).normalize_or(Vec3::Z) * BLINK_RANGE,
    };
    target = from + (target - from).clamp_length_max(BLINK_RANGE);
    target = target.clamp_length_max(ARENA_RADIUS);
    aim.target = target;
    if let Some(Ok(mut t)) = aim.indicator.map(|e| indicator.get_mut(e)) {
        t.translation = target + Vec3::Y * 0.06;
        let pulse = 2.8 + (time.elapsed_secs() * 8.0).sin() * 0.2;
        t.scale = Vec3::splat(pulse);
    }

    if keys.pressed(key) {
        return;
    }
    // released: go
    let img = fx.assets.p_magic.clone();
    for at in [pt.translation, target] {
        fx.burst(&img, at + Vec3::Y * 0.9, 12, 5.0, 0.4, Color::srgb(0.5, 2.4, 2.0), 0.45);
    }
    pt.translation = target;
    player.vel = Vec3::ZERO;
    player.invuln = player.invuln.max(BLINK_INVULN);
    let s = fx.assets.sfx.blink.clone();
    fx.sound(&s, 0.6);
    inv.0.retain(|p| *p != Powerup::Blink);
    if let Some(e) = aim.indicator.take() {
        fx.commands.entity(e).despawn();
    }
    aim.key = None;
}

fn blast_fx(fx: &mut Fx, at: Vec3) {
    let s = fx.assets.sfx.blast.clone();
    fx.sound(&s, 0.9);
    let s = fx.assets.sfx.explosion.clone();
    fx.sound(&s, 0.5);
    fx.flash(at + Vec3::Y * 1.5, Color::srgb(1.0, 0.7, 0.35), 900_000.0, 0.4);
    // expanding ring on the floor
    let img = fx.assets.p_ring.clone();
    let mat = fx.unlit(&img, Color::srgb(3.0, 1.6, 0.6));
    let quad = fx.assets.quad.clone();
    fx.commands.spawn((
        RunEntity,
        Mesh3d(quad),
        MeshMaterial3d(mat),
        Transform::from_translation(at + Vec3::Y * 0.08).with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
        bevy::light::NotShadowCaster,
        Particle {
            max_life: 0.45,
            size: (1.0, BLAST_RADIUS * 2.3),
            color: Color::srgb(3.0, 1.6, 0.6),
            ..default()
        },
    ));
    let flame = fx.assets.p_flame.clone();
    let spark = fx.assets.p_spark.clone();
    for i in 0..14 {
        let a = i as f32 / 14.0 * std::f32::consts::TAU;
        let dir = Vec3::new(a.cos(), 0.0, a.sin());
        fx.particle(
            &flame,
            at + dir * 1.2 + Vec3::Y * 0.8,
            Particle {
                vel: dir * BLAST_RADIUS * 1.8 + Vec3::Y,
                drag: 2.5,
                max_life: 0.5,
                size: (1.2, 2.6),
                color: Color::srgb(3.5, 1.6, 0.5),
                spin_speed: 3.0,
                ..default()
            },
        );
    }
    fx.burst(&spark, at + Vec3::Y, 24, 16.0, 0.5, Color::srgb(3.0, 1.8, 0.6), 0.6);
}
