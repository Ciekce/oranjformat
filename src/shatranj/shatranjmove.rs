use std::{
    fmt::{Debug, Display, Formatter},
    num::NonZeroU16,
};

use crate::shatranj::types::Square;

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Move {
    data: NonZeroU16,
}

const _: () = assert!(std::mem::size_of::<Move>() == std::mem::size_of::<Option<Move>>());

const PROMO_FLAG_BITS: u16 = 0b0001_0000_0000_0000;
const VALID_BITS: u16 = 0b0001_1111_1111_1111;

impl Move {
    const SQ_MASK: u16 = 0b11_1111;
    const TO_SHIFT: usize = 6;

    pub fn new_promo(from: Square, to: Square) -> Self {
        debug_assert!(u16::from(from) & Self::SQ_MASK == u16::from(from));
        debug_assert!(u16::from(to) & Self::SQ_MASK == u16::from(to));
        debug_assert_ne!(from, to);
        let data = u16::from(from) | (u16::from(to) << Self::TO_SHIFT) | PROMO_FLAG_BITS;
        // SAFETY: data is always OR-ed with the promo flag, which is non-zero, and so is always non-zero.
        let data = unsafe { NonZeroU16::new_unchecked(data) };
        Self { data }
    }

    pub fn new(from: Square, to: Square) -> Self {
        debug_assert!(u16::from(from) & Self::SQ_MASK == u16::from(from));
        debug_assert!(u16::from(to) & Self::SQ_MASK == u16::from(to));
        debug_assert_ne!(from, to);
        let data = u16::from(from) | (u16::from(to) << Self::TO_SHIFT);
        // SAFETY: this function is only called from within the movegen routines,
        // where we never create A1 -> A1 moves. This function is technically unsound
        // if called as Move::new(Square::A1, Square::A1).
        let data = unsafe { NonZeroU16::new_unchecked(data) };
        Self { data }
    }

    pub const fn from(self) -> Square {
        // SAFETY: SQ_MASK guarantees that this is in bounds.
        unsafe { Square::new_unchecked((self.data.get() & Self::SQ_MASK) as u8) }
    }

    pub const fn to(self) -> Square {
        // SAFETY: SQ_MASK guarantees that this is in bounds.
        unsafe {
            Square::new_unchecked(((self.data.get() >> Self::TO_SHIFT) & Self::SQ_MASK) as u8)
        }
    }

    pub const fn is_promo(self) -> bool {
        (self.data.get() & PROMO_FLAG_BITS) == PROMO_FLAG_BITS
    }

    pub fn is_valid(self) -> bool {
        (self.data.get() & !VALID_BITS) == 0
    }

    #[allow(dead_code)]
    pub const fn inner(self) -> u16 {
        self.data.get()
    }

    #[allow(dead_code)]
    pub fn from_raw(data: u16) -> Option<Self> {
        NonZeroU16::new(data).map(|nz| Self { data: nz })
    }
}

impl Display for Move {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        if self.is_promo() {
            write!(f, "{}{}q", self.from(), self.to())?;
        } else {
            write!(f, "{}{}", self.from(), self.to())?;
        }

        Ok(())
    }
}

impl Debug for Move {
    fn fmt(&self, f: &mut Formatter) -> Result<(), std::fmt::Error> {
        write!(
            f,
            "move from {} ({:?}) to {} ({:?}), ispromo {}",
            self.from(),
            self.from(),
            self.to(),
            self.to(),
            self.is_promo(),
        )
    }
}

mod tests {
    #[test]
    fn test_simple_move() {
        use super::*;
        let m = Move::new(Square::A1, Square::B2);
        println!("{m:?}");
        println!("bitpattern: {:016b}", m.data);
        assert_eq!(m.from(), Square::A1);
        assert_eq!(m.to(), Square::B2);
        assert!(!m.is_promo());
        assert!(m.is_valid());
    }

    #[test]
    fn test_promotion() {
        use super::*;
        let m = Move::new_promo(Square::A7, Square::A8);
        println!("{m:?}");
        println!("bitpattern: {:016b}", m.data);
        assert_eq!(m.from(), Square::A7);
        assert_eq!(m.to(), Square::A8);
        assert!(m.is_promo());
        assert!(m.is_valid());
    }

    #[test]
    fn test_all_square_combinations() {
        use super::*;
        use crate::shatranj::squareset::SquareSet;
        for from in SquareSet::FULL {
            for to in SquareSet::FULL.iter().filter(|s| *s < from) {
                let m = Move::new(from, to);
                assert_eq!(m.from(), from);
                assert_eq!(m.to(), to);
                assert!(!m.is_promo());
                assert!(m.is_valid());
            }
        }
    }
}
