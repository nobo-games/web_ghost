use crate::fixed_point::{Fixed, Vec2Fixed};
use bevy::prelude::*;
use bevy_matchbox::prelude::PeerId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Component)]
pub struct Player {
    pub handle: usize,
}

#[derive(Component, Reflect, Default)]
pub struct Bullet;

#[derive(Component, Reflect, Default)]
pub struct Character;

#[derive(Component, Reflect, Default, Clone, Copy)]
pub struct MoveDir(pub Vec2Fixed);

#[derive(Component, Reflect, Default, Clone, Copy)]
pub struct Position(pub Vec2Fixed);

#[derive(Component, Reflect, Default, Serialize, Deserialize, Clone, Debug)]
pub struct TabId(pub String);

#[derive(Component)]
pub struct IsLocal;
#[derive(Component, Debug)]
pub struct MatchBoxPeerId(pub PeerId);

#[derive(Component, Default, Clone, PartialEq, Debug)]
pub struct IsReady(pub bool);

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Component)]
pub struct UserInfo {
    pub name: String,
}

#[derive(Serialize, Deserialize, Clone, Component, Debug, Resource)]
pub struct GameSaveData {
    pub snapshot: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Component)]
pub struct Radius(pub Fixed);
