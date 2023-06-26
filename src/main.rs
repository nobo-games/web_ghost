#![allow(clippy::type_complexity)]

use std::collections::VecDeque;

use bevy::{prelude::*, render::camera::ScalingMode, utils::HashMap};
use bevy_asset_loader::prelude::*;
use bevy_egui::{
    egui::{Align, Layout, TopBottomPanel},
    EguiContexts, EguiPlugin,
};
use bevy_ggrs::{
    ggrs::{self, GGRSEvent},
    ggrs_stage::GGRSStage,
    GGRSPlugin, GGRSSchedule, PlayerInputs, Rollback, RollbackIdProvider,
};
use bevy_matchbox::prelude::*;
use chrono::Utc;
use components::*;
use fixed_point::{Fixed, FixedWrapped, Vec2Fixed};
use input::*;
use lobby::LobbyPlugin;
use serde::{Deserialize, Serialize};

use crate::fixed_point::Fix;

mod components;
mod fixed_point;
mod input;
mod lobby;

fn main() {
    let mut app = App::new();

    GGRSPlugin::<GgrsConfig>::new()
        .with_input_system(input)
        .register_rollback_component::<Position>()
        .register_rollback_component::<BulletReady>()
        .register_rollback_component::<MoveDir>()
        .register_rollback_component::<TabId>()
        .register_type_dependency::<bool>()
        .register_type_dependency::<String>()
        .register_type_dependency::<FixedWrapped>()
        .register_type_dependency::<Vec2Fixed>()
        .build(&mut app);

    app.add_state::<GameState>()
        .add_loading_state(
            LoadingState::new(GameState::AssetLoading).continue_to_state(GameState::Matchmaking),
        )
        .add_collection_to_loading_state::<_, ImageAssets>(GameState::AssetLoading)
        .insert_resource(ClearColor(Color::rgb(0.53, 0.53, 0.53)))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                fit_canvas_to_parent: true,
                prevent_default_event_handling: false,
                ..default()
            }),
            ..default()
        }))
        .add_plugin(EguiPlugin)
        .insert_resource(ClearColor(Color::rgb(0.53, 0.53, 0.53)))
        .add_system(setup.in_schedule(OnExit(GameState::AssetLoading)))
        .add_system(start_matchbox_socket.in_schedule(OnEnter(GameState::Matchmaking)))
        .add_systems((
            bottom_bar_ui.run_if(in_state(GameState::InGame)),
            insert_player_components.in_schedule(OnEnter(GameState::InGame)),
            load_snapshot
                .in_schedule(OnEnter(GameState::InGame))
                .after(insert_player_components),
            apply_loaded_components
                .in_schedule(OnEnter(GameState::InGame))
                .after(insert_player_components)
                .after(load_snapshot),
            camera_follow.run_if(in_state(GameState::InGame)),
            cleanup_session.in_schedule(OnExit(GameState::InGame)),
            kill_game.run_if(in_state(GameState::InGame)),
        ))
        .add_systems(
            (
                move_players,
                set_translations_to_positions
                    .after(move_players)
                    .after(move_bullet),
                reload_bullet,
                fire_bullets.after(move_players).after(reload_bullet),
                move_bullet.after(move_players).after(fire_bullets),
                kill_players.after(move_bullet).after(move_players),
            )
                .in_schedule(GGRSSchedule),
        )
        .add_plugin(LobbyPlugin)
        .init_resource::<Messages>()
        .add_system(read_messages.before(kill_game))
        .run();
}

fn read_messages(
    mut messages: ResMut<Messages>,
    mut socket: Option<ResMut<MatchboxSocket<MultipleChannels>>>,
) {
    if let Some(socket) = socket.as_mut() {
        messages.0.extend(socket.channel(1).receive());
    }
}

#[derive(Resource, Default)]
struct Messages(VecDeque<(PeerId, Box<[u8]>)>);

fn kill_game(world: &mut World) {
    let bevy_ggrs::Session::P2PSession(session) = &mut *world
        .get_resource_mut::<bevy_ggrs::Session<GgrsConfig>>()
        .unwrap()
     else {
        return
     };

    if !session
        .events()
        .any(|e| matches!(e, GGRSEvent::Disconnected { .. }))
        && world.get_resource::<Messages>().unwrap().0.is_empty()
    {
        return;
    }

    info!("GGRS Disconnect event detected");
    world
        .get_resource_mut::<NextState<GameState>>()
        .unwrap()
        .set(GameState::Matchmaking);

    if let Ok(mut ready) = world
        .query_filtered::<&mut IsReady, With<IsLocal>>()
        .get_single_mut(world)
    {
        ready.0 = false;
    }

    let snapshot = world
        .get_resource::<GGRSStage<GgrsConfig>>()
        .unwrap()
        .get_serialized_snapshot(world);
    info!("Saving world snapshot: {snapshot}");
    world.insert_resource(GameSaveData {
        snapshot,
        timestamp: Utc::now(),
    });
}

