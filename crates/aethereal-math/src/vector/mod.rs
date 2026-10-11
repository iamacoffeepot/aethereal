macro_rules! impl_vec_ops {
    ($Vec:ident { $($field:ident),+ }) => {
        impl core::ops::Add for $Vec {
            type Output = Self;
            #[inline]
            fn add(self, rhs: Self) -> Self {
                Self {
                    $($field: self.$field + rhs.$field),+
                }
            }
        }
        impl core::ops::Sub for $Vec {
            type Output = Self;
            #[inline]
            fn sub(self, rhs: Self) -> Self {
                Self {
                    $($field: self.$field - rhs.$field),+
                }
            }
        }
        impl core::ops::Neg for $Vec {
            type Output = Self;
            #[inline]
            fn neg(self) -> Self {
                Self {
                    $($field: -self.$field),+
                }
            }
        }
        impl core::ops::Mul<f32> for $Vec {
            type Output = Self;
            #[inline]
            fn mul(self, rhs: f32) -> Self {
                Self {
                    $($field: self.$field * rhs),+
                }
            }
        }
        impl core::ops::Div<f32> for $Vec {
            type Output = Self;
            #[inline]
            fn div(self, rhs: f32) -> Self {
                Self {
                    $($field: self.$field / rhs),+
                }
            }
        }
        impl core::ops::AddAssign for $Vec {
            #[inline]
            fn add_assign(&mut self, rhs: Self) {
                $(self.$field += rhs.$field;)+
            }
        }
        impl core::ops::SubAssign for $Vec {
            #[inline]
            fn sub_assign(&mut self, rhs: Self) {
                $(self.$field -= rhs.$field;)+
            }
        }
        impl core::ops::MulAssign<f32> for $Vec {
            #[inline]
            fn mul_assign(&mut self, rhs: f32) {
                $(self.$field *= rhs;)+
            }
        }
        impl core::ops::DivAssign<f32> for $Vec {
            #[inline]
            fn div_assign(&mut self, rhs: f32) {
                $(self.$field /= rhs;)+
            }
        }
    };
}

mod vec2;
mod vec3;
mod vec4;

pub use vec2::Vec2;
pub use vec3::Vec3;
pub use vec4::Vec4;
