#![allow(deprecated)]
use bevy::audio::Volume;
use bevy::prelude::*;
use bevy::text::{FontSize, FontSource};
use bevy::ui::UiGlobalTransform;
use bevy::window::PrimaryWindow;

use crate::assets::{play, GameAssets};
use crate::gun::{Gun, MAG_SIZE, RELOAD_TIME};
use crate::leaderboard::{mmss, name_char_ok, Identity, Leaderboard, RunSubmit, SubmitScore, SubmitState, NAME_MAX};
use crate::pickup::{powerup_icon, Buffs, Inventory};
use crate::player::{format_time, HurtFlash, Player};
use crate::{GameState, RunClock, RunBegin, RunEntity, Settings, StartRun, SENS_MAX, SENS_MIN};

const INK: Color = Color::srgb(0.08, 0.08, 0.1);
const PAPER: Color = Color::WHITE;
const DEATH_UI_DELAY: f32 = 1.1;

#[derive(Resource, Default, PartialEq, Eq, Clone, Copy, Debug)]
enum Screen {
    #[default]
    Main,
    Settings,
}

#[derive(Component)]
struct MenuRoot;

#[derive(Component, Clone, Copy)]
enum MenuButton {
    Play,
    Resume,
    Settings,
    Back,
    Submit,
    /// Leave the run for the title screen.
    Menu,
}

/// The leaderboard on the title and death screens; rebuilt when the board changes.
#[derive(Component)]
struct BoardPanel;

#[derive(Component)]
struct NameText;

/// Name box and submit button; hidden once the run is on the board.
#[derive(Component)]
struct SubmitRow;

#[derive(Component)]
struct SubmitStatus;

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Setting {
    Sensitivity,
    Volume,
}

impl Setting {
    /// Current value mapped to 0..1 along the slider.
    fn get(self, s: &Settings) -> f32 {
        match self {
            Setting::Sensitivity => (s.sensitivity - SENS_MIN) / (SENS_MAX - SENS_MIN),
            Setting::Volume => s.volume,
        }
    }

    fn set(self, s: &mut Settings, v: f32) {
        match self {
            Setting::Sensitivity => s.sensitivity = SENS_MIN + v * (SENS_MAX - SENS_MIN),
            Setting::Volume => s.volume = v,
        }
    }
}

#[derive(Component)]
struct SliderTrack(Setting);

#[derive(Component)]
struct SliderFill(Setting);

#[derive(Component)]
struct Hud;

#[derive(Component)]
struct HeartRow;

#[derive(Component)]
struct ItemRow;

#[derive(Component)]
struct MagIcon(u32);

#[derive(Component)]
struct ReserveText;

#[derive(Component)]
struct BuffSlot(usize);

#[derive(Component)]
struct HitMarker;

#[derive(Component)]
struct HurtOverlay;

#[derive(Component)]
struct Music;

#[derive(Resource, Default)]
struct DeadTimer(f32);

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Screen>()
            .init_resource::<DeadTimer>()
            .add_systems(Update, (escape, sync_menu, buttons, slider, spawn_hud, music).chain())
            .add_systems(Update, (board_panel, name_entry, submit_status).after(sync_menu))
            .add_systems(Update, scale_ui)
            .add_systems(Update, (hud_hearts, hud_items, hud_ammo, hud_buffs, hud_misc))
            .add_systems(OnEnter(GameState::Dead), |mut t: ResMut<DeadTimer>| t.0 = 0.0)
            .add_systems(
                Update,
                (|real: Res<Time<Real>>, mut t: ResMut<DeadTimer>| t.0 += real.delta_secs())
                    .run_if(in_state(GameState::Dead)),
            );
    }
}

fn font(assets: &GameAssets, size: f32) -> TextFont {
    TextFont {
        font: FontSource::Handle(assets.font.clone()),
        font_size: FontSize::Px(size),
        ..default()
    }
}

/// The UI is laid out for 1600x900; scale it to the window so it fits a browser tab of any size.
fn scale_ui(windows: Query<&Window, With<PrimaryWindow>>, mut scale: ResMut<UiScale>) {
    let Ok(w) = windows.single() else { return };
    let s = (w.width() / 1600.0).min(w.height() / 900.0).clamp(0.4, 2.0);
    if (scale.0 - s).abs() > 0.01 {
        scale.0 = s;
    }
}

