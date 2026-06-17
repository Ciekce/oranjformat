use arrayvec::ArrayVec;

use std::{
    fmt::{Display, Formatter},
    ops::{Deref, DerefMut},
};

use crate::{
    cfor,
    shatranj::{
        board::Board,
        magic::{ROOK_ATTACKS, ROOK_MAGICS, ROOK_MASKS, ROOK_REL_BITS},
        piece::{Black, Col, Colour, PieceType, White},
        shatranjmove::Move,
        squareset::SquareSet,
        types::Square,
    },
};

pub const MAX_POSITION_MOVES: usize = 218;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoveListEntry {
    pub mov: Move,
    pub score: i32,
}

impl MoveListEntry {
    pub const TACTICAL_SENTINEL: i32 = 0x7FFF_FFFF;
    pub const QUIET_SENTINEL: i32 = 0x7FFF_FFFE;
}

#[derive(Clone)]
pub struct MoveList {
    // moves: [MoveListEntry; MAX_POSITION_MOVES],
    // count: usize,
    inner: ArrayVec<MoveListEntry, MAX_POSITION_MOVES>,
}

impl Default for MoveList {
    fn default() -> Self {
        Self::new()
    }
}

impl MoveList {
    pub fn new() -> Self {
        Self {
            inner: ArrayVec::new(),
        }
    }

    fn push<const TACTICAL: bool>(&mut self, m: Move) {
        // debug_assert!(self.count < MAX_POSITION_MOVES, "overflowed {self}");
        let score = if TACTICAL {
            MoveListEntry::TACTICAL_SENTINEL
        } else {
            MoveListEntry::QUIET_SENTINEL
        };

        self.inner.push(MoveListEntry { mov: m, score });
    }

    pub fn iter_moves(&self) -> impl Iterator<Item = &Move> {
        self.inner.iter().map(|e| &e.mov)
    }

    pub fn clear(&mut self) {
        self.inner.clear();
    }
}

impl Deref for MoveList {
    type Target = [MoveListEntry];

    fn deref(&self) -> &[MoveListEntry] {
        &self.inner
    }
}

impl DerefMut for MoveList {
    fn deref_mut(&mut self) -> &mut [MoveListEntry] {
        &mut self.inner
    }
}

impl Display for MoveList {
    fn fmt(&self, f: &mut Formatter) -> Result<(), std::fmt::Error> {
        if self.inner.is_empty() {
            return write!(f, "MoveList: (0) []");
        }
        writeln!(f, "MoveList: ({}) [", self.inner.len())?;
        for m in &self.inner[0..self.inner.len() - 1] {
            writeln!(f, "  {} ${}, ", m.mov, m.score)?;
        }
        writeln!(
            f,
            "  {} ${}",
            self.inner[self.inner.len() - 1].mov,
            self.inner[self.inner.len() - 1].score
        )?;
        write!(f, "]")
    }
}

const fn in_between(sq1: Square, sq2: Square) -> SquareSet {
    const M1: u64 = 0xFFFF_FFFF_FFFF_FFFF;
    const A2A7: u64 = 0x0001_0101_0101_0100;
    const B2G7: u64 = 0x0040_2010_0804_0200;
    const H1B7: u64 = 0x0002_0408_1020_4080;
    let sq1 = sq1.index();
    let sq2 = sq2.index();
    let btwn = (M1 << sq1) ^ (M1 << sq2);
    let file = ((sq2 & 7).wrapping_add((sq1 & 7).wrapping_neg())) as u64;
    let rank = (((sq2 | 7).wrapping_sub(sq1)) >> 3) as u64;
    let mut line = ((file & 7).wrapping_sub(1)) & A2A7;
    line += 2 * ((rank & 7).wrapping_sub(1) >> 58);
    line += ((rank.wrapping_sub(file) & 15).wrapping_sub(1)) & B2G7;
    line += ((rank.wrapping_add(file) & 15).wrapping_sub(1)) & H1B7;
    line = line.wrapping_mul(btwn & btwn.wrapping_neg());
    SquareSet::from_inner(line & btwn)
}

