use bevy::prelude::*;

use crate::balance::{
    self, Powerup, POWERUP_MAX_ON_FIELD,
    AMMO_AMOUNT, AMMO_INTERVAL, AMMO_MAX_ON_FIELD, BUFF_DURATION, FIRE_RATE_BUFF as FIRE_RATE_MULT, HEART_INTERVAL,
    PICKUP_LIFE, PICKUP_RADIUS, POWERUP_INTERVAL, SPEED_BUFF as SPEED_MULT,
};
use crate::fx::{Billboard, Frames, Fx};
use crate::gun::{Gun, MAX_RESERVE};
use crate::player::Player;
use crate::{flat, rand_range, GameState, RunEntity, RunBegin};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PickKind {
    Ammo,
    Heart,
    Power(Powerup),
}

/// One-shot items held in the 1/2/3 slots, in pickup order. At most one of each.
#[derive(Resource, Default)]
pub struct Inventory(pub Vec<Powerup>);

#[derive(Component)]
pub struct Pickup {
    kind: PickKind,
    life: f32,
    phase: f32,
    /// Sprite texture; the pickup blinks by swapping between two cached materials of it.
    image: Handle<Image>,
}

#[derive(Clone, Copy, Default)]
pub struct Buff {
    pub remaining: f32,
    pub duration: f32,
}

impl Buff {
    pub fn active(&self) -> bool {
        self.remaining > 0.0
    }
}

#[derive(Resource, Default)]
pub struct Buffs {
    pub speed: Buff,
    pub fire_rate: Buff,
    pub shield: Buff,
    /// hits the shield can still absorb; it covers every heart when raised
    pub shield_points: i32,
}

impl Buffs {
    pub fn speed_mult(&self) -> f32 {
        if self.speed.active() { SPEED_MULT } else { 1.0 }
    }
    pub fn fire_rate_mult(&self) -> f32 {
        if self.fire_rate.active() { FIRE_RATE_MULT } else { 1.0 }
    }
    pub fn shield_up(&self) -> i32 {
        if self.shield.active() { self.shield_points } else { 0 }
    }
}

#[derive(Resource)]
struct PickupTimers {
    ammo: f32,
    heart: f32,
    powerup: f32,
}

impl Default for PickupTimers {
    fn default() -> Self {
        Self {
            ammo: 4.0,
            heart: rand_range(HEART_INTERVAL.0, HEART_INTERVAL.1),
            powerup: rand_range(POWERUP_INTERVAL.0 * 0.9, POWERUP_INTERVAL.1 * 0.9),
        }
    }
}

pub struct PickupPlugin;

impl Plugin for PickupPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Buffs>()
            .init_resource::<Inventory>()
            .init_resource::<PickupTimers>()
            .add_systems(Update, reset)
            .add_systems(
                Update,
                (spawn_timed, update_pickups, tick_buffs).run_if(in_state(GameState::Playing)),
            );
    }
}

fn reset(
    mut msgs: MessageReader<RunBegin>,
    mut buffs: ResMut<Buffs>,
    mut timers: ResMut<PickupTimers>,
    mut inv: ResMut<Inventory>,
) {
    if msgs.read().count() > 0 {
        *buffs = Buffs::default();
        *timers = PickupTimers::default();
        inv.0.clear();
    }
}

pub fn powerup_icon(assets: &crate::assets::GameAssets, p: Powerup) -> Handle<Image> {
    match p {
        Powerup::Speed => assets.item_speed.clone(),
        Powerup::FireRate => assets.item_firerate.clone(),
        Powerup::MaxHealth => assets.item_maxhealth.clone(),
        Powerup::Blast => assets.item_blast.clone(),
        Powerup::Blink => assets.item_blink.clone(),
        Powerup::Shield => assets.item_shield.clone(),
    }
}