fn escape(
    keys: Res<ButtonInput<KeyCode>>,
    state: Res<State<GameState>>,
    mut screen: ResMut<Screen>,
    mut next: ResMut<NextState<GameState>>,
    settings: Res<Settings>,
) {
    if !keys.just_pressed(KeyCode::Escape) {
        return;
    }
    match (state.get(), *screen) {
        (GameState::Playing, _) => {
            *screen = Screen::Main;
            next.set(GameState::Paused);
        }
        (_, Screen::Settings) => {
            settings.save();
            *screen = Screen::Main;
        }
        // in the browser Esc also drops pointer lock, so resuming on it would pause right away
        // again; the "play" button resumes there
        (GameState::Paused, Screen::Main) if !cfg!(target_arch = "wasm32") => next.set(GameState::Playing),
        _ => {}
    }
}

/// Rebuilds the menu overlay whenever the state or sub-screen changes.
fn sync_menu(
    mut commands: Commands,
    state: Res<State<GameState>>,
    screen: Res<Screen>,
    dead: Res<DeadTimer>,
    clock: Res<RunClock>,
    settings: Res<Settings>,
    assets: Res<GameAssets>,
    roots: Query<Entity, With<MenuRoot>>,
    mut shown: Local<Option<(GameState, Screen, bool)>>,
) {
    let s = *state.get();
    let dead_ready = s == GameState::Dead && dead.0 >= DEATH_UI_DELAY;
    let want = (s, *screen, dead_ready);
    if *shown == Some(want) {
        return;
    }
    *shown = Some(want);
    for e in &roots {
        commands.entity(e).despawn();
    }

    let overlay = match s {
        GameState::Title => Color::NONE,
        GameState::Paused => Color::srgba(1.0, 1.0, 1.0, 0.55),
        GameState::Dead => Color::srgba(1.0, 1.0, 1.0, 0.22),
        GameState::Playing => return,
    };
    if s == GameState::Dead && !dead_ready {
        return;
    }
    let mut root = commands.spawn((
        MenuRoot,
        Node {
            width: percent(100),
            height: percent(100),
            position_type: PositionType::Absolute,
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            row_gap: px(22),
            ..default()
        },
        BackgroundColor(overlay),
        GlobalZIndex(10),
    ));

    if *screen == Screen::Settings && s != GameState::Dead {
        root.with_children(|r| {
            for (label, setting) in [("Sensitivity", Setting::Sensitivity), ("Volume", Setting::Volume)] {
                r.spawn((Text::new(label), font(&assets, 44.0), TextColor(INK)));
                r.spawn((
                    SliderTrack(setting),
                    Interaction::default(),
                    bevy::ui::FocusPolicy::Block,
                    Node {
                        width: px(420),
                        height: px(22),
                        border: UiRect::all(px(3)),
                        border_radius: BorderRadius::all(px(11)),
                        margin: UiRect::bottom(px(14)),
                        ..default()
                    },
                    BorderColor::all(INK),
                    BackgroundColor(PAPER),
                ))
                .with_child((
                    SliderFill(setting),
                    Node {
                        width: percent(setting.get(&settings) * 100.0),
                        height: percent(100),
                        border_radius: BorderRadius::all(px(8)),
                        ..default()
                    },
                    BackgroundColor(INK),
                ));
            }
            r.spawn((
                MenuButton::Back,
                Interaction::default(), bevy::ui::FocusPolicy::Block,
                Node {
                    padding: UiRect::axes(px(18), px(10)),
                    border: UiRect::all(px(3)),
                    margin: UiRect::top(px(10)),
                    ..default()
                },
                BorderColor::all(INK),
                BackgroundColor(PAPER),
            ))
            .with_child((
                ImageNode::new(assets.ui_arrow.clone()).with_color(INK),
                Node {
                    width: px(40),
                    height: px(40),
                    ..default()
                },
            ));
        });
        return;
    }

    root.with_children(|r| match s {
        GameState::Title => {
            r.spawn((
                Text::new("awhbs game"),
                font(&assets, 120.0),
                TextColor(INK),
                Node {
                    margin: UiRect::bottom(px(30)),
                    ..default()
                },
            ));
            text_button(r, &assets, "play", MenuButton::Play);
            text_button(r, &assets, "settings", MenuButton::Settings);
            board_root(r);
        }
        GameState::Paused => {
            text_button(r, &assets, "play", MenuButton::Resume);
            text_button(r, &assets, "settings", MenuButton::Settings);
            text_button(r, &assets, "menu", MenuButton::Menu);
        }
        GameState::Dead => {
            r.spawn((Text::new("NOOB DOWN!!!!!"), font(&assets, 130.0), TextColor(INK)));
            r.spawn((
                Text::new(format!("lasted {}", format_time(&clock))),
                font(&assets, 50.0),
                TextColor(INK),
                Node {
                    margin: UiRect::bottom(px(10)),
                    ..default()
                },
            ));
            r.spawn((
                SubmitRow,
                Node {
                    column_gap: px(12),
                    align_items: AlignItems::Center,
                    ..default()
                },
            ))
            .with_children(|row| {
                row.spawn((
                    Node {
                        width: px(360),
                        padding: UiRect::axes(px(16), px(8)),
                        border: UiRect::all(px(3)),
                        ..default()
                    },
                    BorderColor::all(INK),
                    BackgroundColor(PAPER),
                ))
                .with_child((NameText, Text::new(""), font(&assets, 44.0), TextColor(INK)));
                row.spawn((
                    MenuButton::Submit,
                    Interaction::default(),
                    bevy::ui::FocusPolicy::Block,
                    Node {
                        padding: UiRect::axes(px(22), px(8)),
                        border: UiRect::all(px(3)),
                        ..default()
                    },
                    BorderColor::all(INK),
                    BackgroundColor(PAPER),
                ))
                .with_child((Text::new("submit"), font(&assets, 44.0), TextColor(INK)));
            });
            r.spawn((
                SubmitStatus,
                Text::new(""),
                font(&assets, 36.0),
                TextColor(INK),
                Node {
                    min_height: px(40),
                    margin: UiRect::bottom(px(14)),
                    ..default()
                },
            ));
            text_button(r, &assets, "play again", MenuButton::Play);
            board_root(r);
        }
        GameState::Playing => {}
    });
}

