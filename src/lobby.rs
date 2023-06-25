use crate::{
    components::{IsLocal, IsReady, MatchBoxId, Player, TabId, UserInfo},
    GameSaveData, GameState, GgrsConfig, LocalPlayerHandle, P2PMessage,
};
use bevy::prelude::*;
use bevy_egui::{
    egui::{SidePanel, TextEdit, Ui},
    EguiContexts,
};
use bevy_ggrs::ggrs::{self, PlayerType};
use bevy_matchbox::{
    prelude::{MultipleChannels, PeerId, PeerState},
    MatchboxSocket,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, VecDeque},
    fmt::Debug,
};
use wasm_cookies::CookieOptions;
use web_sys::window;

pub struct LobbyPlugin;

impl Plugin for LobbyPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems((
            update_peers.run_if(in_state(GameState::Matchmaking)),
            receive_from_peers
                .run_if(in_state(GameState::Matchmaking))
                .after(update_peers),
            set_local_metadata.run_if(in_state(GameState::Matchmaking)),
            lobby
                .run_if(in_state(GameState::Matchmaking))
                .after(update_peers),
            broadcast_my_info_changes
                .run_if(in_state(GameState::Matchmaking))
                .after(update_peers),
            ui.run_if(in_state(GameState::Matchmaking)),
        ));
        add_local_property::<UserInfo>(app);
    }
}

fn add_local_property<
    T: Clone + PartialEq + Debug + Default + Serialize + for<'de> Deserialize<'de> + Component,
>(
    app: &mut App,
) {
    app.add_system(update_local_property::<T>.run_if(in_state(GameState::Matchmaking)))
        .add_system(set_local_property::<T>.run_if(in_state(GameState::Matchmaking)));
}

fn set_local_metadata(
    mut commands: Commands,
    socket: Res<MatchboxSocket<MultipleChannels>>,
    players: Query<With<IsLocal>>,
    mut stored_gamesave: Option<Res<GameSaveData>>,
) {
    if players.is_empty() {
        if let Some(peer_id) = socket.id() {
            let peer_id_string = peer_id.0.to_string();
            let window = window().unwrap();

            let storage = window.session_storage().unwrap().unwrap();
            const TAB_ID_KEY: &str = "tab_id";
            let tab_id = if let Ok(Some(value)) = storage.get_item(TAB_ID_KEY) {
                value
            } else {
                info!("{TAB_ID_KEY} not found, setting to {peer_id_string}");
                storage.set_item(TAB_ID_KEY, &peer_id_string).unwrap();
                peer_id_string
            };
            let mut entity_commands =
                commands.spawn((MatchBoxId(peer_id), IsLocal, TabId(tab_id), IsReady(false)));
            if let Some(gamesave) = stored_gamesave.take() {
                entity_commands.insert(gamesave.to_owned());
            }
        }
    }
}

fn get_cookie_map<T: for<'de> Deserialize<'de>>(key: &str) -> HashMap<String, T> {
    wasm_cookies::get(key)
        .map(|map_result| map_result.unwrap())
        .map(|map| ron::from_str::<HashMap<String, T>>(&map).unwrap())
        .unwrap_or_default()
}

fn set_local_property<T>(
    mut commands: Commands,
    entity: Query<(Entity, &TabId), (Without<T>, With<IsLocal>)>,
) where
    for<'de> T: Deserialize<'de> + Default + Serialize + Debug + Clone + Component,
{
    if let Some((entity, tab_id)) = entity.iter().next() {
        let key = std::any::type_name::<T>();
        let mut map = get_cookie_map::<T>(key);
        let value = if let Some(value) = map.get(&tab_id.0) {
            info!("{key} found in cookies: {value:?}");
            value.clone()
        } else {
            let value = map
                .values()
                .next()
                .map(|v| v.to_owned())
                .unwrap_or_default();
            info!("{key} not found in cookies, setting to {value:?}");
            map.insert(tab_id.0.clone(), value.clone());
            wasm_cookies::set(
                key,
                &ron::to_string(&map).unwrap(),
                &CookieOptions::default(),
            );
            value
        };
        commands.entity(entity).insert(value);
    }
}