pub fn spawn_pickup(fx: &mut Fx, kind: PickKind, pos: Vec3) {
    let a = &fx.assets;
    let (img, size, glow) = match kind {
        PickKind::Ammo => (a.item_ammo.clone(), 0.8, Color::srgba(1.0, 0.8, 0.3, 0.7)),
        PickKind::Heart => (a.item_heart.clone(), 0.8, Color::srgba(1.0, 0.2, 0.3, 0.9)),
        PickKind::Power(p) => {
            let glow = match p {
                Powerup::Speed => Color::srgba(0.4, 0.8, 1.0, 0.8),
                Powerup::FireRate => Color::srgba(1.0, 0.6, 0.2, 0.8),
                Powerup::MaxHealth => Color::srgba(1.0, 0.3, 0.6, 0.9),
                Powerup::Blast => Color::srgba(1.0, 0.45, 0.1, 0.85),
                Powerup::Blink => Color::srgba(0.5, 1.0, 0.85, 0.85),
                Powerup::Shield => Color::srgba(0.45, 0.7, 1.0, 0.9),
            };
            (powerup_icon(a, p), 1.0, glow)
        }
    };
    let quad = a.quad.clone();
    let glow_img = a.p_light.clone();
    let mat = fx.unlit(&img, Color::WHITE);
    let glow_mat = fx.unlit(&glow_img, glow);
    let floor_mat = fx.unlit(&glow_img, glow.with_alpha(glow.alpha() * 0.6));
    fx.commands
        .spawn((
            RunEntity,
            Pickup {
                kind,
                life: PICKUP_LIFE,
                phase: rand_range(0.0, 6.28),
                image: img.clone(),
            },
            Transform::from_translation(pos),
            Visibility::default(),
        ))
        .with_children(|p| {
            p.spawn((
                Mesh3d(quad.clone()),
                MeshMaterial3d(mat),
                Transform::from_xyz(0.0, 0.9, 0.0).with_scale(Vec3::splat(size)),
                Billboard {
                    upright: true,
                    spin: 0.0,
                },
                bevy::light::NotShadowCaster,
            ))
            .with_child((
                Mesh3d(quad.clone()),
                MeshMaterial3d(glow_mat),
                Transform::from_xyz(0.0, 0.0, -0.02).with_scale(Vec3::splat(2.0)),
                bevy::light::NotShadowCaster,
            ));
            p.spawn((
                Mesh3d(quad),
                MeshMaterial3d(floor_mat),
                Transform::from_xyz(0.0, 0.03, 0.0)
                    .with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2))
                    .with_scale(Vec3::splat(2.2)),
                bevy::light::NotShadowCaster,
            ));
        });
}

fn random_spot(player: Vec3) -> Vec3 {
    (0..20)
        .find_map(|_| balance::pickup_spot(player, fastrand::f32(), fastrand::f32()))
        .unwrap_or(Vec3::new(rand_range(-20.0, 20.0), 0.0, rand_range(-20.0, 20.0)))
}

