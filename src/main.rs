use bevy::{
    math::Vec3Swizzles,
    prelude::*,
    render::camera::ScalingMode,
    utils::{HashMap, Uuid},
};
use bevy_asset_loader::prelude::*;
use bevy_egui::{
    egui::{Align, Layout, SidePanel, TextEdit, TopBottomPanel},
    EguiContexts, EguiPlugin,
};
use bevy_ggrs::{
    ggrs::{self, GGRSEvent, PlayerType},
    ggrs_stage::GGRSStage,
    GGRSPlugin, GGRSSchedule, PlayerInputs, Rollback, RollbackIdProvider,
};
use bevy_matchbox::prelude::*;
use chrono::{DateTime, Utc};
use components::*;
use fixed::traits::LossyFrom;
use input::*;
use serde::{Deserialize, Serialize};
use web_sys::window;

mod components;
mod input;

fn main() {
    let mut app = App::new();

    GGRSPlugin::<GgrsConfig>::new()
        .with_input_system(input)
        .register_rollback_component::<Position>()
        .register_rollback_component::<BulletReady>()
        .register_rollback_component::<MoveDir>()
        .register_rollback_component::<PersistentPeerId>()
        .register_type_dependency::<bool>()
        .register_type_dependency::<String>()
        .register_type_dependency::<SpatialFixed>()
        .register_type_dependency::<Vec2Fixed>()
        .register_type_dependency::<Vec2>()
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
            lobby.run_if(in_state(GameState::Matchmaking)),
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
            kill_game_on_disconnect.run_if(in_state(GameState::InGame)),
        ))
        .add_systems(
            (
                move_players,
                set_translations_to_positions.after(move_players),
                reload_bullet,
                fire_bullets.after(move_players).after(reload_bullet),
                move_bullet.after(move_players).after(fire_bullets),
                kill_players.after(move_bullet).after(move_players),
            )
                .in_schedule(GGRSSchedule),
        )
        .init_resource::<GameSave>()
        .run();

    // TODO: use this : .run_if(resource_exists::<InputCounter>())
}

#[derive(Serialize, Deserialize, Clone)]
struct GameSaveData {
    snapshot: String,
    timestamp: DateTime<Utc>,
}

#[derive(Resource, Default)]
struct GameSave(Option<GameSaveData>);

fn kill_game_on_disconnect(world: &mut World) {
    let bevy_ggrs::Session::P2PSession(session) = &mut *world
        .get_resource_mut::<bevy_ggrs::Session<GgrsConfig>>()
        .unwrap()
     else {
        return
     };

    if !session.events().any(|e| {
        if let GGRSEvent::Disconnected { .. } = e {
            true
        } else {
            false
        }
    }) {
        return;
    }

    info!("GGRS Disconnect event detected");
    world
        .get_resource_mut::<NextState<GameState>>()
        .unwrap()
        .set(GameState::Matchmaking);

    for (local, mut info) in world.query::<(&IsLocal, &mut PeerInfo)>().iter_mut(world) {
        if local.0 {
            info.ready = false;
            break;
        }
    }

    let snapshot = world
        .get_resource::<GGRSStage<GgrsConfig>>()
        .unwrap()
        .get_serialized_snapshot(world);
    info!("Saving world snapshot: {snapshot}");
    world.get_resource_mut::<GameSave>().unwrap().0 = Some(GameSaveData {
        snapshot,
        timestamp: Utc::now(),
    });
}

fn load_snapshot(world: &mut World) {
    if let Some(snapshot) = &world.get_resource_mut::<GameSave>().unwrap().0.take() {
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
        info!("Inserting player components, entity: {entity:?}");
        commands.entity(entity).insert((
            Rollback::new(rip.next_id()),
            SpriteBundle {
                transform: Transform::from_translation(Vec3::new(
                    -8. + 2. * player.handle as f32,
                    0.,
                    100.,
                )),
                sprite: Sprite {
                    color: Color::rgb(0., 0.47, 1.),
                    custom_size: Some(Vec2::new(1., 1.)),
                    ..default()
                },
                ..default()
            },
            BulletReady(true),
            MoveDir((-Vec2::X).into()),
        ));
    }
}

