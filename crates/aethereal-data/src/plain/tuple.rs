//! `()` and tuples of one to twelve elements as plain data (ADR-0004).

use crate::{Boundary, Decode, DecodeError, Encode, EncodeError, Plain, Schema, Shape};

impl Schema for () {
    const SHAPE: Shape = Shape::Tuple { elements: &[] };
}

impl<B: Boundary> Encode<B> for () {
    fn encode(&self, _out: &mut B::Writer<'_>) -> Result<(), EncodeError> {
        Ok(())
    }
}

impl<B: Boundary> Decode<B> for () {
    fn decode(_input: &mut B::Reader<'_>) -> Result<Self, DecodeError> {
        Ok(())
    }
}

impl Plain for () {}

macro_rules! tuple {
    ($(($t:ident $v:ident)),+) => {
        impl<$($t: Schema),+> Schema for ($($t,)+) {
            const SHAPE: Shape = Shape::Tuple { elements: &[$($t::SHAPE),+] };
        }

        impl<B: Boundary, $($t: Encode<B>),+> Encode<B> for ($($t,)+) {
            fn encode(&self, out: &mut B::Writer<'_>) -> Result<(), EncodeError> {
                let ($( $v, )+) = self;
                $($v.encode(out)?;)+
                Ok(())
            }
        }

        impl<B: Boundary, $($t: Decode<B>),+> Decode<B> for ($($t,)+) {
            fn decode(input: &mut B::Reader<'_>) -> Result<Self, DecodeError> {
                $(let $v = $t::decode(input)?;)+
                Ok(($($v,)+))
            }
        }

        impl<$($t: Plain),+> Plain for ($($t,)+) {}
    };
}

tuple!((T0 v0));
tuple!((T0 v0), (T1 v1));
tuple!((T0 v0), (T1 v1), (T2 v2));
tuple!((T0 v0), (T1 v1), (T2 v2), (T3 v3));
tuple!((T0 v0), (T1 v1), (T2 v2), (T3 v3), (T4 v4));
tuple!((T0 v0), (T1 v1), (T2 v2), (T3 v3), (T4 v4), (T5 v5));
tuple!((T0 v0), (T1 v1), (T2 v2), (T3 v3), (T4 v4), (T5 v5), (T6 v6));
tuple!((T0 v0), (T1 v1), (T2 v2), (T3 v3), (T4 v4), (T5 v5), (T6 v6), (T7 v7));
tuple!((T0 v0), (T1 v1), (T2 v2), (T3 v3), (T4 v4), (T5 v5), (T6 v6), (T7 v7), (T8 v8));
tuple!(
    (T0 v0),
    (T1 v1),
    (T2 v2),
    (T3 v3),
    (T4 v4),
    (T5 v5),
    (T6 v6),
    (T7 v7),
    (T8 v8),
    (T9 v9)
);
tuple!(
    (T0 v0),
    (T1 v1),
    (T2 v2),
    (T3 v3),
    (T4 v4),
    (T5 v5),
    (T6 v6),
    (T7 v7),
    (T8 v8),
    (T9 v9),
    (T10 v10)
);
tuple!(
    (T0 v0),
    (T1 v1),
    (T2 v2),
    (T3 v3),
    (T4 v4),
    (T5 v5),
    (T6 v6),
    (T7 v7),
    (T8 v8),
    (T9 v9),
    (T10 v10),
    (T11 v11)
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Budget, from_bytes, to_bytes};

    #[test]
    fn unit_has_no_bytes() {
        // Catches a unit codec that writes a marker byte.
        assert_eq!(to_bytes(&()), Ok(alloc::vec![]));
        assert_eq!(from_bytes::<()>(&[], &mut Budget::new(16)), Ok(()));
    }

    #[test]
    fn tuple_is_elements_in_order() {
        // Catches a tuple codec that reorders or counts its elements.
        assert_eq!(to_bytes(&(1u8, 0x0201u16)), Ok(alloc::vec![0x01, 0x01, 0x02]));
        assert_eq!(from_bytes::<(u8, u16)>(&[0x01, 0x01, 0x02], &mut Budget::new(16)), Ok((1, 0x0201)));
    }

    #[test]
    fn trailing_bytes_are_refused() {
        // Catches a decoder that stops after the value and ignores the rest.
        assert_eq!(from_bytes::<()>(&[0], &mut Budget::new(16)), Err(DecodeError::TrailingBytes { count: 1 }));
    }
}