fn update_local_property<T>(property: Query<(&T, &TabId), (Changed<T>, With<IsLocal>)>)
where
    for<'de> T: Deserialize<'de> + Serialize + Clone + Component,
{
    if let Some((property, tab_id)) = property.iter().next() {
        let key = std::any::type_name::<T>();
        let mut map = get_cookie_map::<T>(key);
        map.insert(tab_id.0.clone(), property.clone());
        wasm_cookies::set(
            key,
            &ron::to_string(&map).unwrap(),
            &CookieOptions::default(),
        );
    }
}

fn broadcast_my_info_changes(
    mut socket: ResMut<MatchboxSocket<MultipleChannels>>,
    my_info: Query<&UserInfo, (With<IsLocal>, Changed<UserInfo>)>,
    my_ready: Query<&IsReady, (With<IsLocal>, Changed<IsReady>)>,
) {
    for message in [
        my_info
            .get_single()
            .map(|x| P2PMessage::UserInfo(x.clone())),
        my_ready.get_single().map(|x| P2PMessage::Ready(x.0)),
    ]
    .iter()
    .flatten()
    {
        info!("Broadcasting user_info: {message:?}");
        for peer_id in socket.connected_peers().collect::<Vec<_>>().iter() {
            socket.send_p2p_message(peer_id, message.clone());
        }
    }
}

trait SocketExt {
    fn send_p2p_message(&mut self, peer_id: &PeerId, message: P2PMessage);
}

impl SocketExt for MatchboxSocket<MultipleChannels> {
    fn send_p2p_message(&mut self, peer_id: &PeerId, message: P2PMessage) {
        self.channel(1).send(
            bincode::serialize(&message).unwrap().into_boxed_slice(),
            *peer_id,
        );
    }
}

fn maybe_mutate<T: Clone + PartialEq + Debug>(
    ui: &mut Ui,
    data: &mut Mut<T>,
    f: impl FnOnce(&mut Ui, &mut T),
) {
    let mut working_version = data.as_ref().clone();
    f(ui, &mut working_version);
    if working_version != *data.as_ref() {
        **data = working_version;
    }
}

fn ui(
    mut contexts: EguiContexts,
    mut my_info: Query<(&mut UserInfo, &mut IsReady), With<IsLocal>>,
    other_players: Query<(&UserInfo, &IsReady), Without<IsLocal>>,
) {
    if my_info.is_empty() {
        return;
    }
    SidePanel::left("left_panel").show(contexts.ctx_mut(), |ui| {
        ui.heading("Lobby");
        ui.separator();
        let (mut my_info, mut ready) = my_info.single_mut();
        ui.horizontal(|ui| {
            ui.label("Name:");
            maybe_mutate(ui, &mut my_info, |ui, UserInfo { name }| {
                ui.add(TextEdit::singleline(name).clip_text(false));
                const MAX_NAME_LENGTH: usize = 20;
                if name.len() > MAX_NAME_LENGTH {
                    name.truncate(MAX_NAME_LENGTH);
                }
            });
        });
        maybe_mutate(ui, &mut ready, |ui, ready| {
            ui.checkbox(&mut ready.0, "I'm ready");
        });

        ui.group(|ui| {
            ui.heading("Players");
            ui.separator();
            for (index, (info, ready)) in other_players.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(if ready.0 { "☑" } else { "☐" });
                    ui.label(format!("{index}: {}", info.name));
                });
            }
        });
    });
}

fn update_peers(
    mut commands: Commands,
    mut socket: ResMut<MatchboxSocket<MultipleChannels>>,
    my_info: Query<(&TabId, &IsReady, &UserInfo, Option<&GameSaveData>), With<IsLocal>>,
    players: Query<(Entity, &MatchBoxId)>,
) {
    let Ok((tab_id, ready, user_info, gamesave)) = my_info.get_single() else {
        return;
    };
    for (peer_id, peer_state) in socket.update_peers() {
        match peer_state {
            PeerState::Connected => {
                info!("Peer joined: {:?}", peer_id);
                commands.spawn((MatchBoxId(peer_id), IsReady(false)));
                socket.send_p2p_message(&peer_id, P2PMessage::TabId(tab_id.clone()));
                socket.send_p2p_message(&peer_id, P2PMessage::Ready(ready.0));
                socket.send_p2p_message(
                    &peer_id,
                    P2PMessage::GameSave(gamesave.map(|g| g.to_owned())),
                );
                socket.send_p2p_message(&peer_id, P2PMessage::UserInfo(user_info.clone()));
            }
            PeerState::Disconnected => {
                info!("Peer left: {:?}", peer_id);
                if let Some((entity, ..)) = players.iter().find(|(.., id)| id.0 == peer_id) {
                    info!("Despawning entity: {:?}", entity);
                    commands.entity(entity).despawn();
                }
            }
        }
    }
}

