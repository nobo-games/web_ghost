use bevy::prelude::*;
use bevy_reflect_derive::impl_reflect_value;
use fixed::{traits::ToFixed, types::I20F12};
use serde::{Deserialize, Serialize};
pub type Fixed = I20F12;

#[derive(Default, Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Deref, DerefMut)]
pub struct FixedWrapped(pub Fixed);

pub trait Fix {
    fn fix(self) -> Fixed;
}

impl<T> Fix for T
where
    T: ToFixed,
{
    fn fix(self) -> Fixed {
        self.to_fixed()
    }
}

impl_reflect_value!(FixedWrapped(
    Debug,
    PartialEq,
    Serialize,
    Deserialize,
    Default
));

#[derive(Reflect, Default, Clone, Copy, PartialEq, Debug)]
pub struct Vec2Fixed {
    pub x: FixedWrapped,
    pub y: FixedWrapped,
}

impl From<Vec2Fixed> for Vec2 {
    fn from(v: Vec2Fixed) -> Self {
        Self::new(v.x.0.to_num(), v.y.0.to_num())
    }
}

impl std::ops::Mul<Fixed> for Vec2Fixed {
    type Output = Self;
    fn mul(self, rhs: Fixed) -> Self::Output {
        Self {
            x: FixedWrapped(self.x.0 * rhs),
            y: FixedWrapped(self.y.0 * rhs),
        }
    }
}
impl std::ops::Add<Vec2Fixed> for Vec2Fixed {
    type Output = Self;
    fn add(self, rhs: Vec2Fixed) -> Self::Output {
        Self {
            x: FixedWrapped(self.x.0 + rhs.x.0),
            y: FixedWrapped(self.y.0 + rhs.y.0),
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
            x: FixedWrapped(self.x.0 - rhs.x.0),
            y: FixedWrapped(self.y.0 - rhs.y.0),
        }
    }
}

impl std::ops::Neg for Vec2Fixed {
    type Output = Self;
    fn neg(self) -> Self::Output {
        Self {
            x: FixedWrapped(-self.x.0),
            y: FixedWrapped(-self.y.0),
        }
    }
}

impl Vec2Fixed {
    pub fn new(x: impl ToFixed, y: impl ToFixed) -> Self {
        Self {
            x: FixedWrapped(x.fix()),
            y: FixedWrapped(y.fix()),
        }
    }

    pub fn clamp(self, min: Vec2Fixed, max: Vec2Fixed) -> Self {
        Self {
            x: FixedWrapped(self.x.0.clamp(min.x.0, max.x.0)),
            y: FixedWrapped(self.y.0.clamp(min.y.0, max.y.0)),
        }
    }

    pub fn norm_sq(&self) -> Fixed {
        let norm_sq = self.x.0 * self.x.0 + self.y.0 * self.y.0;
        if norm_sq < 0 {
            0.fix()
        } else {
            norm_sq
        }
    }

    pub fn norm(&self) -> Fixed {
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
