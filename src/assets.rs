use bevy::audio::Volume;
use bevy::image::{ImageLoaderSettings, ImageSampler};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;

use crate::enemy::Kind;

pub struct AssetsPlugin;

impl Plugin for AssetsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreStartup, load);
    }
}

pub struct EnemyFrames {
    pub idle: Vec<Handle<Image>>,
    pub hurt: Vec<Handle<Image>>,
    pub dead: Vec<Handle<Image>>,
}

#[derive(Resource)]
pub struct GameAssets {
    pub font: Handle<Font>,
    pub quad: Handle<Mesh>,
    pub awhbs: Handle<Image>,
    pub gun_scene: Handle<WorldAsset>,
    pub arena_scene: Handle<WorldAsset>,
    pub enemies: HashMap<Kind, EnemyFrames>,
    // items
    pub item_speed: Handle<Image>,
    pub item_firerate: Handle<Image>,
    pub item_maxhealth: Handle<Image>,
    pub item_ammo: Handle<Image>,
    pub item_blast: Handle<Image>,
    pub item_blink: Handle<Image>,
    pub item_shield: Handle<Image>,
    pub p_ring: Handle<Image>,
    pub item_heart: Handle<Image>,
    pub heal_burst: Vec<Handle<Image>>,
    pub dynamite: Handle<Image>,
    pub snowball: Handle<Image>,
    pub rubber_ball: Handle<Image>,
    // ui
    pub ui_heart: Handle<Image>,
    pub ui_heart_white: Handle<Image>,
    pub ui_bullet: Handle<Image>,
    pub ui_bullets: Handle<Image>,
    pub ui_arrow: Handle<Image>,
    pub crosshair: Handle<Image>,
    pub hitmarker: Handle<Image>,
    // particles
    pub p_muzzle: Handle<Image>,
    pub p_trace: Handle<Image>,
    pub p_smoke: Handle<Image>,
    pub p_magic: Handle<Image>,
    pub p_circle: Handle<Image>,
    pub p_light: Handle<Image>,
    pub p_spark: Handle<Image>,
    pub p_scorch: Handle<Image>,
    pub p_star: Handle<Image>,
    pub p_flame: Handle<Image>,
    pub p_fire: Handle<Image>,
    pub p_dirt: Handle<Image>,
    pub p_twirl: Handle<Image>,
    pub p_hole: Handle<Image>,
    // audio
    pub sfx: Sfx,
    pub music_battle: Handle<AudioSource>,
    pub music_menu: Handle<AudioSource>,
    /// Every handle loaded at startup, for the web page's loading bar.
    pub startup: Vec<UntypedHandle>,
}

pub struct Sfx {
    pub br_shot: Handle<AudioSource>,
    pub br_burst: Handle<AudioSource>,
    pub br_reload: Handle<AudioSource>,
    pub dry_fire: Handle<AudioSource>,
    pub zoom_in: Handle<AudioSource>,
    pub zoom_out: Handle<AudioSource>,
    pub enemy_hit: Handle<AudioSource>,
    pub enemy_die: Handle<AudioSource>,
    pub player_hurt: Handle<AudioSource>,
    pub heart_pickup: Handle<AudioSource>,
    pub ammo_pickup: Handle<AudioSource>,
    pub powerup_pickup: Handle<AudioSource>,
    pub powerup_end: Handle<AudioSource>,
    pub enemy_shoot: Handle<AudioSource>,
    pub enemy_lob: Handle<AudioSource>,
    pub explosion: Handle<AudioSource>,
    pub homing_fire: Handle<AudioSource>,
    pub projectile_pop: Handle<AudioSource>,
    pub pounce: Handle<AudioSource>,
    pub jump: Handle<AudioSource>,
    pub land: Handle<AudioSource>,
    pub glass_shatter: Handle<AudioSource>,
    pub blink: Handle<AudioSource>,
    pub shield_up: Handle<AudioSource>,
    pub shield_hit: Handle<AudioSource>,
    pub blast: Handle<AudioSource>,
    pub menu_click: Handle<AudioSource>,
    pub menu_hover: Handle<AudioSource>,
    pub footsteps: Vec<Handle<AudioSource>>,
}

