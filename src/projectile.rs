use bevy::prelude::*;

pub use crate::balance::ProjKind;
use crate::balance::{self, ARENA_RADIUS, EXPLOSION_RADIUS, PLAYER_H};
use crate::fx::{Billboard, Fx, Particle};
use crate::player::{Player, PlayerHit};
use crate::{flat, GameState, RunEntity};

#[derive(Component)]
pub struct Projectile {
    pub kind: ProjKind,
    pub vel: Vec3,
    pub life: f32,
    pub radius: f32,
}

pub struct ProjectilePlugin;

impl Plugin for ProjectilePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, update.run_if(in_state(GameState::Playing)));
    }
}

pub fn spawn_projectile(fx: &mut Fx, kind: ProjKind, pos: Vec3, vel: Vec3) {
    let a = &fx.assets;
    let (img, size, color, glow) = match kind {
        ProjKind::Linear => (a.p_circle.clone(), 0.7, Color::srgb(0.5, 2.6, 0.9), Color::srgba(0.2, 1.0, 0.4, 0.6)),
        ProjKind::Lob => (a.snowball.clone(), 0.65, Color::WHITE, Color::srgba(0.4, 0.7, 1.0, 0.7)),
        ProjKind::Roll => (a.rubber_ball.clone(), 0.85, Color::WHITE, Color::srgba(1.0, 0.4, 0.5, 0.6)),
        ProjKind::Bomb => (a.dynamite.clone(), 0.9, Color::WHITE, Color::srgba(1.0, 0.3, 0.1, 0.8)),
        ProjKind::Homing => (a.p_magic.clone(), 1.0, Color::srgb(3.0, 0.9, 0.3), Color::srgba(1.0, 0.3, 0.1, 0.7)),
    };
    let (radius, life) = balance::proj_stats(kind);
    let glow_img = a.p_light.clone();
    let quad = a.quad.clone();
    let mat = fx.unlit(&img, color);
    let glow_mat = fx.unlit(&glow_img, glow);
    fx.commands
        .spawn((
            RunEntity,
            Projectile { kind, vel, life, radius },
            Mesh3d(quad.clone()),
            MeshMaterial3d(mat),
            Transform::from_translation(pos).with_scale(Vec3::splat(size)),
            Billboard::default(),
            bevy::light::NotShadowCaster,
        ))
        .with_child((
            Mesh3d(quad),
            MeshMaterial3d(glow_mat),
            Transform::from_xyz(0.0, 0.0, -0.02).with_scale(Vec3::splat(1.9)),
            bevy::light::NotShadowCaster,
        ));
}

fn update(
    time: Res<Time>,
    mut fx: Fx,
    players: Query<(&Transform, &Player), Without<Projectile>>,
    mut q: Query<(Entity, &mut Projectile, &mut Transform, &mut Billboard)>,
    mut hit_player: MessageWriter<PlayerHit>,
) {
    let Ok((pt, player)) = players.single() else { return };
    let dt = time.delta_secs();
    let ppos = pt.translation;
    let chest = ppos + Vec3::Y * PLAYER_H * player.height_scale() * 0.55;

    for (e, mut p, mut t, mut bb) in &mut q {
        p.life -= dt;
        p.vel = balance::step_projectile(p.kind, t.translation, p.vel, chest, dt);
        match p.kind {
            ProjKind::Homing => bb.spin += dt * 4.0,
            ProjKind::Lob | ProjKind::Bomb => bb.spin += dt * 7.0,
            // roll clockwise when moving right on screen; close enough for a ball
            ProjKind::Roll => bb.spin -= flat(p.vel).length() / p.radius * dt,
            ProjKind::Linear => {}
        }
        let v = p.vel;
        t.translation += v * dt;

        let pos = t.translation;
        let mut gone = p.life <= 0.0 || flat(pos).length() > ARENA_RADIUS + 12.0;
        let mut explode = false;

        if balance::hits_player(ppos, player.height_scale(), pos, p.radius) {
            if p.kind == ProjKind::Bomb {
                explode = true;
            } else {
                hit_player.write(PlayerHit { from: pos - p.vel.normalize_or_zero() });
            }
            gone = true;
        } else if pos.y <= p.radius * 0.8 && p.kind != ProjKind::Roll {
            gone = true;
            match p.kind {
                ProjKind::Bomb => explode = true,
                ProjKind::Lob => {
                    let img = fx.assets.p_dirt.clone();
                    fx.burst(&img, Vec3::new(pos.x, 0.1, pos.z), 6, 4.0, 0.35, Color::srgba(0.75, 0.85, 1.0, 0.9), 0.5);
                }
                _ => {}
            }
        }

        if p.kind == ProjKind::Homing && p.life <= 0.0 {
            let img = fx.assets.p_twirl.clone();
            fx.particle(
                &img,
                pos,
                Particle {
                    max_life: 0.3,
                    size: (0.8, 0.1),
                    color: Color::srgb(2.0, 0.7, 0.3),
                    spin_speed: 8.0,
                    ..default()
                },
            );
        }
        if explode {
            fx.explosion(Vec3::new(pos.x, 0.0, pos.z), EXPLOSION_RADIUS);
            let center = ppos + Vec3::Y * PLAYER_H * player.height_scale() * 0.5;
            if center.distance(Vec3::new(pos.x, 0.3, pos.z)) < EXPLOSION_RADIUS {
                hit_player.write(PlayerHit { from: pos });
            }
        }
        if gone {
            fx.commands.entity(e).despawn();
        }
    }
}