/// Right-hand column the leaderboard is drawn into.
fn board_root(parent: &mut ChildSpawnerCommands) {
    parent.spawn((
        BoardPanel,
        Node {
            position_type: PositionType::Absolute,
            right: px(28),
            top: px(28),
            width: px(380),
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(16)),
            row_gap: px(2),
            border: UiRect::all(px(3)),
            ..default()
        },
        BorderColor::all(INK),
        BackgroundColor(PAPER.with_alpha(0.85)),
    ));
}

fn board_row(p: &mut ChildSpawnerCommands, assets: &GameAssets, rank: &str, name: &str, time: &str, you: bool) {
    let color = if you { Color::srgb(0.75, 0.1, 0.2) } else { INK };
    p.spawn(Node {
        column_gap: px(10),
        ..default()
    })
    .with_children(|row| {
        for (text, width) in [(rank, Some(62.0)), (name, None), (time, Some(82.0))] {
            row.spawn(Node {
                width: width.map(px).unwrap_or(auto()),
                flex_grow: if width.is_none() { 1.0 } else { 0.0 },
                overflow: Overflow::clip(),
                ..default()
            })
            .with_child((Text::new(text), font(assets, 30.0), TextColor(color)));
        }
    });
}

fn board_panel(
    mut commands: Commands,
    assets: Res<GameAssets>,
    board: Res<Leaderboard>,
    panels: Query<Entity, With<BoardPanel>>,
    added: Query<(), Added<BoardPanel>>,
) {
    if !board.is_changed() && added.is_empty() {
        return;
    }
    for panel in &panels {
        commands.entity(panel).despawn_children();
        commands.entity(panel).with_children(|p| {
            p.spawn((
                Text::new("leaderboard"),
                font(&assets, 42.0),
                TextColor(INK),
                Node {
                    margin: UiRect::bottom(px(8)),
                    ..default()
                },
            ));
            let Some(b) = &board.board else {
                let msg = board.error.clone().unwrap_or_else(|| "loading...".into());
                p.spawn((Text::new(msg), font(&assets, 30.0), TextColor(INK.with_alpha(0.6))));
                return;
            };
            if b.top.is_empty() {
                p.spawn((Text::new("nobody yet!"), font(&assets, 30.0), TextColor(INK.with_alpha(0.6))));
            }
            for e in &b.top {
                board_row(p, &assets, &format!("#{}", e.rank), &e.name, &mmss(e.time_ms), e.you);
            }
            if let Some(me) = b.me.as_ref().filter(|_| !b.top.iter().any(|e| e.you)) {
                p.spawn((Text::new("..."), font(&assets, 30.0), TextColor(INK.with_alpha(0.5))));
                board_row(p, &assets, &format!("#{}", me.rank), &me.name, &mmss(me.time_ms), true);
            }
            let footer = match &b.me {
                Some(me) => format!("you're #{} of {}", me.rank, b.total),
                None if b.total == 1 => "1 player".to_string(),
                None => format!("{} players", b.total),
            };
            p.spawn((
                Text::new(footer),
                font(&assets, 28.0),
                TextColor(INK.with_alpha(0.7)),
                Node {
                    margin: UiRect::top(px(8)),
                    ..default()
                },
            ));
        });
    }
}