fn load_snapshot(world: &mut World) {
    if let Some(snapshot) = &world
        .query_filtered::<Option<&GameSaveData>, With<IsLocal>>()
        .get_single(world)
        .expect("no local player found")
        .cloned()
    {
        info!(
            "Loading world snapshot from {}: {}",
            snapshot.timestamp, snapshot.snapshot
        );
        world.resource_scope(|world, stage: Mut<GGRSStage<GgrsConfig>>| {
            stage.load_serialized_snapshot(world, &snapshot.snapshot);
        });
    }
}

fn cleanup_session(mut commands: Commands, rollback_entities: Query<Entity, With<Rollback>>) {
    commands.remove_resource::<bevy_ggrs::Session<GgrsConfig>>();
    for entity in rollback_entities.iter() {
        commands.entity(entity).despawn();
    }
}

const MAP_SIZE: u32 = 41;
const GRID_WIDTH: f32 = 0.05;

fn setup(mut commands: Commands) {
    let mut camera_bundle = Camera2dBundle::default();
    camera_bundle.projection.scaling_mode = ScalingMode::FixedVertical(10.);
    commands.spawn(camera_bundle);

    // Horizontal lines
    for i in 0..=MAP_SIZE {
        commands.spawn(SpriteBundle {
            transform: Transform::from_translation(Vec3::new(
                0.,
                i as f32 - MAP_SIZE as f32 / 2.,
                0.,
            )),
            sprite: Sprite {
                color: Color::rgb(0.27, 0.27, 0.27),
                custom_size: Some(Vec2::new(MAP_SIZE as f32, GRID_WIDTH)),
                ..default()
            },
            ..default()
        });
    }

    // Vertical lines
    for i in 0..=MAP_SIZE {
        commands.spawn(SpriteBundle {
            transform: Transform::from_translation(Vec3::new(
                i as f32 - MAP_SIZE as f32 / 2.,
                0.,
                0.,
            )),
            sprite: Sprite {
                color: Color::rgb(0.27, 0.27, 0.27),
                custom_size: Some(Vec2::new(GRID_WIDTH, MAP_SIZE as f32)),
                ..default()
            },
            ..default()
        });
    }
}

fn insert_player_components(
    mut commands: Commands,
    mut rip: ResMut<RollbackIdProvider>,
    players: Query<(Entity, &Player)>, // This won't find any if loaded from gamestate
) {
    for (entity, player) in players.iter() {
        commands
            .entity(entity)
            .insert((
                Rollback::new(rip.next_id()),
                SpriteBundle {
                    transform: Transform::from_translation(Vec3::new(0., 0., 100.)),
                    sprite: Sprite {
                        color: Color::rgb(0., 0.47, 1.),
                        custom_size: Some(Vec2::new(1., 1.)),
                        ..default()
                    },
                    ..default()
                },
                BulletReady(true),
                MoveDir(-Vec2Fixed::new(1, 0)),
            ))
            .insert(Position(Vec2Fixed::new(
                (-8).fix() + 2 * player.handle.fix(),
                0,
            )));
    }
}

fn apply_loaded_components(
    mut commands: Commands,
    new_players: Query<(Entity, &TabId), With<Player>>,
    loaded_players: Query<(Entity, &TabId, &Position, &MoveDir, &BulletReady), Without<Player>>,
) {
    for (new_entity, new_id) in new_players.iter() {
        for (_loaded_entity, loaded_id, loaded_transform, move_dir, bullet_ready) in
            loaded_players.iter()
        {
            if new_id.0 == loaded_id.0 {
                commands.entity(new_entity).insert((
                    *loaded_transform,
                    *move_dir,
                    BulletReady(bullet_ready.0),
                ));
                break;
            }
        }
    }
    for (entity, ..) in loaded_players.iter() {
        commands.entity(entity).despawn();
    }
}

fn move_players(
    inputs: Res<PlayerInputs<GgrsConfig>>,
    mut player_query: Query<(&mut Position, &mut MoveDir, &Player)>,
) {
    for (mut position, mut move_dir, player) in player_query.iter_mut() {
        let (input, _) = inputs[player.handle];
        let direction = direction(input);

        if direction == Vec2Fixed::new(0, 0) {
            continue;
        }
        move_dir.0 = direction;

        let move_speed = 13.fix() / 100;
        let move_delta = direction * move_speed;

        let old_pos = position.0;
        let width = (MAP_SIZE.fix() + 1.fix()) / 2;
        let limit = Vec2Fixed {
            x: FixedWrapped(width),
            y: FixedWrapped(width),
        };

        let new_pos = (old_pos + move_delta).clamp(-limit, limit);

        position.0.x = new_pos.x;
        position.0.y = new_pos.y;
    }
}

fn set_translations_to_positions(mut entities: Query<(&mut Transform, &Position)>) {
    for (mut transform, position) in entities.iter_mut() {
        transform.translation.x = position.0.x.0.to_num();
        transform.translation.y = position.0.y.0.to_num();
    }
}

fn bottom_bar_ui(
    mut contexts: EguiContexts,
    mut players: Query<(&TabId, &UserInfo), With<IsLocal>>,
) {
    let (TabId(tab_id), UserInfo { name }) = players.single_mut();
    TopBottomPanel::bottom("bottom_panel").show(contexts.ctx_mut(), |ui| {
        ui.horizontal(|ui| {
            ui.label(format!("Name: {name}"));
            ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                ui.label(format!("ID: {tab_id}"));
            });
        });
    });
}