pub static RAY_BETWEEN: [[SquareSet; 64]; 64] = {
    let mut res = [[SquareSet::EMPTY; 64]; 64];
    let mut from = Square::A1;
    loop {
        let mut to = Square::A1;
        loop {
            res[from.index()][to.index()] = in_between(from, to);
            let Some(next) = to.add(1) else {
                break;
            };
            to = next;
        }
        let Some(next) = from.add(1) else {
            break;
        };
        from = next;
    }
    res
};

// alfil and ferz
const fn init_jumping_attacks4<const IS_FERZ: bool>() -> [SquareSet; 64] {
    let mut attacks = [SquareSet::EMPTY; 64];
    let deltas = if IS_FERZ {
        &[9, 7, -7, -9]
    } else {
        &[18, 14, -14, -18]
    };

    cfor!(let mut sq = Square::A1; true; sq = sq.saturating_add(1); {
        let mut attacks_bb = 0;
        cfor!(let mut idx = 0; idx < 4; idx += 1; {
            let delta = deltas[idx];
            let attacked_sq = sq.signed_inner() + delta;
            #[allow(clippy::cast_sign_loss)]
            if 0 <= attacked_sq && attacked_sq < 64 && Square::distance(
                sq,
                Square::new_clamped(attacked_sq as u8)) <= 2 {
                attacks_bb |= 1 << attacked_sq;
            }
        });
        attacks[sq.index()] = SquareSet::from_inner(attacks_bb);
        if matches!(sq, Square::H8) {
            break;
        }
    });

    attacks
}

// king and knight
const fn init_jumping_attacks8<const IS_KNIGHT: bool>() -> [SquareSet; 64] {
    let mut attacks = [SquareSet::EMPTY; 64];
    let deltas = if IS_KNIGHT {
        &[17, 15, 10, 6, -17, -15, -10, -6]
    } else {
        &[9, 8, 7, 1, -9, -8, -7, -1]
    };

    cfor!(let mut sq = Square::A1; true; sq = sq.saturating_add(1); {
        let mut attacks_bb = 0;
        cfor!(let mut idx = 0; idx < 8; idx += 1; {
            let delta = deltas[idx];
            let attacked_sq = sq.signed_inner() + delta;
            #[allow(clippy::cast_sign_loss)]
            if 0 <= attacked_sq && attacked_sq < 64 && Square::distance(
                sq,
                Square::new_clamped(attacked_sq as u8)) <= 2 {
                attacks_bb |= 1 << attacked_sq;
            }
        });
        attacks[sq.index()] = SquareSet::from_inner(attacks_bb);
        if matches!(sq, Square::H8) {
            break;
        }
    });

    attacks
}

#[allow(clippy::cast_possible_truncation)]
pub fn rook_attacks(sq: Square, blockers: SquareSet) -> SquareSet {
    let relevant_blockers = blockers & ROOK_MASKS[sq];
    let data = relevant_blockers.inner().wrapping_mul(ROOK_MAGICS[sq]);
    let idx = (data >> (64 - ROOK_REL_BITS[sq])) as usize;
    // SAFETY: ROOK_REL_BITS[sq] is at most 12, so this shift is at least by 52.
    // The largest value we can obtain from (data >> 52) is u64::MAX >> 52, which
    // is 4095 (0xFFF). ROOK_ATTACKS[sq] is 4096 elements long, so this is always
    // in bounds.
    unsafe {
        if idx >= ROOK_ATTACKS[sq].len() {
            // assert to the compiler that it's chill not to bounds-check
            std::hint::unreachable_unchecked();
        }
        ROOK_ATTACKS[sq][idx]
    }
}
pub fn alfil_attacks(sq: Square) -> SquareSet {
    static ALFIL_ATTACKS: [SquareSet; 64] = init_jumping_attacks4::<false>();
    ALFIL_ATTACKS[sq]
}
pub fn ferz_attacks(sq: Square) -> SquareSet {
    static FERZ_ATTACKS: [SquareSet; 64] = init_jumping_attacks4::<true>();
    FERZ_ATTACKS[sq]
}
pub fn knight_attacks(sq: Square) -> SquareSet {
    static KNIGHT_ATTACKS: [SquareSet; 64] = init_jumping_attacks8::<true>();
    KNIGHT_ATTACKS[sq]
}
pub fn king_attacks(sq: Square) -> SquareSet {
    static KING_ATTACKS: [SquareSet; 64] = init_jumping_attacks8::<false>();
    KING_ATTACKS[sq]
}
pub fn pawn_attacks<C: Col>(bb: SquareSet) -> SquareSet {
    if C::WHITE {
        bb.north_east_one() | bb.north_west_one()
    } else {
        bb.south_east_one() | bb.south_west_one()
    }
}