/// Typing into the name box on the death screen; Enter submits.
fn name_entry(
    mut keys: MessageReader<bevy::input::keyboard::KeyboardInput>,
    time: Res<Time<Real>>,
    mut id: ResMut<Identity>,
    run: Res<RunSubmit>,
    mut submit: MessageWriter<SubmitScore>,
    mut texts: Query<(&mut Text, &mut TextColor), With<NameText>>,
    mut rows: Query<&mut Node, With<SubmitRow>>,
) {
    use bevy::input::keyboard::Key;
    let done = matches!(run.state, SubmitState::Done { .. });
    for mut n in &mut rows {
        let d = if done { Display::None } else { Display::Flex };
        if n.display != d {
            n.display = d;
        }
    }
    if texts.is_empty() {
        keys.clear();
        return;
    }
    let editable = !done && run.state != SubmitState::Sending;
    for ev in keys.read() {
        if !ev.state.is_pressed() || !editable {
            continue;
        }
        match &ev.logical_key {
            Key::Backspace => {
                id.name.pop();
            }
            Key::Enter => {
                submit.write(SubmitScore);
            }
            _ => {
                if let Some(t) = &ev.text {
                    for c in t.chars().filter(|c| name_char_ok(*c)) {
                        if id.name.chars().count() < NAME_MAX {
                            id.name.push(c);
                        }
                    }
                }
            }
        }
    }
    let caret = if editable && (time.elapsed_secs() * 2.0) as u32 % 2 == 0 { "_" } else { " " };
    for (mut t, mut c) in &mut texts {
        let (text, color) = if id.name.is_empty() {
            (format!("{caret}your name"), INK.with_alpha(0.4))
        } else {
            (format!("{}{caret}", id.name), INK)
        };
        if t.0 != text {
            t.0 = text;
        }
        c.0 = color;
    }
}

fn submit_status(run: Res<RunSubmit>, mut texts: Query<(&mut Text, &mut TextColor), With<SubmitStatus>>) {
    let (text, color) = match &run.state {
        SubmitState::Idle => (String::new(), INK),
        SubmitState::Sending => ("submitting...".to_string(), INK.with_alpha(0.6)),
        SubmitState::Done { run_rank, best } => (
            format!("#{run_rank} in the world{}", if *best { "  -  new best!" } else { "" }),
            Color::srgb(0.75, 0.1, 0.2),
        ),
        SubmitState::Failed(e) => (e.clone(), Color::srgb(0.7, 0.15, 0.1)),
    };
    for (mut t, mut c) in &mut texts {
        if t.0 != text {
            t.0 = text.clone();
        }
        c.0 = color;
    }
}

fn text_button(parent: &mut ChildSpawnerCommands, assets: &GameAssets, label: &str, action: MenuButton) {
    parent
        .spawn((
            action,
            Interaction::default(), bevy::ui::FocusPolicy::Block,
            Node {
                min_width: px(300),
                padding: UiRect::axes(px(28), px(8)),
                border: UiRect::all(px(3)),
                justify_content: JustifyContent::Center,
                ..default()
            },
            BorderColor::all(INK),
            BackgroundColor(PAPER),
        ))
        .with_child((Text::new(label), font(assets, 52.0), TextColor(INK)));
}

