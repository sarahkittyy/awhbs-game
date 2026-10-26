// no console window in the shipped build
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

// awhbs game: over-the-shoulder survival shooter in OMORI's White Space.

mod ability;
mod arena;
mod assets;
mod balance;
#[cfg(test)]
mod sim;
#[cfg(debug_assertions)]
mod debug;
mod enemy;
mod fx;
mod gun;
mod gun_model;
mod leaderboard;
mod pickup;
mod player;
mod projectile;
mod storage;
mod ui;
#[cfg(target_arch = "wasm32")]
mod web;

use bevy::asset::AssetMetaCheck;
use bevy::prelude::*;
use bevy::window::{PresentMode, WindowResolution};

#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum GameState {
    #[default]
    Title,
    Playing,
    Paused,
    Dead,
}

/// Everything spawned for a single run; despawned when a new run starts.
#[derive(Component)]
pub struct RunEntity;

#[derive(Resource)]
pub struct Settings {
    pub sensitivity: f32,
    /// Master volume, 0..1.
    pub volume: f32,
}

impl Settings {
    fn load() -> Self {
        Self {
            sensitivity: storage::get_f32("sensitivity").unwrap_or(1.0).clamp(SENS_MIN, SENS_MAX),
            volume: storage::get_f32("volume").unwrap_or(DEFAULT_VOLUME).clamp(0.0, 1.0),
        }
    }

    pub fn save(&self) {
        storage::set("sensitivity", &self.sensitivity.to_string());
        storage::set("volume", &self.volume.to_string());
    }
}

pub const SENS_MIN: f32 = 0.1;
pub const SENS_MAX: f32 = 3.0;
pub const DEFAULT_VOLUME: f32 = 0.3;

/// Seconds survived in the current run.
#[derive(Resource, Default)]
pub struct RunClock(pub f32);

/// Sent when a new run should begin (title "play" or "play again").
#[derive(Message)]
pub struct StartRun;

/// Sent by `start_run` after the previous run has been cleaned up.
#[derive(Message)]
pub struct RunBegin;

fn main() {
    #[cfg(target_arch = "wasm32")]
    console_error_panic_hook::set_once();
    let mut app = App::new();
    app.add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "awhbs game".into(),
                        resolution: WindowResolution::new(1600, 900),
                        present_mode: PresentMode::AutoVsync,
                        #[cfg(target_arch = "wasm32")]
                        canvas: Some("#awhbs-canvas".into()),
                        #[cfg(target_arch = "wasm32")]
                        fit_canvas_to_parent: true,
                        #[cfg(target_arch = "wasm32")]
                        prevent_default_event_handling: true,
                        ..default()
                    }),
                    ..default()
                })
                .set(AssetPlugin {
                    file_path: assets_dir(),
                    // there are no .meta files; skip the extra request per asset
                    meta_check: AssetMetaCheck::Never,
                    ..default()
                }),
        )
        .init_state::<GameState>()
        .insert_resource(Settings::load())
        .insert_resource(ClearColor(Color::WHITE))
        .init_resource::<RunClock>()
        .add_message::<StartRun>()
        .add_message::<RunBegin>()
        .add_plugins((
            assets::AssetsPlugin,
            ability::AbilityPlugin,
            arena::ArenaPlugin,
            player::PlayerPlugin,
            gun::GunPlugin,
            gun_model::GunModelPlugin,
            enemy::EnemyPlugin,
            projectile::ProjectilePlugin,
            pickup::PickupPlugin,
            fx::FxPlugin,
            ui::UiPlugin,
            leaderboard::LeaderboardPlugin,
        ))
        .add_plugins(DebugPlugins)
        .add_systems(Update, (start_run, apply_volume, tick_clock.run_if(in_state(GameState::Playing))))
        .add_systems(OnEnter(GameState::Title), clear_run)
        .add_systems(OnEnter(GameState::Paused), |mut t: ResMut<Time<Virtual>>| t.pause())
        .add_systems(OnExit(GameState::Paused), |mut t: ResMut<Time<Virtual>>| t.unpause());
    #[cfg(target_arch = "wasm32")]
    app.add_plugins(web::WebPlugin);
    app.run();
}

/// On the web the assets sit in a content-hashed directory next to the page (`build-web.sh`
/// bakes its name in), so the server can cache them forever.
#[cfg(target_arch = "wasm32")]
fn assets_dir() -> String {
    option_env!("AWHBS_ASSET_DIR").unwrap_or("assets").to_string()
}

/// Assets live next to the executable so the game can be zipped and shared;
/// under `cargo run` they live in the project root.
#[cfg(not(target_arch = "wasm32"))]
fn assets_dir() -> String {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let a = dir.join("assets");
            if a.is_dir() {
                return a.to_string_lossy().into_owned();
            }
        }
    }
    std::env::current_dir()
        .map(|d| d.join("assets").to_string_lossy().into_owned())
        .unwrap_or_else(|_| "assets".into())
}

fn apply_volume(settings: Res<Settings>, mut global: ResMut<GlobalVolume>) {
    if settings.is_changed() {
        *global = GlobalVolume::new(bevy::audio::Volume::Linear(settings.volume));
    }
}

fn tick_clock(time: Res<Time>, mut clock: ResMut<RunClock>) {
    clock.0 += time.delta_secs();
}

fn start_run(
    mut msgs: MessageReader<StartRun>,
    mut commands: Commands,
    run_entities: Query<Entity, With<RunEntity>>,
    mut clock: ResMut<RunClock>,
    mut next: ResMut<NextState<GameState>>,
    mut time: ResMut<Time<Virtual>>,
    mut begin: MessageWriter<RunBegin>,
) {
    if msgs.read().count() == 0 {
        return;
    }
    for e in &run_entities {
        commands.entity(e).despawn();
    }
    clock.0 = 0.0;
    time.unpause();
    time.set_relative_speed(1.0);
    next.set(GameState::Playing);
    begin.write(RunBegin);
}

/// Back on the title screen (from the pause menu): the abandoned run is cleared away.
fn clear_run(mut commands: Commands, run_entities: Query<Entity, With<RunEntity>>, mut time: ResMut<Time<Virtual>>) {
    for e in &run_entities {
        commands.entity(e).despawn();
    }
    time.unpause();
    time.set_relative_speed(1.0);
}

pub fn rand_range(a: f32, b: f32) -> f32 {
    a + (b - a) * fastrand::f32()
}

pub fn flat(v: Vec3) -> Vec3 {
    Vec3::new(v.x, 0.0, v.z)
}

struct DebugPlugins;

impl Plugin for DebugPlugins {
    fn build(&self, _app: &mut App) {
        #[cfg(debug_assertions)]
        _app.add_plugins(debug::DebugPlugin);
    }
}