pub fn attacks_by_type(pt: PieceType, sq: Square, blockers: SquareSet) -> SquareSet {
    match pt {
        PieceType::Alfil => alfil_attacks(sq),
        PieceType::Rook => rook_attacks(sq, blockers),
        PieceType::Ferz => ferz_attacks(sq),
        PieceType::Knight => knight_attacks(sq),
        PieceType::King => king_attacks(sq),
        PieceType::Pawn => panic!("Invalid piece type: {pt:?}"),
    }
}

pub trait MoveGenMode {
    const SKIP_QUIETS: bool;
}

pub struct SkipQuiets;
impl MoveGenMode for SkipQuiets {
    const SKIP_QUIETS: bool = true;
}
pub struct AllMoves;
impl MoveGenMode for AllMoves {
    const SKIP_QUIETS: bool = false;
}

impl Board {
    fn generate_pawn_caps<C: Col, Mode: MoveGenMode>(
        &self,
        move_list: &mut MoveList,
        valid_target_squares: SquareSet,
    ) {
        let our_pawns = self.pieces.pawns::<C>();
        let their_pieces = self.pieces.their_pieces::<C>();
        // to determine which pawns can capture, we shift the opponent's pieces backwards and find the intersection
        let attacking_west = if C::WHITE {
            their_pieces.south_east_one() & our_pawns
        } else {
            their_pieces.north_east_one() & our_pawns
        };
        let attacking_east = if C::WHITE {
            their_pieces.south_west_one() & our_pawns
        } else {
            their_pieces.north_west_one() & our_pawns
        };
        let valid_west = if C::WHITE {
            valid_target_squares.south_east_one()
        } else {
            valid_target_squares.north_east_one()
        };
        let valid_east = if C::WHITE {
            valid_target_squares.south_west_one()
        } else {
            valid_target_squares.north_west_one()
        };
        let promo_rank = if C::WHITE {
            SquareSet::RANK_7
        } else {
            SquareSet::RANK_2
        };
        let from_mask = attacking_west & !promo_rank & valid_west;
        let to_mask = if C::WHITE {
            from_mask.north_west_one()
        } else {
            from_mask.south_west_one()
        };
        for (from, to) in from_mask.into_iter().zip(to_mask) {
            move_list.push::<true>(Move::new(from, to));
        }
        let from_mask = attacking_east & !promo_rank & valid_east;
        let to_mask = if C::WHITE {
            from_mask.north_east_one()
        } else {
            from_mask.south_east_one()
        };
        for (from, to) in from_mask.into_iter().zip(to_mask) {
            move_list.push::<true>(Move::new(from, to));
        }
        let from_mask = attacking_west & promo_rank & valid_west;
        let to_mask = if C::WHITE {
            from_mask.north_west_one()
        } else {
            from_mask.south_west_one()
        };
        for (from, to) in from_mask.into_iter().zip(to_mask) {
            move_list.push::<true>(Move::new_promo(from, to));
        }
        let from_mask = attacking_east & promo_rank & valid_east;
        let to_mask = if C::WHITE {
            from_mask.north_east_one()
        } else {
            from_mask.south_east_one()
        };
        for (from, to) in from_mask.into_iter().zip(to_mask) {
            move_list.push::<true>(Move::new_promo(from, to));
        }
    }