#[allow(clippy::type_complexity)]
fn buttons(
    mut commands: Commands,
    assets: Res<GameAssets>,
    mut q: Query<(&Interaction, &MenuButton, &mut BackgroundColor, &Children), Changed<Interaction>>,
    mut text: Query<&mut TextColor>,
    mut images: Query<&mut ImageNode>,
    mut screen: ResMut<Screen>,
    mut next: ResMut<NextState<GameState>>,
    mut start: MessageWriter<StartRun>,
    mut submit: MessageWriter<SubmitScore>,
    settings: Res<Settings>,
) {
    for (interaction, action, mut bg, children) in &mut q {
        let (bgc, fg) = match interaction {
            Interaction::None => (PAPER, INK),
            _ => (INK, PAPER),
        };
        bg.0 = bgc;
        for c in children.iter() {
            if let Ok(mut t) = text.get_mut(c) {
                t.0 = fg;
            }
            if let Ok(mut i) = images.get_mut(c) {
                i.color = fg;
            }
        }
        match interaction {
            Interaction::Hovered => play(&mut commands, &assets.sfx.menu_hover, 0.4),
            Interaction::Pressed => {
                play(&mut commands, &assets.sfx.menu_click, 0.6);
                match action {
                    MenuButton::Play => {
                        *screen = Screen::Main;
                        start.write(StartRun);
                    }
                    MenuButton::Resume => next.set(GameState::Playing),
                    MenuButton::Settings => *screen = Screen::Settings,
                    MenuButton::Back => {
                        settings.save();
                        *screen = Screen::Main;
                    }
                    MenuButton::Submit => {
                        submit.write(SubmitScore);
                    }
                    MenuButton::Menu => {
                        *screen = Screen::Main;
                        next.set(GameState::Title);
                    }
                }
            }
            Interaction::None => {}
        }
    }
}

fn slider(
    windows: Query<&Window, With<PrimaryWindow>>,
    tracks: Query<(&Interaction, &ComputedNode, &UiGlobalTransform, &SliderTrack)>,
    mut fill: Query<(&mut Node, &SliderFill)>,
    mut settings: ResMut<Settings>,
) {
    let Ok(window) = windows.single() else { return };
    let Some(cursor) = window.physical_cursor_position() else { return };
    for (interaction, node, gt, SliderTrack(setting)) in &tracks {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let size = node.size();
        let left = gt.translation.x - size.x / 2.0;
        let v = ((cursor.x - left) / size.x).clamp(0.0, 1.0);
        setting.set(&mut settings, v);
        for (mut n, SliderFill(f)) in &mut fill {
            if f == setting {
                n.width = percent(v * 100.0);
            }
        }
    }
}

fn spawn_hud(mut msgs: MessageReader<RunBegin>, mut commands: Commands, assets: Res<GameAssets>) {
    if msgs.read().count() == 0 {
        return;
    }
    commands
        .spawn((
            RunEntity,
            Hud,
            Node {
                width: percent(100),
                height: percent(100),
                position_type: PositionType::Absolute,
                ..default()
            },
        ))
        .with_children(|r| {
            r.spawn((
                HurtOverlay,
                Node {
                    width: percent(100),
                    height: percent(100),
                    position_type: PositionType::Absolute,
                    ..default()
                },
                BackgroundColor(Color::NONE),
            ));
            for (marker, img, size) in [(false, &assets.crosshair, 44.0), (true, &assets.hitmarker, 30.0)] {
                let mut e = r.spawn((
                    ImageNode::new(img.clone()),
                    Node {
                        position_type: PositionType::Absolute,
                        left: percent(50),
                        top: percent(50),
                        width: px(size),
                        height: px(size),
                        margin: UiRect {
                            left: px(-size / 2.0),
                            top: px(-size / 2.0),
                            ..default()
                        },
                        ..default()
                    },
                ));
                if marker {
                    e.insert((HitMarker, Visibility::Hidden));
                }
            }
            r.spawn((
                HeartRow,
                Node {
                    position_type: PositionType::Absolute,
                    left: px(24),
                    top: px(20),
                    column_gap: px(8),
                    flex_wrap: FlexWrap::Wrap,
                    max_width: percent(45),
                    ..default()
                },
            ));
            // active buffs, top right: icon over a grey pie timer
            r.spawn(Node {
                position_type: PositionType::Absolute,
                right: px(24),
                top: px(20),
                column_gap: px(12),
                ..default()
            })
            .with_children(|b| {
                for (i, img) in [&assets.item_speed, &assets.item_firerate, &assets.item_shield, &assets.item_piercing]
                    .into_iter()
                    .enumerate()
                {
                    b.spawn((
                        BuffSlot(i),
                        Node {
                            width: px(72),
                            height: px(72),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            border_radius: BorderRadius::MAX,
                            display: Display::None,
                            ..default()
                        },
                        BackgroundGradient::from(ConicGradient::new(UiPosition::CENTER, vec![])),
                    ))
                    .with_child((
                        ImageNode::new(img.clone()),
                        Node {
                            width: px(54),
                            height: px(54),
                            ..default()
                        },
                    ));
                }
            });
            // one-shot items, bottom left, in pickup order with their key
            r.spawn((
                ItemRow,
                Node {
                    position_type: PositionType::Absolute,
                    left: px(24),
                    bottom: px(20),
                    column_gap: px(16),
                    align_items: AlignItems::FlexEnd,
                    ..default()
                },
            ));
            // ammo, bottom right
            r.spawn(Node {
                position_type: PositionType::Absolute,
                right: px(28),
                bottom: px(24),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexEnd,
                row_gap: px(10),
                ..default()
            })
            .with_children(|a| {
                a.spawn(Node {
                    flex_wrap: FlexWrap::Wrap,
                    width: px(18.0 * 13.0),
                    row_gap: px(5),
                    column_gap: px(3),
                    justify_content: JustifyContent::FlexEnd,
                    ..default()
                })
                .with_children(|m| {
                    for i in 0..MAG_SIZE {
                        m.spawn((
                            MagIcon(i),
                            ImageNode::new(assets.ui_bullet.clone()).with_color(INK),
                            Node {
                                width: px(10),
                                height: px(28),
                                ..default()
                            },
                        ));
                    }
                });
                a.spawn(Node {
                    align_items: AlignItems::Center,
                    column_gap: px(8),
                    ..default()
                })
                .with_children(|row| {
                    row.spawn((ReserveText, Text::new("108x"), font(&assets, 40.0), TextColor(INK)));
                    row.spawn((
                        ImageNode::new(assets.ui_bullets.clone()).with_color(INK),
                        Node {
                            width: px(34),
                            height: px(30),
                            ..default()
                        },
                    ));
                });
            });
        });
}

