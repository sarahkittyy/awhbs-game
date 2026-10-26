use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::{CascadeShadowConfigBuilder, ShadowFilteringMethod};
use bevy::pbr::DistanceFog;
use bevy::post_process::bloom::Bloom;
use bevy::ecs::system::SystemParam;
use bevy::picking::mesh_picking::ray_cast::{MeshRayCast, MeshRayCastSettings, RayCastVisibility};
use bevy::prelude::*;
use bevy::world_serialization::WorldInstanceReady;

pub const ARENA_SCALE: f32 = 1.6;

#[derive(Component)]
pub struct GameCamera;

#[derive(Component)]
struct Arena;

/// Arena geometry that bullets can hit and leave holes in.
#[derive(Component)]
pub struct ArenaMesh;

/// Ray casts against the arena geometry.
#[derive(SystemParam)]
pub struct ArenaRay<'w, 's> {
    ray: MeshRayCast<'w, 's>,
    arena: Query<'w, 's, (), With<ArenaMesh>>,
}

impl ArenaRay<'_, '_> {
    /// Distance and surface normal of the nearest arena hit.
    pub fn cast(&mut self, origin: Vec3, dir: Vec3) -> Option<(f32, Vec3)> {
        let arena = &self.arena;
        let filter = |e: Entity| arena.contains(e);
        let settings = MeshRayCastSettings::default()
            .with_filter(&filter)
            .with_visibility(RayCastVisibility::Any);
        let ray = Ray3d::new(origin, Dir3::new(dir).ok()?);
        self.ray.cast_ray(ray, &settings).first().map(|(_, hit)| (hit.distance, hit.normal))
    }
}

#[derive(Resource)]
struct ArenaAnimation {
    graph: Handle<AnimationGraph>,
    node: AnimationNodeIndex,
}

pub struct ArenaPlugin;

impl Plugin for ArenaPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(GlobalAmbientLight {
            color: Color::srgb(1.0, 0.98, 0.95),
            brightness: 350.0,
            ..default()
        })
        .add_systems(Startup, setup);
    }
}

fn setup(
    mut commands: Commands,
    server: Res<AssetServer>,
    assets: Res<crate::assets::GameAssets>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
) {
    let path = "arena/white_space.glb";
    let (graph, node) = AnimationGraph::from_clip(server.load(GltfAssetLabel::Animation(0).from_asset(path)));
    commands.insert_resource(ArenaAnimation {
        graph: graphs.add(graph),
        node,
    });

    commands
        .spawn((
            Arena,
            WorldAssetRoot(assets.arena_scene.clone()),
            Transform::from_scale(Vec3::splat(ARENA_SCALE)),
        ))
        .observe(start_arena_animation);

    // soft sun for shadows
    commands.spawn((
        DirectionalLight {
            illuminance: 5500.0,
            shadow_maps_enabled: true,
            color: Color::srgb(1.0, 0.97, 0.92),
            ..default()
        },
        Transform::from_xyz(8.0, 20.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
        CascadeShadowConfigBuilder {
            first_cascade_far_bound: 12.0,
            maximum_distance: 90.0,
            ..default()
        }
        .build(),
    ));

    // the white space lightbulb glows warm over the rug
    commands.spawn((
        PointLight {
            intensity: 60_000.0,
            range: 22.0,
            color: Color::srgb(1.0, 0.86, 0.6),
            // a point light shadow is six extra passes a frame; too much for the browser build
            shadow_maps_enabled: !cfg!(target_arch = "wasm32"),
            ..default()
        },
        Transform::from_xyz(-0.5 * ARENA_SCALE, 3.1 * ARENA_SCALE, 2.5 * ARENA_SCALE),
    ));

    commands.spawn((
        GameCamera,
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            fov: 70f32.to_radians(),
            ..default()
        }),
        Msaa::Sample4,
        Tonemapping::AgX,
        Bloom::NATURAL,
        ShadowFilteringMethod::Gaussian,
        DistanceFog {
            color: Color::WHITE,
            falloff: FogFalloff::Linear {
                start: 35.0,
                end: 120.0,
            },
            ..default()
        },
        Transform::from_xyz(0.0, 4.0, 12.0).looking_at(Vec3::new(0.0, 1.0, 0.0), Vec3::Y),
    ));
}

fn start_arena_animation(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    children: Query<&Children>,
    mut players: Query<&mut AnimationPlayer>,
    anim: Res<ArenaAnimation>,
    meshes: Query<&ChildOf, With<Mesh3d>>,
    names: Query<&Name>,
) {
    for e in children.iter_descendants(ready.entity) {
        if let Ok(parent) = meshes.get(e) {
            // the bulb's floating glow particles aren't solid
            let name = names.get(parent.parent()).map(|n| n.as_str().to_owned()).unwrap_or_default();
            if !name.starts_with("LightParticles") && !name.starts_with("LightbbulbFilament") {
                commands.entity(e).insert(ArenaMesh);
            }
        }
        if let Ok(mut player) = players.get_mut(e) {
            player.play(anim.node).repeat();
            commands.entity(e).insert(AnimationGraphHandle(anim.graph.clone()));
        }
    }
}
