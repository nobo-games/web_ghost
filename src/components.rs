use bevy::prelude::*;
use bevy_reflect_derive::impl_reflect_value;
use fixed::types::I20F12;
use serde::{Deserialize, Serialize};

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

pub type SpatiatialFixedInner = I20F12;

#[derive(Default, Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Deref, DerefMut)]
pub struct SpatialFixed(pub SpatiatialFixedInner);

impl_reflect_value!(SpatialFixed(
    Debug,
    PartialEq,
    Serialize,
    Deserialize,
    Default
));

#[derive(Reflect, Default, Clone, Copy, PartialEq)]
pub struct Vec2Fixed {
    pub x: SpatialFixed,
    pub y: SpatialFixed,
}

impl From<Vec2> for Vec2Fixed {
    fn from(v: Vec2) -> Self {
        Self {
            x: SpatialFixed(I20F12::from_num(v.x)),
            y: SpatialFixed(I20F12::from_num(v.y)),
        }
    }
}

impl From<Vec2Fixed> for Vec2 {
    fn from(v: Vec2Fixed) -> Self {
        Self::new(v.x.0.to_num(), v.y.0.to_num())
    }
}