fn hud_hearts(
    mut commands: Commands,
    assets: Res<GameAssets>,
    buffs: Res<Buffs>,
    players: Query<&Player>,
    rows: Query<Entity, With<HeartRow>>,
    mut last: Local<(i32, i32, i32, Option<Entity>)>,
) {
    let Ok(p) = players.single() else { return };
    let Ok(row) = rows.single() else { return };
    let shield = buffs.shield_up();
    if *last == (p.hp, p.max_hp, shield, Some(row)) {
        return;
    }
    *last = (p.hp, p.max_hp, shield, Some(row));
    commands.entity(row).despawn_children();
    commands.entity(row).with_children(|r| {
        for i in 0..p.max_hp {
            let color = if i < p.hp { Color::WHITE } else { Color::srgba(0.1, 0.1, 0.1, 0.25) };
            r.spawn(Node {
                width: px(45),
                height: px(39),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|h| {
                if i < shield {
                    // heart-shaped shield behind the heart, one per point left
                    h.spawn((
                        ImageNode::new(assets.ui_heart_white.clone()).with_color(Color::srgb(0.45, 0.75, 1.0)),
                        Node {
                            position_type: PositionType::Absolute,
                            left: px((45.0 - 63.0) / 2.0),
                            top: px((39.0 - 55.0) / 2.0),
                            width: px(63),
                            height: px(55),
                            ..default()
                        },
                    ));
                }
                h.spawn((
                    ImageNode::new(assets.ui_heart.clone()).with_color(color),
                    Node {
                        width: px(45),
                        height: px(39),
                        ..default()
                    },
                ));
            });
        }
    });
}

fn hud_items(
    mut commands: Commands,
    assets: Res<GameAssets>,
    inv: Res<Inventory>,
    rows: Query<Entity, With<ItemRow>>,
    mut last: Local<Option<(Vec<crate::balance::Powerup>, Entity)>>,
) {
    let Ok(row) = rows.single() else { return };
    if last.as_ref().is_some_and(|(items, r)| *items == inv.0 && *r == row) {
        return;
    }
    *last = Some((inv.0.clone(), row));
    commands.entity(row).despawn_children();
    commands.entity(row).with_children(|r| {
        for (i, item) in inv.0.iter().enumerate() {
            r.spawn(Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: px(2),
                ..default()
            })
            .with_children(|c| {
                c.spawn((
                    ImageNode::new(powerup_icon(&assets, *item)),
                    Node {
                        width: px(60),
                        height: px(60),
                        ..default()
                    },
                ));
                c.spawn((Text::new(format!("[{}]", i + 1)), font(&assets, 30.0), TextColor(INK)));
            });
        }
    });
}

