//! Browser glue: loading progress for the page, and pointer lock bookkeeping.

use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use wasm_bindgen::prelude::*;

use crate::assets::GameAssets;
use crate::GameState;

#[wasm_bindgen]
extern "C" {
    /// Defined by index.html; hides the loading screen once every startup asset is in.
    #[wasm_bindgen(catch, js_namespace = window, js_name = awhbsProgress)]
    fn awhbs_progress(loaded: u32, total: u32) -> Result<(), JsValue>;
}

pub struct WebPlugin;

impl Plugin for WebPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, report_progress)
            .add_systems(Update, pointer_lock.run_if(in_state(GameState::Playing)))
            .add_systems(OnEnter(GameState::Playing), |mut l: ResMut<LockState>| l.had_lock = false)
            .init_resource::<LockState>();
    }
}

fn report_progress(server: Res<AssetServer>, assets: Option<Res<GameAssets>>, mut last: Local<Option<(u32, u32)>>) {
    let Some(assets) = assets else { return };
    if last.is_some_and(|(l, t)| l >= t) {
        return;
    }
    let total = assets.startup.len() as u32;
    let loaded = assets
        .startup
        .iter()
        .filter(|h| server.is_loaded_with_dependencies(h.id()))
        .count() as u32;
    if *last != Some((loaded, total)) {
        *last = Some((loaded, total));
        let _ = awhbs_progress(loaded, total);
    }
}

#[derive(Resource, Default)]
struct LockState {
    had_lock: bool,
}

/// The browser drops pointer lock on Esc (and on alt-tab) without the game seeing the key, so a
/// lost lock mid-run pauses. A click on the canvas asks for the lock again, since a request made
/// outside a click (resuming right after Esc) can be refused.
fn pointer_lock(
    mut lock: ResMut<LockState>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut cursor: Query<&mut CursorOptions, With<PrimaryWindow>>,
    mut next: ResMut<NextState<GameState>>,
) {
    let locked = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.pointer_lock_element())
        .is_some();
    if locked {
        lock.had_lock = true;
        return;
    }
    if lock.had_lock {
        next.set(GameState::Paused);
        return;
    }
    if mouse.just_pressed(MouseButton::Left) {
        if let Ok(mut c) = cursor.single_mut() {
            c.grab_mode = CursorGrabMode::Locked;
            c.visible = false;
        }
    }
}
