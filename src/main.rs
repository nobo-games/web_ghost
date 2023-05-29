use bevy::{math::Vec3Swizzles, prelude::*, render::camera::ScalingMode, utils::HashMap};
use bevy_asset_loader::prelude::*;
use bevy_egui::{egui::SidePanel, EguiContexts, EguiPlugin};
use bevy_ggrs::{
    ggrs::{self, PlayerType},
    GGRSPlugin, GGRSSchedule, PlayerInputs, Rollback, RollbackIdProvider,
};
use bevy_matchbox::prelude::*;
use components::*;
use input::*;
use serde::{Deserialize, Serialize};

mod components;
mod input;

fn main() {
    let mut app = App::new();

    GGRSPlugin::<GgrsConfig>::new()
        .with_input_system(input)
        .register_rollback_component::<Transform>()
        .register_rollback_component::<BulletReady>()
        .register_rollback_component::<MoveDir>()
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
        .add_systems((setup, start_matchbox_socket).in_schedule(OnEnter(GameState::Matchmaking)))
        .add_systems((
            lobby.run_if(in_state(GameState::Matchmaking)),
            spawn_players.in_schedule(OnEnter(GameState::InGame)),
            camera_follow.run_if(in_state(GameState::InGame)),
        ))
        .add_systems(
            (
                move_players,
                reload_bullet,
                fire_bullets.after(move_players).after(reload_bullet),
                move_bullet.after(move_players).after(fire_bullets),
                kill_players.after(move_bullet).after(move_players),
            )
                .in_schedule(GGRSSchedule),
        )
        .run();
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

fn spawn_players(
    mut commands: Commands,
    mut rip: ResMut<RollbackIdProvider>,
    players: Query<(Entity, &Player)>,
) {
    for (entity, player) in players.iter() {
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
            MoveDir(-Vec2::X),
        ));
    }
}

fn move_players(
    inputs: Res<PlayerInputs<GgrsConfig>>,
    mut player_query: Query<(&mut Transform, &mut MoveDir, &Player)>,
) {
    for (mut transform, mut move_dir, player) in player_query.iter_mut() {
        let (input, _) = inputs[player.handle];
        let direction = direction(input);

        if direction == Vec2::ZERO {
            continue;
        }
        move_dir.0 = direction;

        let move_speed = 0.13;
        let move_delta = direction * move_speed;

        let old_pos = transform.translation.xy();
        let limit = Vec2::splat(MAP_SIZE as f32 / 2. - 0.5);
        let new_pos = (old_pos + move_delta).clamp(-limit, limit);

        transform.translation.x = new_pos.x;
        transform.translation.y = new_pos.y;
    }
}

#[derive(Serialize, Deserialize)]
enum P2PMessage {
    PeerInfo(PeerInfo),
}

fn start_matchbox_socket(mut commands: Commands) {
    let room_url = "ws://127.0.0.1:3536/web_ghost";
    info!("connecting to matchbox server: {:?}", room_url);
    commands.insert_resource(MatchboxSocket::new_ggrs(room_url));
}

#[derive(Serialize, Deserialize, Default, Clone, PartialEq, Debug, Component)]
struct PeerInfo {
    ready: bool,
    name: String,
}

#[derive(Component)]
pub struct IsLocal(bool);
#[derive(Component)]
pub struct MatchBoxId(PeerId);

#[derive(Resource, Default)]
struct HandleMapping(HashMap<PeerId, usize>);

fn lobby(
    mut commands: Commands,
    mut socket: ResMut<MatchboxSocket<SingleChannel>>,
    mut next_state: ResMut<NextState<GameState>>,
    mut contexts: EguiContexts,
    mut players: Query<(Entity, &MatchBoxId, &IsLocal, &mut PeerInfo)>,
) {
    SidePanel::left("left_panel").show(contexts.ctx_mut(), |ui| {
        if socket.get_channel(0).is_err() {
            return; // we've already started
        }

        let connected_peers_ids = socket.connected_peers().collect::<Vec<_>>();
        if let Some(id) = socket.id() {
            if players.is_empty() {
                let my_info = PeerInfo {
                    ready: false,
                    name: "Peer A".to_string(),
                };
                for peer_id in &connected_peers_ids {
                    socket.send(
                        bincode::serialize(&P2PMessage::PeerInfo(my_info.clone()))
                            .unwrap()
                            .into_boxed_slice(),
                        *peer_id,
                    );
                }
                commands.spawn((MatchBoxId(id), IsLocal(true), my_info));
                return;
            }
        } else {
            return;
        }

        let local_player_entity = players.iter().filter(|p| p.2 .0).next().unwrap().0;

        // Check for new connections
        for (peer, state) in socket.update_peers() {
            match state {
                PeerState::Connected => {
                    info!("Peer joined: {:?}", peer);
                    let my_info = players
                        .get_component::<PeerInfo>(local_player_entity)
                        .unwrap()
                        .clone();

                    socket.send(
                        bincode::serialize(&P2PMessage::PeerInfo(my_info))
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

        for (peer_id, packet) in socket.receive() {
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
                            commands.spawn((MatchBoxId(peer_id), IsLocal(false), info));
                        }
                    }
                }
            }
        }

        {
            let mut my_info = players
                .get_component_mut::<PeerInfo>(local_player_entity)
                .unwrap();
            let info_before = my_info.clone();
            ui.checkbox(&mut my_info.ready, "I'm ready");
            ui.horizontal(|ui| {
                ui.label("Name: ");
                ui.text_edit_singleline(&mut my_info.name);
            });
            if *my_info != info_before {
                for peer_id in connected_peers_ids {
                    socket.send(
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

        // create a GGRS P2P session
        let mut session_builder = ggrs::SessionBuilder::<GgrsConfig>::new()
            .with_num_players(players.iter().len())
            .with_input_delay(0);

        for (i, player) in socket.players().into_iter().enumerate() {
            let (entity, ..) = players
                .iter()
                .find(|(_, id, local, ..)| match player {
                    PlayerType::Local => local.0,
                    PlayerType::Remote(remote_id) => remote_id == id.0,
                    PlayerType::Spectator(spectator_id) => spectator_id == id.0,
                })
                .unwrap();
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
                commands.entity(player).despawn_recursive();
            }
        }
    }
}