    fn generate_pawn_forward<C: Col>(
        &self,
        move_list: &mut MoveList,
        valid_target_squares: SquareSet,
    ) {
        let promo_rank = if C::WHITE {
            SquareSet::RANK_7
        } else {
            SquareSet::RANK_2
        };
        let shifted_empty_squares = if C::WHITE {
            self.pieces.empty() >> 8
        } else {
            self.pieces.empty() << 8
        };
        let shifted_valid_squares = if C::WHITE {
            valid_target_squares >> 8
        } else {
            valid_target_squares << 8
        };
        let our_pawns = self.pieces.pawns::<C>();
        let pushable_pawns = our_pawns & shifted_empty_squares;
        let promoting_pawns = pushable_pawns & promo_rank;

        let from_mask = pushable_pawns & !promoting_pawns & shifted_valid_squares;
        let to_mask = if C::WHITE {
            from_mask.north_one()
        } else {
            from_mask.south_one()
        };
        for (from, to) in from_mask.into_iter().zip(to_mask) {
            move_list.push::<false>(Move::new(from, to));
        }
        let from_mask = promoting_pawns & shifted_valid_squares;
        let to_mask = if C::WHITE {
            from_mask.north_one()
        } else {
            from_mask.south_one()
        };
        for (from, to) in from_mask.into_iter().zip(to_mask) {
            move_list.push::<true>(Move::new_promo(from, to));
        }
    }

    fn generate_forward_promos<C: Col, Mode: MoveGenMode>(
        &self,
        move_list: &mut MoveList,
        valid_target_squares: SquareSet,
    ) {
        let promo_rank = if C::WHITE {
            SquareSet::RANK_7
        } else {
            SquareSet::RANK_2
        };
        let shifted_empty_squares = if C::WHITE {
            self.pieces.empty() >> 8
        } else {
            self.pieces.empty() << 8
        };
        let shifted_valid_squares = if C::WHITE {
            valid_target_squares >> 8
        } else {
            valid_target_squares << 8
        };
        let our_pawns = self.pieces.pawns::<C>();
        let pushable_pawns = our_pawns & shifted_empty_squares;
        let promoting_pawns = pushable_pawns & promo_rank;

        let from_mask = promoting_pawns & shifted_valid_squares;
        let to_mask = if C::WHITE {
            from_mask.north_one()
        } else {
            from_mask.south_one()
        };
        for (from, to) in from_mask.into_iter().zip(to_mask) {
            move_list.push::<true>(Move::new_promo(from, to));
        }
    }

    pub fn generate_moves(&self, move_list: &mut MoveList) {
        move_list.clear();
        if self.side == Colour::White {
            self.generate_moves_for::<White>(move_list);
        } else {
            self.generate_moves_for::<Black>(move_list);
        }
        debug_assert!(move_list.iter_moves().all(|m| m.is_valid()));
    }

    fn generate_moves_for<C: Col>(&self, move_list: &mut MoveList) {
        #[cfg(debug_assertions)]
        self.check_validity().unwrap();

        let their_pieces = self.pieces.their_pieces::<C>();
        let freespace = self.pieces.empty();
        let our_king_sq = self.pieces.king::<C>().first();

        if self.threats.checkers.count() > 1 {
            // we're in double-check, so we can only move the king.
            let moves = king_attacks(our_king_sq) & !self.threats.all;
            for to in moves & their_pieces {
                move_list.push::<true>(Move::new(our_king_sq, to));
            }
            for to in moves & freespace {
                move_list.push::<false>(Move::new(our_king_sq, to));
            }
            return;
        }

        let valid_target_squares = if self.in_check() {
            RAY_BETWEEN[our_king_sq][self.threats.checkers.first()] | self.threats.checkers
        } else {
            SquareSet::FULL
        };

        self.generate_pawn_forward::<C>(move_list, valid_target_squares);
        self.generate_pawn_caps::<C, AllMoves>(move_list, valid_target_squares);

        // alfils
        let our_alfils = self.pieces.alfils::<C>();
        for sq in our_alfils {
            let moves = alfil_attacks(sq) & valid_target_squares;
            for to in moves & their_pieces {
                move_list.push::<true>(Move::new(sq, to));
            }
            for to in moves & freespace {
                move_list.push::<false>(Move::new(sq, to));
            }
        }

        // ferzes
        let our_ferzes = self.pieces.ferzes::<C>();
        for sq in our_ferzes {
            let moves = ferz_attacks(sq) & valid_target_squares;
            for to in moves & their_pieces {
                move_list.push::<true>(Move::new(sq, to));
            }
            for to in moves & freespace {
                move_list.push::<false>(Move::new(sq, to));
            }
        }

        // knights
        let our_knights = self.pieces.knights::<C>();
        for sq in our_knights {
            let moves = knight_attacks(sq) & valid_target_squares;
            for to in moves & their_pieces {
                move_list.push::<true>(Move::new(sq, to));
            }
            for to in moves & freespace {
                move_list.push::<false>(Move::new(sq, to));
            }
        }

        // kings
        let moves = king_attacks(our_king_sq) & !self.threats.all;
        for to in moves & their_pieces {
            move_list.push::<true>(Move::new(our_king_sq, to));
        }
        for to in moves & freespace {
            move_list.push::<false>(Move::new(our_king_sq, to));
        }

        // rooks
        let blockers = self.pieces.occupied();
        let our_rooks = self.pieces.rooks::<C>();
        for sq in our_rooks {
            let moves = rook_attacks(sq, blockers) & valid_target_squares;
            for to in moves & their_pieces {
                move_list.push::<true>(Move::new(sq, to));
            }
            for to in moves & freespace {
                move_list.push::<false>(Move::new(sq, to));
            }
        }
    }

