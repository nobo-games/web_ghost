use crate::fixed_point::Vec2Fixed;
use bevy::prelude::*;

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
