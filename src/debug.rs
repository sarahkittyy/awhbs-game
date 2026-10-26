//! Debug-build test harness driven by environment variables, used to capture
//! screenshots without a human at the keyboard. Inert unless the vars are set.

use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};

use crate::balance::Powerup;
use crate::leaderboard::{Identity, SubmitScore};
use crate::pickup::{Buff, Buffs, Inventory};
use crate::player::{CamRig, Player, PlayerHit};
use crate::{GameState, RunClock, StartRun};

pub struct DebugPlugin;

#[derive(Resource)]
struct Harness {
    autostart: bool,
    shots: Vec<f32>,
    dir: String,
    exit_after: bool,
    clock_offset: f32,
    god: bool,
    fire: bool,
    kill_at: Option<f32>,
    buffs: bool,
    turn: f32,
    items: bool,
    sprint: bool,
    /// (key, press time, release time)
    keys: Vec<(KeyCode, f32, f32)>,
    /// leaderboard name to submit under, and when to press submit
    name: Option<String>,
    submit_at: Option<f32>,
    started: bool,
    t: f32,
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|s| !s.is_empty())
}

impl Plugin for DebugPlugin {
    fn build(&self, app: &mut App) {
        let shots: Vec<f32> = env("AWHBS_SHOTS")
            .map(|s| s.split(',').filter_map(|v| v.trim().parse().ok()).collect())
            .unwrap_or_default();
        app.insert_resource(Harness {
            autostart: env("AWHBS_AUTOSTART").is_some(),
            exit_after: env("AWHBS_EXIT").is_some(),
            dir: env("AWHBS_SHOT_DIR").unwrap_or_else(|| ".".into()),
            clock_offset: env("AWHBS_CLOCK").and_then(|s| s.parse().ok()).unwrap_or(0.0),
            god: env("AWHBS_GOD").is_some(),
            fire: env("AWHBS_FIRE").is_some(),
            kill_at: env("AWHBS_KILL").and_then(|s| s.parse().ok()),
            buffs: env("AWHBS_BUFFS").is_some(),
            turn: env("AWHBS_TURN").and_then(|s| s.parse().ok()).unwrap_or(0.0),
            items: env("AWHBS_ITEMS").is_some(),
            sprint: env("AWHBS_SPRINT").is_some(),
            keys: env("AWHBS_KEYS")
                .map(|s| {
                    s.split(',')
                        .filter_map(|k| {
                            let v: Vec<&str> = k.split(':').collect();
                            let key = match *v.first()? {
                                "1" => KeyCode::Digit1,
                                "2" => KeyCode::Digit2,
                                "3" => KeyCode::Digit3,
                                "R" => KeyCode::KeyR,
                                "W" => KeyCode::KeyW,
                                "SHIFT" => KeyCode::ShiftLeft,
                                "SPACE" => KeyCode::Space,
                                "ESC" => KeyCode::Escape,
                                _ => return None,
                            };
                            Some((key, v.get(1)?.parse().ok()?, v.get(2)?.parse().ok()?))
                        })
                        .collect()
                })
                .unwrap_or_default(),
            name: env("AWHBS_NAME"),
            submit_at: env("AWHBS_SUBMIT").and_then(|s| s.parse().ok()),
            shots,
            started: false,
            t: 0.0,
        })
        .add_systems(PreUpdate, fake_input.after(bevy::input::InputSystems))
        .add_systems(Update, drive);
    }
}

fn fake_input(
    h: Res<Harness>,
    state: Res<State<GameState>>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
) {
    if *state.get() != GameState::Playing || h.t < 1.5 {
        return;
    }
    if h.fire {
        mouse.press(MouseButton::Left);
    }
    if h.sprint {
        keys.press(KeyCode::KeyW);
        keys.press(KeyCode::ShiftLeft);
    }
    for &(key, down, up) in &h.keys {
        if h.t >= down && h.t < up {
            keys.press(key);
        } else if h.t >= up && keys.pressed(key) {
            keys.release(key);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn drive(
    mut commands: Commands,
    real: Res<Time<Real>>,
    mut h: ResMut<Harness>,
    mut start: MessageWriter<StartRun>,
    mut clock: ResMut<RunClock>,
    mut players: Query<&mut Player>,
    mut buffs: ResMut<Buffs>,
    mut rig: ResMut<CamRig>,
    mut hits: MessageWriter<PlayerHit>,
    mut exit: MessageWriter<AppExit>,
    mut inv: ResMut<Inventory>,
    mut identity: ResMut<Identity>,
    mut submit: MessageWriter<SubmitScore>,
) {
    let dt = real.delta_secs();
    let before = h.t;
    h.t += dt;
    let t = h.t;
    if h.autostart && !h.started && t > 0.5 {
        h.started = true;
        start.write(StartRun);
    }
    if h.started && before <= 0.6 && t > 0.6 && h.clock_offset > 0.0 {
        clock.0 = h.clock_offset;
    }
    if let Ok(mut p) = players.single_mut() {
        if h.god {
            p.hp = p.max_hp;
        }
        if h.kill_at.is_some_and(|k| before < k && t >= k) {
            h.god = false;
            p.hp = 1;
            p.invuln = 0.0;
            hits.write(PlayerHit { from: Vec3::ZERO });
        }
    }
    if h.buffs && !buffs.speed.active() {
        buffs.speed = Buff { remaining: 20.0, duration: 30.0 };
        buffs.fire_rate = Buff { remaining: 9.0, duration: 30.0 };
        buffs.piercing = Buff { remaining: 15.0, duration: 30.0 };
    }
    rig.yaw += h.turn * dt;
    if h.items && h.started && before < 1.0 && t >= 1.0 {
        inv.0 = vec![Powerup::Blink, Powerup::Blast, Powerup::Shield];
        buffs.shield = Buff { remaining: 25.0, duration: 30.0 };
        buffs.shield_points = 3;
    }
    if let Some(name) = h.name.clone() {
        if identity.name != name {
            identity.name = name;
        }
    }
    if h.submit_at.is_some_and(|s| before < s && t >= s) {
        submit.write(SubmitScore);
    }
    let shots = h.shots.clone();
    for (i, s) in shots.iter().enumerate() {
        if before < *s && t >= *s {
            let path = format!("{}/shot_{i}.png", h.dir);
            commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
        }
    }
    if h.exit_after && shots.last().is_some_and(|l| t > l + 1.0) {
        exit.write(AppExit::Success);
    }
}