fn load(mut commands: Commands, server: Res<AssetServer>, mut meshes: ResMut<Assets<Mesh>>) {
    let tracked = std::cell::RefCell::new(Vec::<UntypedHandle>::new());
    let track = |h: UntypedHandle| tracked.borrow_mut().push(h);
    let img = |p: &str| {
        let h = server.load::<Image>(p.to_string());
        track(h.clone().untyped());
        h
    };
    let pixel = |p: &str| {
        server
            .load_builder()
            .with_settings(|s: &mut ImageLoaderSettings| s.sampler = ImageSampler::nearest())
            .load::<Image>(p.to_string())
    };
    let snd = |n: &str| {
        let h = server.load::<AudioSource>(format!("sfx/{n}.ogg"));
        track(h.clone().untyped());
        h
    };
    let gun_scene: Handle<WorldAsset> = server.load(GltfAssetLabel::Scene(0).from_asset("models/br55.glb"));
    let arena_scene: Handle<WorldAsset> = server.load(GltfAssetLabel::Scene(0).from_asset("arena/white_space.glb"));
    let font: Handle<Font> = server.load("fonts/OMORI_GAME2.ttf");
    let music_battle: Handle<AudioSource> = server.load("music/battle.ogg");
    let music_menu: Handle<AudioSource> = server.load("music/menu.ogg");
    for h in [gun_scene.clone().untyped(), arena_scene.clone().untyped(), font.clone().untyped(), music_menu.clone().untyped(), music_battle.clone().untyped()] {
        track(h);
    }

    let mut enemies = HashMap::default();
    for kind in Kind::ALL {
        let key = kind.info().key;
        let frames = |anim: &str| (0..4).map(|i| img(&format!("sprites/enemies/{key}/{anim}_{i}.png"))).collect();
        enemies.insert(
            kind,
            EnemyFrames {
                idle: frames("idle"),
                hurt: frames("hurt"),
                dead: frames("dead"),
            },
        );
    }

    let mut assets = GameAssets {
        font,
        quad: meshes.add(Rectangle::new(1.0, 1.0)),
        awhbs: pixel("sprites/awhbs.png"),
        gun_scene,
        arena_scene,
        enemies,
        item_speed: img("sprites/items/speed.png"),
        item_firerate: img("sprites/items/firerate.png"),
        item_maxhealth: img("sprites/items/maxhealth.png"),
        item_ammo: img("sprites/items/ammo.png"),
        item_blast: img("sprites/items/blast.png"),
        item_blink: img("sprites/items/blink.png"),
        item_shield: img("sprites/items/shield.png"),
        p_ring: img("particles/ring.png"),
        item_heart: pixel("ui/heart.png"),
        heal_burst: (0..7).map(|i| img(&format!("sprites/items/heart_{i}.png"))).collect(),
        dynamite: img("sprites/items/dynamite.png"),
        snowball: img("sprites/items/snowball.png"),
        rubber_ball: img("sprites/items/rubber_ball.png"),
        ui_heart: pixel("ui/heart.png"),
        ui_heart_white: pixel("ui/heart_white.png"),
        ui_bullet: img("ui/bullet.png"),
        ui_bullets: img("ui/bullets.png"),
        ui_arrow: img("ui/arrowLeft.png"),
        crosshair: img("ui/crosshair.png"),
        hitmarker: img("ui/hitmarker.png"),
        p_muzzle: img("particles/muzzle_02.png"),
        p_trace: img("particles/trace_01.png"),
        p_smoke: img("particles/smoke_04.png"),
        p_magic: img("particles/magic_02.png"),
        p_circle: img("particles/circle_05.png"),
        p_light: img("particles/light_01.png"),
        p_spark: img("particles/spark_05.png"),
        p_scorch: img("particles/scorch_01.png"),
        p_star: img("particles/star_04.png"),
        p_flame: img("particles/flame_04.png"),
        p_fire: img("particles/fire_01.png"),
        p_dirt: img("particles/dirt_02.png"),
        p_twirl: img("particles/twirl_02.png"),
        p_hole: img("particles/bullet_hole.png"),
        sfx: Sfx {
            br_shot: snd("br_shot"),
            br_burst: snd("br_burst"),
            br_reload: snd("br_reload"),
            dry_fire: snd("dry_fire"),
            zoom_in: snd("zoom_in"),
            zoom_out: snd("zoom_out"),
            enemy_hit: snd("enemy_hit"),
            enemy_die: snd("enemy_die"),
            player_hurt: snd("player_hurt"),
            heart_pickup: snd("heart_pickup"),
            ammo_pickup: snd("ammo_pickup"),
            powerup_pickup: snd("powerup_pickup"),
            powerup_end: snd("powerup_end"),
            enemy_shoot: snd("enemy_shoot"),
            enemy_lob: snd("enemy_lob"),
            explosion: snd("explosion"),
            homing_fire: snd("homing_fire"),
            projectile_pop: snd("projectile_pop"),
            pounce: snd("pounce"),
            jump: snd("jump"),
            land: snd("land"),
            glass_shatter: snd("glass_shatter"),
            blink: snd("blink"),
            shield_up: snd("shield_up"),
            shield_hit: snd("shield_hit"),
            blast: snd("blast"),
            menu_click: snd("menu_click"),
            menu_hover: snd("menu_hover"),
            footsteps: (1..=4).map(|i| snd(&format!("footstep_{i}"))).collect(),
        },
        music_battle,
        music_menu,
        startup: Vec::new(),
    };
    assets.startup = tracked.into_inner();
    commands.insert_resource(assets);
}

/// Fire-and-forget sound with slight pitch variation.
pub fn play(commands: &mut Commands, sound: &Handle<AudioSource>, volume: f32) {
    play_pitched(commands, sound, volume, crate::rand_range(0.94, 1.06));
}

pub fn play_pitched(commands: &mut Commands, sound: &Handle<AudioSource>, volume: f32, speed: f32) {
    commands.spawn((
        AudioPlayer::new(sound.clone()),
        PlaybackSettings::DESPAWN
            .with_volume(Volume::Linear(volume))
            .with_speed(speed),
    ));
}
