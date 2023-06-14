use crate::fixed_point::Vec2Fixed;
use bevy::prelude::*;
use bevy_matchbox::prelude::PeerId;

#[derive(Component)]
pub struct Player {
    pub handle: usize,
}

#[derive(Component, Reflect, Default)]
pub struct Bullet;

#[derive(Component, Reflect, Default, Clone, Copy)]
pub struct MoveDir(pub Vec2Fixed);

#[derive(Component, Reflect, Default, Clone, Copy)]
pub struct Position(pub Vec2Fixed);

#[derive(Component, Reflect, Default)]
pub struct PersistentPeerId(pub String);

#[derive(Component)]
pub struct IsLocal(pub bool);
#[derive(Component)]
pub struct MatchBoxId(pub PeerId);
