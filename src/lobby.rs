use crate::{
    components::{IsLocal, MatchBoxId, PersistentPeerId, Player},
    GameSave, GameSaveData, GameState, GgrsConfig, LocalPlayerHandle, P2PMessage, PeerInfo,
};
use bevy::{prelude::*, utils::Uuid};
use bevy_egui::{
    egui::{SidePanel, TextEdit},
    EguiContexts,
};
use bevy_ggrs::ggrs::{self, PlayerType};
use bevy_matchbox::{
    prelude::{MultipleChannels, PeerId, PeerState},
    MatchboxSocket,
};
use std::collections::HashMap;
use web_sys::window;

pub struct LobbyPlugin;

impl Plugin for LobbyPlugin {
    fn build(&self, app: &mut App) {
        app.add_system(lobby.run_if(in_state(GameState::Matchmaking)));
    }
}

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