fn hud_ammo(gun: Res<Gun>, mut icons: Query<(&MagIcon, &mut ImageNode)>, mut text: Query<&mut Text, With<ReserveText>>) {
    let reload_fill = if gun.reloading > 0.0 {
        let k = 1.0 - gun.reloading / RELOAD_TIME;
        Some(gun.mag + ((MAG_SIZE - gun.mag).min(gun.reserve) as f32 * k) as u32)
    } else {
        None
    };
    for (MagIcon(i), mut img) in &mut icons {
        // icons empty from the left as the mag drains
        let idx = MAG_SIZE - 1 - i;
        let a = if idx < gun.mag {
            1.0
        } else if reload_fill.is_some_and(|f| idx < f) {
            0.45
        } else {
            0.12
        };
        let c = INK.with_alpha(a);
        if img.color != c {
            img.color = c;
        }
    }
    for mut t in &mut text {
        let s = format!("{}x", gun.reserve);
        if t.0 != s {
            t.0 = s;
        }
    }
}

fn hud_buffs(buffs: Res<Buffs>, mut slots: Query<(&BuffSlot, &mut Node, &mut BackgroundGradient)>) {
    for (BuffSlot(i), mut node, mut grad) in &mut slots {
        let b = [buffs.speed, buffs.fire_rate, buffs.shield, buffs.piercing][*i];
        let display = if b.active() { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
        if !b.active() {
            continue;
        }
        let frac = (b.remaining / b.duration).clamp(0.0, 1.0);
        let a = frac * std::f32::consts::TAU;
        let grey = Color::srgba(0.45, 0.45, 0.45, 0.5);
        grad.0 = vec![
            ConicGradient::new(
                UiPosition::CENTER,
                vec![
                    AngularColorStop::new(grey, 0.0),
                    AngularColorStop::new(grey, a),
                    AngularColorStop::new(Color::NONE, a),
                    AngularColorStop::new(Color::NONE, std::f32::consts::TAU),
                ],
            )
            .into(),
        ];
    }
}

fn hud_misc(
    time: Res<Time<Real>>,
    state: Res<State<GameState>>,
    gun: Res<Gun>,
    mut flash: ResMut<HurtFlash>,
    mut hud: Query<&mut Visibility, (With<Hud>, Without<HitMarker>)>,
    mut marker: Query<&mut Visibility, (With<HitMarker>, Without<Hud>)>,
    mut overlay: Query<&mut BackgroundColor, With<HurtOverlay>>,
) {
    let show = matches!(state.get(), GameState::Playing | GameState::Paused);
    for mut v in &mut hud {
        v.set_if_neq(if show { Visibility::Inherited } else { Visibility::Hidden });
    }
    for mut v in &mut marker {
        v.set_if_neq(if gun.hit_marker > 0.0 { Visibility::Inherited } else { Visibility::Hidden });
    }
    flash.0 = (flash.0 - time.delta_secs() * 2.5).max(0.0);
    for mut bg in &mut overlay {
        bg.0 = Color::srgba(0.85, 0.05, 0.1, flash.0 * 0.3);
    }
}

fn music(
    mut commands: Commands,
    state: Res<State<GameState>>,
    assets: Res<GameAssets>,
    mut current: Local<Option<(Entity, bool)>>,
    mut sinks: Query<&mut AudioSink, With<Music>>,
    settings: Res<Settings>,
) {
    let want_battle = matches!(state.get(), GameState::Playing | GameState::Paused);
    if current.map(|(_, b)| b) != Some(want_battle) {
        if let Some((e, _)) = *current {
            commands.entity(e).despawn();
        }
        let track = if want_battle { &assets.music_battle } else { &assets.music_menu };
        let e = commands
            .spawn((
                Music,
                AudioPlayer::new(track.clone()),
                PlaybackSettings::LOOP.with_volume(Volume::Linear(0.4)),
            ))
            .id();
        *current = Some((e, want_battle));
    }
    let vol = settings.volume * if *state.get() == GameState::Paused { 0.15 } else { 0.4 };
    for mut sink in &mut sinks {
        if (sink.volume().to_linear() - vol).abs() > 0.002 {
            sink.set_volume(Volume::Linear(vol));
        }
    }
}