    pub fn generate_captures<Mode: MoveGenMode>(&self, move_list: &mut MoveList) {
        move_list.clear();
        if self.side == Colour::White {
            self.generate_captures_for::<White, Mode>(move_list);
        } else {
            self.generate_captures_for::<Black, Mode>(move_list);
        }
        debug_assert!(move_list.iter_moves().all(|m| m.is_valid()));
    }

    fn generate_captures_for<C: Col, Mode: MoveGenMode>(&self, move_list: &mut MoveList) {
        #[cfg(debug_assertions)]
        self.check_validity().unwrap();

        let their_pieces = self.pieces.their_pieces::<C>();
        let our_king_sq = self.pieces.king::<C>().first();

        if self.threats.checkers.count() > 1 {
            // we're in double-check, so we can only move the king.
            let moves = king_attacks(our_king_sq) & !self.threats.all;
            for to in moves & their_pieces {
                move_list.push::<true>(Move::new(our_king_sq, to));
            }
            return;
        }

        let valid_target_squares = if self.in_check() {
            RAY_BETWEEN[our_king_sq][self.threats.checkers.first()] | self.threats.checkers
        } else {
            SquareSet::FULL
        };

        // promotions
        self.generate_forward_promos::<C, Mode>(move_list, valid_target_squares);

        // pawn captures and capture promos
        self.generate_pawn_caps::<C, Mode>(move_list, valid_target_squares);

        // alfils
        let our_alfils = self.pieces.alfils::<C>();
        for sq in our_alfils {
            let moves = alfil_attacks(sq) & valid_target_squares;
            for to in moves & their_pieces {
                move_list.push::<true>(Move::new(sq, to));
            }
        }

        // ferzes
        let our_ferzes = self.pieces.ferzes::<C>();
        for sq in our_ferzes {
            let moves = ferz_attacks(sq) & valid_target_squares;
            for to in moves & their_pieces {
                move_list.push::<true>(Move::new(sq, to));
            }
        }

        // knights
        let our_knights = self.pieces.knights::<C>();
        let their_pieces = self.pieces.their_pieces::<C>();
        for sq in our_knights {
            let moves = knight_attacks(sq) & valid_target_squares;
            for to in moves & their_pieces {
                move_list.push::<true>(Move::new(sq, to));
            }
        }

        // kings
        let moves = king_attacks(our_king_sq) & !self.threats.all;
        for to in moves & their_pieces {
            move_list.push::<true>(Move::new(our_king_sq, to));
        }

        // rooks
        let our_rooks = self.pieces.rooks::<C>();
        let blockers = self.pieces.occupied();
        for sq in our_rooks {
            let moves = rook_attacks(sq, blockers) & valid_target_squares;
            for to in moves & their_pieces {
                move_list.push::<true>(Move::new(sq, to));
            }
        }
    }

    pub fn generate_quiets(&self, move_list: &mut MoveList) {
        // we don't need to clear the move list here because we're only adding to it.
        if self.side == Colour::White {
            self.generate_quiets_for::<White>(move_list);
        } else {
            self.generate_quiets_for::<Black>(move_list);
        }
        debug_assert!(move_list.iter_moves().all(|m| m.is_valid()));
    }