fn receive_from_peers(
    mut commands: Commands,
    mut socket: ResMut<MatchboxSocket<MultipleChannels>>,
    players: Query<(Entity, &MatchBoxId)>,
    mut messages: Local<VecDeque<(PeerId, Box<[u8]>)>>,
) {
    messages.extend(socket.channel(1).receive());
    messages.retain(|(peer_id, packet)| {
        if let Some(entity) = players
            .iter()
            .find(|(_, id)| id.0 == *peer_id)
            .map(|(entity, ..)| entity)
        {
            if let Ok(p2p_message) = bincode::deserialize::<P2PMessage>(packet) {
                let mut entity_commands = commands.entity(entity);
                info!("Received P2PMessage: {:?}", p2p_message);
                match p2p_message {
                    P2PMessage::TabId(tab_id) => {
                        entity_commands.insert(tab_id);
                    }
                    P2PMessage::GameSave(Some(game_save)) => {
                        entity_commands.insert(game_save);
                    }
                    P2PMessage::GameSave(None) => {
                        entity_commands.remove::<GameSaveData>();
                    }
                    P2PMessage::Ready(ready) => {
                        entity_commands.insert(IsReady(ready));
                    }
                    P2PMessage::UserInfo(user_info) => {
                        entity_commands.insert(user_info);
                    }
                }
            } else {
                warn!("Failed to deserialize P2PMessage");
            }
            false
        } else {
            true
        }
    });
    while messages.len() > 100 {
        messages.pop_front();
    }
}

#[allow(clippy::too_many_arguments)]
fn lobby(
    mut commands: Commands,
    mut socket: ResMut<MatchboxSocket<MultipleChannels>>,
    mut next_state: ResMut<NextState<GameState>>,
    game_saves: Query<(&MatchBoxId, Option<&GameSaveData>)>,
    players: Query<(Entity, &MatchBoxId, &TabId, &IsReady, Option<&GameSaveData>)>,
    my_player: Query<(Entity, &MatchBoxId, &TabId, &IsReady, Option<&GameSaveData>), With<IsLocal>>,
) {
    if players.is_empty() || my_player.is_empty() || !players.iter().all(|(.., ready, _)| ready.0) {
        return;
    }

    info!("All peers are ready, starting game");

    // Hopefully this sorting will resolve the same way on all peers
    let (entity, ..) = my_player.single();
    if let Some(best_save) = game_saves
        .iter()
        .filter_map(|(id, gamesave)| gamesave.map(|gamesave| (id.0, gamesave)))
        .reduce(|acc, x| {
            if acc.1.timestamp > x.1.timestamp {
                acc
            } else if acc.1.timestamp < x.1.timestamp {
                x
            } else if acc.0 > x.0 {
                acc
            } else {
                x
            }
        })
        .map(|(_, gamesave)| gamesave)
    {
        commands.entity(entity).insert(best_save.clone());
    }

    // create a GGRS P2P session
    let mut session_builder = ggrs::SessionBuilder::<GgrsConfig>::new()
        .with_num_players(players.iter().len())
        .with_input_delay(0);
    let socket_players = if let Some(our_id) = socket.id() {
        // player order needs to be consistent order across all peers
        let mut ids = socket
            .connected_peers()
            .chain(std::iter::once(our_id))
            .collect::<Vec<_>>();
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
            let (entity, _, info, ..) = players
                .iter()
                .find(|(_, id, ..)| match player {
                    PlayerType::Local => false, // local player is handled separately below
                    PlayerType::Remote(remote_id) => remote_id == id.0,
                    PlayerType::Spectator(spectator_id) => spectator_id == id.0,
                })
                .unwrap_or_else(|| my_player.single());
            (player, entity, info)
        })
        .collect::<Vec<_>>();
    socket_players.sort_by_key(|(_, _, tab_id)| &tab_id.0);
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
}