fn apply_loaded_components(
    mut commands: Commands,
    new_players: Query<(Entity, &PersistentPeerId), With<Player>>,
    loaded_players: Query<
        (
            Entity,
            &PersistentPeerId,
            &Transform,
            &MoveDir,
            &BulletReady,
        ),
        Without<Player>,
    >,
) {
    for (new_entity, new_id) in new_players.iter() {
        info!("New player: {:?}", new_id.0);
        for (_loaded_entity, loaded_id, loaded_transform, move_dir, bullet_ready) in
            loaded_players.iter()
        {
            info!("laoded player: {:?}", loaded_id.0);
            if new_id.0 == loaded_id.0 {
                info!("Match: {} == {}", new_id.0, loaded_id.0);
                commands.entity(new_entity).insert((
                    loaded_transform.clone(),
                    move_dir.clone(),
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

        if direction == Vec2::ZERO.into() {
            continue;
        }
        move_dir.0 = direction;

        let move_speed = SpatiatialFixedInner::from_num(0.13);
        let move_delta = direction * move_speed;

        let old_pos = position;
        const WIDTH: SpatiatialFixedInner =
            SpatiatialFixedInner::from(MAP_SIZE) / 2.into() - 0.5.into();
        let limit = Vec2Fixed {
            x: SpatialFixed(WIDTH),
            y: SpatialFixed(WIDTH),
        };
        let new_pos = (old_pos + move_delta).clamp(-limit, limit);

        position.x = new_pos.x;
        position.y = new_pos.y;
    }
}

fn set_translations_to_positions(mut entities: Query<(&mut Transform, &Position)>) {
    for (mut transform, position) in entities.iter_mut() {
        transform.translation.x = position.0.x.0.to_num();
        transform.translation.y = position.0.y.0.to_num();
    }
}

fn bottom_bar_ui(mut contexts: EguiContexts, mut players: Query<(&IsLocal, &mut PeerInfo)>) {
    let PeerInfo {
        name,
        persistent_id,
        ..
    } = &*players.iter_mut().find(|(local, ..)| local.0).unwrap().1;
    TopBottomPanel::bottom("bottom_panel").show(contexts.ctx_mut(), |ui| {
        ui.horizontal(|ui| {
            ui.label(format!("Name: {name}"));
            ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                ui.label(format!("ID: {persistent_id}"));
            });
        });
    });
}

#[derive(Serialize, Deserialize)]
enum P2PMessage {
    PeerInfo(PeerInfo),
    GameSave(Option<GameSaveData>),
}

fn start_matchbox_socket(mut commands: Commands) {
    let room_url = "ws://127.0.0.1:3536/web_ghost";
    info!("connecting to matchbox server: {:?}", room_url);
    // commands.insert_resource(MatchboxSocket::new_ggrs(room_url));
    commands.insert_resource(MatchboxSocket::from(
        WebRtcSocketBuilder::new(room_url)
            .add_channel(ChannelConfig::ggrs())
            .add_reliable_channel()
            .build(),
    ));
}

#[derive(Serialize, Deserialize, Default, Clone, PartialEq, Debug, Component)]
struct PeerInfo {
    ready: bool,
    name: String,
    persistent_id: Uuid,
}

#[derive(Component, Reflect, Default)]
struct PersistentPeerId(String);

#[derive(Component)]
pub struct IsLocal(bool);
#[derive(Component)]
pub struct MatchBoxId(PeerId);

#[derive(Resource, Default)]
struct HandleMapping(HashMap<PeerId, usize>);

fn lobby(
    mut commands: Commands,
    mut socket: ResMut<MatchboxSocket<MultipleChannels>>,
    mut next_state: ResMut<NextState<GameState>>,
    mut contexts: EguiContexts,
    mut players: Query<(Entity, &MatchBoxId, &IsLocal, &mut PeerInfo)>,
    mut my_gamesave: ResMut<GameSave>,
    mut gamesaves: Local<HashMap<PeerId, Option<GameSaveData>>>,
) {
    SidePanel::left("left_panel").show(contexts.ctx_mut(), |ui| {
        if socket.get_channel(0).is_err() {
            return; // we've already started
        }

        let connected_peers_ids = socket.connected_peers().collect::<Vec<_>>();
        let Some(id) = socket.id() else {
            return ;
        };

        let id_string = id.0.to_string();
        let storage = window().unwrap().session_storage().unwrap().unwrap();
        const KEY: &str = "matchbox_id";
        let unique_id = if let Ok(Some(value)) = storage.get_item(KEY) {
            value
        } else {
            info!("{KEY} not found, setting to {id_string}");
            storage.set_item(KEY, &id_string).unwrap();
            id_string.clone()
        };

        if players.is_empty() {
            let my_info = PeerInfo {
                ready: false,
                name: "Peer A".to_string(),
                persistent_id: Uuid::parse_str(&unique_id).unwrap(),
            };
            gamesaves.insert(id, my_gamesave.0.clone());
            // TODO: handle case when 2 peers connect at same time??
            // TODO: store as cookies too in hashmap, then use session storage to store key for latest cookie
            for peer in &connected_peers_ids {
                socket.channel(1).send(
                    bincode::serialize(&P2PMessage::PeerInfo(my_info.clone()))
                        .unwrap()
                        .into_boxed_slice(),
                    *peer,
                );
                socket.channel(1).send(
                    bincode::serialize(&P2PMessage::GameSave(
                        my_gamesave.0.as_ref().map(|g| g.clone()),
                    ))
                    .unwrap()
                    .into_boxed_slice(),
                    *peer,
                );
            }
            commands.spawn((
                MatchBoxId(id),
                IsLocal(true),
                my_info,
                PersistentPeerId(unique_id.to_string()),
            ));
            return;
        }

        let local_player_entity = players.iter().filter(|p| p.2 .0).next().unwrap().0;
        let my_info = players
            .get_component::<PeerInfo>(local_player_entity)
            .unwrap();

        // Check for new connections
        for (peer, state) in socket.update_peers() {
            match state {
                PeerState::Connected => {
                    info!("Peer joined: {:?}", peer);
                    socket.channel(1).send(
                        bincode::serialize(&P2PMessage::PeerInfo(my_info.clone()))
                            .unwrap()
                            .into_boxed_slice(),
                        peer,
                    );
                    socket.channel(1).send(
                        bincode::serialize(&P2PMessage::GameSave(
                            my_gamesave.0.as_ref().map(|g| g.clone()),
                        ))
                        .unwrap()
                        .into_boxed_slice(),
                        peer,
                    );
                }
                PeerState::Disconnected => {
                    info!("Peer left: {peer:?}");
                }
            }
        }

        for (entity, id, local, ..) in players.iter() {
            if !(local.0 || connected_peers_ids.contains(&id.0)) {
                commands.entity(entity).despawn();
            }
        }

        for (peer_id, packet) in socket.channel(1).receive() {
            if let Ok(p2p_message) = bincode::deserialize::<P2PMessage>(&packet) {
                match p2p_message {
                    P2PMessage::PeerInfo(info) => {
                        let entity = players
                            .iter()
                            .filter(|(_, id, ..)| id.0 == peer_id)
                            .map(|(entity, ..)| entity)
                            .next();
                        if let Some(entity) = entity {
                            commands.entity(entity).insert(info);
                        } else {
                            commands.spawn((
                                MatchBoxId(peer_id),
                                IsLocal(false),
                                PersistentPeerId(info.persistent_id.to_string()),
                                info,
                            ));
                        }
                    }
                    P2PMessage::GameSave(game_save) => {
                        gamesaves.insert(peer_id, game_save);
                    }
                }
            } else {
                warn!("Failed to deserialize P2PMessage");
            }
        }

        let mut my_info = players
            .get_component_mut::<PeerInfo>(local_player_entity)
            .unwrap();
        {
            let info_before = my_info.clone();
            ui.checkbox(&mut my_info.ready, "I'm ready");
            ui.horizontal(|ui| {
                ui.label("Name:");
                ui.add(TextEdit::singleline(&mut my_info.name).clip_text(false));
                if my_info.name.len() > 20 {
                    my_info.name.truncate(20);
                }
            });
            if *my_info != info_before {
                for peer_id in connected_peers_ids {
                    socket.channel(1).send(
                        bincode::serialize(&P2PMessage::PeerInfo(my_info.clone()))
                            .unwrap()
                            .into_boxed_slice(),
                        peer_id,
                    );
                }
            }
        }

        ui.group(|ui| {
            ui.heading("Players");
            ui.separator();
            // TODO: filter me out
            for (index, (.., peer_info)) in players.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(if peer_info.ready { "☑" } else { "☐" });
                    ui.label(format!("{index}: {}", peer_info.name));
                });
            }
        });

        if !players.iter().all(|(.., info)| info.ready) {
            return;
        }

        info!("All peers are ready, starting game");

        // Hopefully this sorting will resolve the same way on all peers
        my_gamesave.0 = gamesaves
            .drain()
            .reduce(|acc, x| match (&acc.1, &x.1) {
                (Some(acc_save), Some(x_save)) => {
                    if acc_save.timestamp > x_save.timestamp {
                        acc
                    } else if acc_save.timestamp < x_save.timestamp {
                        x
                    } else if acc.0 > x.0 {
                        acc
                    } else {
                        x
                    }
                }
                (None, Some(_)) => x,
                _ => acc,
            })
            .unwrap()
            .1;

        // create a GGRS P2P session
        let mut session_builder = ggrs::SessionBuilder::<GgrsConfig>::new()
            .with_num_players(players.iter().len())
            .with_input_delay(0);
        let socket_players = if let Some(our_id) = socket.id() {
            // player order needs to be consistent order across all peers
            let mut ids: Vec<_> = socket
                .connected_peers()
                .chain(std::iter::once(our_id))
                .collect();
            ids.sort();

            ids.into_iter()
                .map(|id| {
                    if id == our_id {
                        PlayerType::Local
                    } else {
                        PlayerType::Remote(id)
                    }
                })
                .collect::<Vec<_>>()
        } else {
            // we're still waiting for the server to initialize our id
            // no peers should be added at this point anyway
            vec![PlayerType::Local]
        };

        let mut socket_players = socket_players
            .into_iter()
            .map(|player| {
                let (entity, .., info) = players
                    .iter()
                    .find(|(_, id, local, ..)| match player {
                        PlayerType::Local => local.0,
                        PlayerType::Remote(remote_id) => remote_id == id.0,
                        PlayerType::Spectator(spectator_id) => spectator_id == id.0,
                    })
                    .unwrap();
                (player, entity, info)
            })
            .collect::<Vec<_>>();
        socket_players.sort_by_key(|(_, _, info)| info.persistent_id);
        for (i, (player, entity, ..)) in socket_players.into_iter().enumerate() {
            if let PlayerType::Local = player {
                commands.insert_resource(LocalPlayerHandle(i));
            }
            session_builder = session_builder
                .add_player(player, i)
                .expect("failed to add player");
            commands.entity(entity).insert(Player { handle: i });
        }

        // move the channel out of the socket (required because GGRS takes ownership of it)
        let channel = socket.take_channel(0).unwrap();

        // start the GGRS session
        let ggrs_session = session_builder
            .start_p2p_session(channel)
            .expect("failed to start session");

        commands.insert_resource(bevy_ggrs::Session::P2PSession(ggrs_session));
        next_state.set(GameState::InGame);
    });
}

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
    mut player_query: Query<(&Transform, &Player, &mut BulletReady, &MoveDir)>,
    mut rip: ResMut<RollbackIdProvider>,
) {
    for (transform, player, mut bullet_ready, move_dir) in player_query.iter_mut() {
        let (input, _) = inputs[player.handle];
        if fire(input) && bullet_ready.0 {
            let player_pos = transform.translation.xy();
            let pos = player_pos + move_dir.0 * PLAYER_RADIUS + BULLET_RADIUS;
            commands.spawn((
                Bullet,
                move_dir.clone(),
                SpriteBundle {
                    transform: Transform::from_translation(pos.extend(200.))
                        .with_rotation(Quat::from_rotation_arc_2d(Vec2::X, move_dir.0)),
                    texture: images.bullet.clone(),
                    sprite: Sprite {
                        custom_size: Some(Vec2::new(0.3, 0.1)),
                        ..default()
                    },
                    ..default()
                },
                Rollback::new(rip.next_id()),
            ));
            bullet_ready.0 = false;
        }
    }
}

fn move_bullet(mut query: Query<(&mut Transform, &MoveDir), With<Bullet>>) {
    for (mut transform, dir) in query.iter_mut() {
        let delta = (dir.0 * 0.35).extend(0.);
        transform.translation += delta;
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
    player_query: Query<(Entity, &Transform), (With<Player>, Without<Bullet>)>,
    bullet_query: Query<&Transform, With<Bullet>>,
) {
    for (player, player_transform) in player_query.iter() {
        for bullet_transform in bullet_query.iter() {
            let distance = Vec2::distance(
                player_transform.translation.xy(),
                bullet_transform.translation.xy(),
            );
            if distance < PLAYER_RADIUS + BULLET_RADIUS {
                commands.entity(player).remove::<SpriteBundle>();
            }
        }
    }
}