    fn generate_pawn_quiet<C: Col>(
        &self,
        move_list: &mut MoveList,
        valid_target_squares: SquareSet,
    ) {
        let start_rank = if C::WHITE {
            SquareSet::RANK_2
        } else {
            SquareSet::RANK_7
        };
        let promo_rank = if C::WHITE {
            SquareSet::RANK_7
        } else {
            SquareSet::RANK_2
        };
        let shifted_empty_squares = if C::WHITE {
            self.pieces.empty() >> 8
        } else {
            self.pieces.empty() << 8
        };
        let double_shifted_empty_squares = if C::WHITE {
            self.pieces.empty() >> 16
        } else {
            self.pieces.empty() << 16
        };
        let shifted_valid_squares = if C::WHITE {
            valid_target_squares >> 8
        } else {
            valid_target_squares << 8
        };
        let double_shifted_valid_squares = if C::WHITE {
            valid_target_squares >> 16
        } else {
            valid_target_squares << 16
        };
        let our_pawns = self.pieces.pawns::<C>();
        let pushable_pawns = our_pawns & shifted_empty_squares;
        let double_pushable_pawns = pushable_pawns & double_shifted_empty_squares & start_rank;
        let promoting_pawns = pushable_pawns & promo_rank;

        let from_mask = pushable_pawns & !promoting_pawns & shifted_valid_squares;
        let to_mask = if C::WHITE {
            from_mask.north_one()
        } else {
            from_mask.south_one()
        };
        for (from, to) in from_mask.into_iter().zip(to_mask) {
            move_list.push::<false>(Move::new(from, to));
        }
        let from_mask = double_pushable_pawns & double_shifted_valid_squares;
        let to_mask = if C::WHITE {
            from_mask.north_one().north_one()
        } else {
            from_mask.south_one().south_one()
        };
        for (from, to) in from_mask.into_iter().zip(to_mask) {
            move_list.push::<false>(Move::new(from, to));
        }
    }

    fn generate_quiets_for<C: Col>(&self, move_list: &mut MoveList) {
        let freespace = self.pieces.empty();
        let our_king_sq = self.pieces.king::<C>().first();
        let blockers = self.pieces.occupied();

        if self.threats.checkers.count() > 1 {
            // we're in double-check, so we can only move the king.
            let moves = king_attacks(our_king_sq) & !self.threats.all;
            for to in moves & freespace {
                move_list.push::<false>(Move::new(our_king_sq, to));
            }
            return;
        }

        let valid_target_squares = if self.in_check() {
            RAY_BETWEEN[our_king_sq][self.threats.checkers.first()] | self.threats.checkers
        } else {
            SquareSet::FULL
        };

        // pawns
        self.generate_pawn_quiet::<C>(move_list, valid_target_squares);

        // alfils
        let our_alfils = self.pieces.alfils::<C>();
        for sq in our_alfils {
            let moves = alfil_attacks(sq) & valid_target_squares;
            for to in moves & !blockers {
                move_list.push::<false>(Move::new(sq, to));
            }
        }

        // alfils
        let our_ferzes = self.pieces.ferzes::<C>();
        for sq in our_ferzes {
            let moves = ferz_attacks(sq) & valid_target_squares;
            for to in moves & !blockers {
                move_list.push::<false>(Move::new(sq, to));
            }
        }

        // knights
        let our_knights = self.pieces.knights::<C>();
        for sq in our_knights {
            let moves = knight_attacks(sq) & valid_target_squares;
            for to in moves & !blockers {
                move_list.push::<false>(Move::new(sq, to));
            }
        }

        // kings
        let moves = king_attacks(our_king_sq) & !self.threats.all;
        for to in moves & !blockers {
            move_list.push::<false>(Move::new(our_king_sq, to));
        }

        // rooks
        let our_rooks = self.pieces.rooks::<C>();
        for sq in our_rooks {
            let moves = rook_attacks(sq, blockers) & valid_target_squares;
            for to in moves & !blockers {
                move_list.push::<false>(Move::new(sq, to));
            }
        }
    }
}