fn spawn_timed(
    time: Res<Time>,
    mut timers: ResMut<PickupTimers>,
    mut fx: Fx,
    players: Query<&Transform, With<Player>>,
    pickups: Query<&Pickup>,
) {
    let Ok(pt) = players.single() else { return };
    let dt = time.delta_secs();
    let count = |k: fn(PickKind) -> bool| pickups.iter().filter(|p| k(p.kind)).count();

    timers.ammo -= dt;
    if timers.ammo <= 0.0 {
        timers.ammo = rand_range(AMMO_INTERVAL.0, AMMO_INTERVAL.1);
        if count(|k| k == PickKind::Ammo) < AMMO_MAX_ON_FIELD {
            spawn_pickup(&mut fx, PickKind::Ammo, random_spot(pt.translation));
        }
    }
    timers.heart -= dt;
    if timers.heart <= 0.0 {
        timers.heart = rand_range(HEART_INTERVAL.0, HEART_INTERVAL.1);
        if count(|k| k == PickKind::Heart) == 0 {
            spawn_pickup(&mut fx, PickKind::Heart, random_spot(pt.translation));
        }
    }
    timers.powerup -= dt;
    if timers.powerup <= 0.0 {
        timers.powerup = rand_range(POWERUP_INTERVAL.0, POWERUP_INTERVAL.1);
        if count(|k| matches!(k, PickKind::Power(_))) < POWERUP_MAX_ON_FIELD {
            let kind = PickKind::Power(Powerup::pick(fastrand::f32()));
            spawn_pickup(&mut fx, kind, random_spot(pt.translation));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn update_pickups(
    time: Res<Time>,
    mut fx: Fx,
    mut gun: ResMut<Gun>,
    mut buffs: ResMut<Buffs>,
    mut inv: ResMut<Inventory>,
    mut players: Query<(&Transform, &mut Player), Without<Pickup>>,
    mut pickups: Query<(Entity, &mut Pickup, &Transform, &Children)>,
    mut sprites: Query<(&mut Transform, &mut MeshMaterial3d<StandardMaterial>), (Without<Pickup>, Without<Player>, With<Billboard>)>,
) {
    let Ok((pt, mut player)) = players.single_mut() else { return };
    let dt = time.delta_secs();
    let now = time.elapsed_secs();
    for (e, mut pk, t, children) in &mut pickups {
        pk.life -= dt;
        if pk.life <= 0.0 {
            fx.commands.entity(e).despawn();
            continue;
        }
        // bob, and blink during the last few seconds
        let visible = pk.life > 5.0 || (pk.life * 8.0) as i32 % 2 == 0;
        if let Some(&c) = children.first() {
            if let Ok((mut st, mut mat)) = sprites.get_mut(c) {
                st.translation.y = 0.9 + (now * 2.5 + pk.phase).sin() * 0.15;
                let want = fx.unlit(&pk.image, Color::WHITE.with_alpha(if visible { 1.0 } else { 0.15 }));
                if mat.0 != want {
                    mat.0 = want;
                }
            }
        }

        let d = flat(pt.translation - t.translation).length();
        if d > PICKUP_RADIUS || (pt.translation.y - t.translation.y).abs() > 2.2 {
            continue;
        }
        let taken = match pk.kind {
            PickKind::Ammo => {
                if gun.reserve >= MAX_RESERVE {
                    false
                } else {
                    gun.reserve = (gun.reserve + AMMO_AMOUNT).min(MAX_RESERVE);
                    let s = fx.assets.sfx.ammo_pickup.clone();
                    fx.sound(&s, 0.6);
                    true
                }
            }
            PickKind::Heart => {
                if player.hp >= player.max_hp {
                    false
                } else {
                    player.hp += 1;
                    heal_fx(&mut fx, pt.translation);
                    true
                }
            }
            PickKind::Power(p) if p.is_item() => {
                // one of each: a duplicate stays on the floor
                if inv.0.contains(&p) {
                    false
                } else {
                    inv.0.push(p);
                    let s = fx.assets.sfx.powerup_pickup.clone();
                    fx.sound(&s, 0.55);
                    true
                }
            }
            PickKind::Power(Powerup::MaxHealth) => {
                player.max_hp += 1;
                player.hp += 1;
                heal_fx(&mut fx, pt.translation);
                let s = fx.assets.sfx.powerup_pickup.clone();
                fx.sound(&s, 0.5);
                true
            }
            PickKind::Power(p) => {
                let dur = rand_range(BUFF_DURATION.0, BUFF_DURATION.1);
                let b = if p == Powerup::Speed { &mut buffs.speed } else { &mut buffs.fire_rate };
                *b = Buff {
                    remaining: dur,
                    duration: dur,
                };
                let s = fx.assets.sfx.powerup_pickup.clone();
                fx.sound(&s, 0.6);
                true
            }
        };
        if taken {
            fx.commands.entity(e).despawn();
        }
    }
}

fn heal_fx(fx: &mut Fx, at: Vec3) {
    let s = fx.assets.sfx.heart_pickup.clone();
    fx.sound(&s, 0.6);
    let frames = fx.assets.heal_burst.clone();
    let mat = fx.unlit(&frames[0], Color::WHITE);
    let quad = fx.assets.quad.clone();
    fx.commands.spawn((
        RunEntity,
        Mesh3d(quad),
        MeshMaterial3d(mat),
        Transform::from_translation(at + Vec3::Y * 1.4).with_scale(Vec3::new(1.5, 1.35, 1.0)),
        Billboard::default(),
        Frames {
            frames,
            fps: 12.0,
            t: 0.0,
            looping: false,
            despawn_at_end: true,
        },
        bevy::light::NotShadowCaster,
    ));
}

fn tick_buffs(time: Res<Time>, mut buffs: ResMut<Buffs>, mut fx: Fx) {
    let dt = time.delta_secs();
    let buffs = &mut *buffs;
    for b in [&mut buffs.speed, &mut buffs.fire_rate, &mut buffs.shield] {
        if b.remaining > 0.0 {
            b.remaining -= dt;
            if b.remaining <= 0.0 {
                b.remaining = 0.0;
                let s = fx.assets.sfx.powerup_end.clone();
                fx.sound(&s, 0.5);
            }
        }
    }
}
