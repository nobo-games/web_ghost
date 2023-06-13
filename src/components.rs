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

pub type SpatialFixedInner = I20F12;

#[derive(Default, Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Deref, DerefMut)]
pub struct SpatialFixed(pub SpatialFixedInner);

impl SpatialFixed {
    pub fn from_num(num: f32) -> Self {
        Self(SpatialFixedInner::from_num(num))
    }
}

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

impl std::ops::Mul<SpatialFixedInner> for Vec2Fixed {
    type Output = Self;
    fn mul(self, rhs: SpatialFixedInner) -> Self::Output {
        Self {
            x: SpatialFixed(self.x.0 * rhs),
            y: SpatialFixed(self.y.0 * rhs),
        }
    }
}
impl std::ops::Add<Vec2Fixed> for Vec2Fixed {
    type Output = Self;
    fn add(self, rhs: Vec2Fixed) -> Self::Output {
        Self {
            x: SpatialFixed(self.x.0 + rhs.x.0),
            y: SpatialFixed(self.y.0 + rhs.y.0),
        }
    }
}

impl std::ops::AddAssign<Vec2Fixed> for Vec2Fixed {
    fn add_assign(&mut self, rhs: Vec2Fixed) {
        self.x.0 += rhs.x.0;
        self.y.0 += rhs.y.0;
    }
}

impl std::ops::Sub<Vec2Fixed> for Vec2Fixed {
    type Output = Self;
    fn sub(self, rhs: Vec2Fixed) -> Self::Output {
        Self {
            x: SpatialFixed(self.x.0 - rhs.x.0),
            y: SpatialFixed(self.y.0 - rhs.y.0),
        }
    }
}

impl std::ops::Neg for Vec2Fixed {
    type Output = Self;
    fn neg(self) -> Self::Output {
        Self {
            x: SpatialFixed(-self.x.0),
            y: SpatialFixed(-self.y.0),
        }
    }
}

impl Vec2Fixed {
    pub fn clamp(self, min: Vec2Fixed, max: Vec2Fixed) -> Self {
        Self {
            x: SpatialFixed(self.x.0.clamp(min.x.0, max.x.0)),
            y: SpatialFixed(self.y.0.clamp(min.y.0, max.y.0)),
        }
    }

    pub fn norm_sq(&self) -> SpatialFixedInner {
        self.x.0 * self.x.0 + self.y.0 * self.y.0
    }

    pub fn norm(&self) -> SpatialFixedInner {
        use fixed_sqrt::FixedSqrt;
        self.norm_sq().sqrt()
    }

    pub fn normalize_or_zero(mut self) -> Self {
        let len_sq = self.norm_sq();
        if len_sq != 0 {
            use fixed_sqrt::FixedSqrt;
            let len = len_sq.sqrt();
            self.x.0 /= len;
            self.y.0 /= len;
        }
        self
    }
}