#[cfg(test)]
pub fn synced_perft(pos: &mut Board, depth: usize) -> u64 {
    #![allow(clippy::to_string_in_format_args)]
    #[cfg(debug_assertions)]
    pos.check_validity().unwrap();

    if depth == 0 {
        return 1;
    }

    let mut ml = MoveList::new();
    pos.generate_moves(&mut ml);
    let mut ml_staged = MoveList::new();
    pos.generate_captures::<AllMoves>(&mut ml_staged);
    pos.generate_quiets(&mut ml_staged);

    let mut full_moves_vec = ml.to_vec();
    let mut staged_moves_vec = ml_staged.to_vec();
    full_moves_vec.sort_unstable_by_key(|m| m.mov);
    staged_moves_vec.sort_unstable_by_key(|m| m.mov);
    let eq = full_moves_vec == staged_moves_vec;
    assert!(
        eq,
        "full and staged move lists differ in {}, \nfull list: \n[{}], \nstaged list: \n[{}]",
        pos.to_string(),
        {
            let mut mvs = Vec::new();
            for m in full_moves_vec {
                mvs.push(format!(
                    "{}{}",
                    pos.san(m.mov).unwrap(),
                    if m.score == MoveListEntry::TACTICAL_SENTINEL {
                        "T"
                    } else {
                        "Q"
                    }
                ));
            }
            mvs.join(", ")
        },
        {
            let mut mvs = Vec::new();
            for m in staged_moves_vec {
                mvs.push(format!(
                    "{}{}",
                    pos.san(m.mov).unwrap(),
                    if m.score == MoveListEntry::TACTICAL_SENTINEL {
                        "T"
                    } else {
                        "Q"
                    }
                ));
            }
            mvs.join(", ")
        }
    );

    let mut count = 0;
    for &m in ml.iter_moves() {
        if !pos.make_move_simple(m) {
            continue;
        }
        count += synced_perft(pos, depth - 1);
        pos.unmake_move_base();
    }

    count
}

#[cfg(test)]
mod tests {
    use crate::shatranj::{
        board::movegen::{king_attacks, knight_attacks},
        squareset::SquareSet,
        types::Square,
    };

    #[test]
    fn python_chess_validation() {
        // testing that the attack squaresets match the ones in the python-chess library,
        // which are known to be correct.
        assert_eq!(
            knight_attacks(Square::new(0).unwrap()),
            SquareSet::from_inner(132_096)
        );
        assert_eq!(
            knight_attacks(Square::new(63).unwrap()),
            SquareSet::from_inner(9_077_567_998_918_656)
        );

        assert_eq!(
            king_attacks(Square::new(0).unwrap()),
            SquareSet::from_inner(770)
        );
        assert_eq!(
            king_attacks(Square::new(63).unwrap()),
            SquareSet::from_inner(4_665_729_213_955_833_856)
        );
    }

    #[test]
    fn ray_test() {
        use super::{RAY_BETWEEN, Square};
        use crate::shatranj::squareset::SquareSet;
        assert_eq!(RAY_BETWEEN[Square::A1][Square::A1], SquareSet::EMPTY);
        assert_eq!(RAY_BETWEEN[Square::A1][Square::B1], SquareSet::EMPTY);
        assert_eq!(RAY_BETWEEN[Square::A1][Square::C1], Square::B1.as_set());
        assert_eq!(
            RAY_BETWEEN[Square::A1][Square::D1],
            Square::B1.as_set() | Square::C1.as_set()
        );
        assert_eq!(RAY_BETWEEN[Square::B1][Square::D1], Square::C1.as_set());
        assert_eq!(RAY_BETWEEN[Square::D1][Square::B1], Square::C1.as_set());

        for from in Square::all() {
            for to in Square::all() {
                assert_eq!(RAY_BETWEEN[from][to], RAY_BETWEEN[to][from]);
            }
        }
    }

    #[test]
    fn ray_diag_test() {
        use super::{RAY_BETWEEN, Square};
        let ray = RAY_BETWEEN[Square::B5][Square::E8];
        assert_eq!(ray, Square::C6.as_set() | Square::D7.as_set());
    }
}