#[derive(Serialize, Deserialize, Debug, Clone)]
enum P2PMessage {
    TabId(TabId),
    Ready(bool),
    UserInfo(UserInfo),
    GameSave(Option<GameSaveData>),
}

fn start_matchbox_socket(mut commands: Commands) {
    // let room_url = "ws://127.0.0.1:3536/web_ghost";
    let room_url = "wss://areyougoingserver.solve.social/web_ghost";
    info!("connecting to matchbox server: {:?}", room_url);
    commands.insert_resource(MatchboxSocket::from(
        WebRtcSocketBuilder::new(room_url)
            .add_channel(ChannelConfig::ggrs())
            .add_reliable_channel()
            .build(),
    ));
}

impl Default for UserInfo {
    fn default() -> Self {
        Self {
            name: "New User".to_string(),
        }
    }
}

#[derive(Resource, Default)]
struct HandleMapping(HashMap<PeerId, usize>);

#[derive(Debug)]
struct GgrsConfig;

impl ggrs::Config for GgrsConfig {
    // 4-directions + fire fits easily in a single byte
    type Input = u8;
    type State = u8;
    // Matchbox' WebRtcSocket addresses are called `PeerId`s
    type Address = PeerId;
}

#[derive(Resource)]
struct LocalPlayerHandle(usize);

fn camera_follow(
    player_handle: Option<Res<LocalPlayerHandle>>,
    player_query: Query<(&Player, &Transform)>,
    mut camera_query: Query<&mut Transform, (With<Camera>, Without<Player>)>,
) {
    let player_handle = match player_handle {
        Some(handle) => handle.0,
        None => return, // Session hasn't started yet
    };
    for (player, player_transform) in player_query.iter() {
        if player.handle != player_handle {
            continue;
        }

        let pos = player_transform.translation;

        for mut transform in camera_query.iter_mut() {
            transform.translation.x = pos.x;
            transform.translation.y = pos.y;
        }
    }
}

#[derive(AssetCollection, Resource)]
struct ImageAssets {
    #[asset(path = "bullet.png")]
    bullet: Handle<Image>,
}

#[derive(States, Clone, Eq, PartialEq, Debug, Hash, Default)]
enum GameState {
    #[default]
    AssetLoading,
    Matchmaking,
    InGame,
}

fn fire_bullets(
    mut commands: Commands,
    inputs: Res<PlayerInputs<GgrsConfig>>,
    images: Res<ImageAssets>,
    mut player_query: Query<(&Position, &Player, &mut BulletReady, &MoveDir)>,
    mut rip: ResMut<RollbackIdProvider>,
) {
    for (transform, player, mut bullet_ready, move_dir) in player_query.iter_mut() {
        let (input, _) = inputs[player.handle];
        if fire(input) && bullet_ready.0 {
            let player_pos = transform.0;
            let pos = player_pos + (move_dir.0) * Fixed::from_num(PLAYER_RADIUS + BULLET_RADIUS);
            commands
                .spawn((
                    Bullet,
                    *move_dir,
                    SpriteBundle {
                        transform: Transform::from_translation(Vec2::from(pos).extend(200.))
                            .with_rotation(Quat::from_rotation_arc_2d(
                                Vec2::X,
                                Vec2::from(move_dir.0),
                            )),
                        texture: images.bullet.clone(),
                        sprite: Sprite {
                            custom_size: Some(Vec2::new(0.3, 0.1)),
                            ..default()
                        },
                        ..default()
                    },
                    Rollback::new(rip.next_id()),
                ))
                .insert(Position(pos));
            bullet_ready.0 = false;
        }
    }
}

fn move_bullet(mut query: Query<(&mut Position, &MoveDir), With<Bullet>>) {
    for (mut transform, dir) in query.iter_mut() {
        transform.0 += dir.0 * Fixed::from_num(0.35);
    }
}

#[derive(Component, Reflect, Default)]
pub struct BulletReady(pub bool);

fn reload_bullet(
    inputs: Res<PlayerInputs<GgrsConfig>>,
    mut query: Query<(&mut BulletReady, &Player)>,
) {
    for (mut can_fire, player) in query.iter_mut() {
        let (input, _) = inputs[player.handle];
        if !fire(input) {
            can_fire.0 = true;
        }
    }
}

const PLAYER_RADIUS: f32 = 0.5;
const BULLET_RADIUS: f32 = 0.025;

fn kill_players(
    mut commands: Commands,
    player_query: Query<(Entity, &Position), (With<Player>, Without<Bullet>)>,
    bullet_query: Query<&Position, With<Bullet>>,
) {
    for (player, player_transform) in player_query.iter() {
        for bullet_transform in bullet_query.iter() {
            let distance = (player_transform.0 - bullet_transform.0).norm();
            if distance < PLAYER_RADIUS + BULLET_RADIUS {
                commands.entity(player).remove::<SpriteBundle>();
            }
        }
    }
}
