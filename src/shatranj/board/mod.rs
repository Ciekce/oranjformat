pub mod movegen;
pub mod validation;

use std::fmt::{Debug, Display, Formatter};

use anyhow::{Context, bail};

use arrayvec::ArrayVec;
use rand::{rngs::ThreadRng, seq::IndexedRandom};

use crate::{
    makemove::{hash_piece, hash_side},
    shatranj::{
        board::movegen::{
            MoveList, alfil_attacks, ferz_attacks, king_attacks, knight_attacks, pawn_attacks,
            rook_attacks,
        },
        piece::{Black, Col, Colour, Piece, PieceType, White},
        shatranjmove::Move,
        squareset::SquareSet,
        types::{CheckState, File, Rank, Square, Undo},
    },
};

use crate::shatranj::piecelayout::{PieceLayout, Threats};

#[derive(Clone, Copy, Debug)]
pub struct MovedPiece {
    pub from: Square,
    pub to: Square,
    pub piece: Piece,
}

/// Struct representing some unmaterialised feature update made as part of a move.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct FeatureUpdate {
    pub sq: Square,
    pub piece: Piece,
}

impl Display for FeatureUpdate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{piece} on {sq}", piece = self.piece, sq = self.sq)
    }
}

#[derive(PartialEq, Eq, Clone, Debug, Default)]
pub struct UpdateBuffer {
    add: ArrayVec<FeatureUpdate, 2>,
    sub: ArrayVec<FeatureUpdate, 2>,
}

impl UpdateBuffer {
    pub fn move_piece(&mut self, from: Square, to: Square, piece: Piece) {
        self.add.push(FeatureUpdate { sq: to, piece });
        self.sub.push(FeatureUpdate { sq: from, piece });
    }

    pub fn clear_piece(&mut self, sq: Square, piece: Piece) {
        self.sub.push(FeatureUpdate { sq, piece });
    }

    pub fn add_piece(&mut self, sq: Square, piece: Piece) {
        self.add.push(FeatureUpdate { sq, piece });
    }

    pub fn adds(&self) -> &[FeatureUpdate] {
        &self.add[..]
    }

    pub fn subs(&self) -> &[FeatureUpdate] {
        &self.sub[..]
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct Board {
    /// The square-sets of all the pieces on the board.
    pub pieces: PieceLayout,
    /// An array to accelerate `Board::piece_at()`.
    pub piece_array: [Option<Piece>; 64],
    /// The side to move.
    side: Colour,
    /// The number of half moves made since the last capture or pawn advance.
    seventy_move_counter: u8,
    /// The number of half moves made since the start of the game.
    ply: usize,

    /// The Zobrist hash of the board.
    key: u64,
    /// The Zobrist hash of the pawns on the board.
    pawn_key: u64,
    /// The Zobrist hash of the non-pawns on the board, split by side.
    non_pawn_key: [u64; 2],
    /// The Zobrist hash of the minor pieces on the board.
    minor_key: u64,
    /// The Zobrist hash of the major pieces on the board.
    major_key: u64,

    /// Squares that the opponent attacks.
    threats: Threats,

    height: usize,
    history: Vec<Undo>,
}

impl Debug for Board {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Board")
            .field("piece_array", &self.piece_array)
            .field("side", &self.side)
            .field("seventy_move_counter", &self.seventy_move_counter)
            .field("height", &self.height)
            .field("ply", &self.ply)
            .field("key", &self.key)
            .field("threats", &self.threats)
            .finish_non_exhaustive()
    }
}

impl Board {
    pub const STARTING_FEN: &'static str = "rnbkqbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBKQBNR w - - 0 1";

    pub fn new() -> Self {
        let mut out = Self {
            pieces: PieceLayout::NULL,
            piece_array: [None; 64],
            side: Colour::White,
            seventy_move_counter: 0,
            height: 0,
            ply: 0,
            key: 0,
            pawn_key: 0,
            non_pawn_key: [0; 2],
            minor_key: 0,
            major_key: 0,
            threats: Threats::default(),
            history: Vec::new(),
        };
        out.reset();
        out
    }

    pub fn to_bulletformat(
        &self,
        wdl: u8,
        eval: i16,
    ) -> Result<bulletformat::ChessBoard, anyhow::Error> {
        let mut bbs = [0; 8];
        let piece_layout = &self.pieces;
        bbs[0] = piece_layout.occupied_co(Colour::White).inner();
        bbs[1] = piece_layout.occupied_co(Colour::Black).inner();
        bbs[2] = piece_layout.of_type(PieceType::Pawn).inner();
        bbs[3] = piece_layout.of_type(PieceType::Alfil).inner();
        bbs[4] = piece_layout.of_type(PieceType::Ferz).inner();
        bbs[5] = piece_layout.of_type(PieceType::Knight).inner();
        bbs[6] = piece_layout.of_type(PieceType::Rook).inner();
        bbs[7] = piece_layout.of_type(PieceType::King).inner();
        let bulletformat = bulletformat::ChessBoard::from_raw(
            bbs,
            (self.turn() != Colour::White).into(),
            eval,
            f32::from(wdl) / 2.0,
        )
        .map_err(|e| anyhow::anyhow!(e))
        .with_context(|| "Failed to convert raw components into bulletformat::ChessBoard.")?;
        Ok(bulletformat)
    }

    pub fn turn_mut(&mut self) -> &mut Colour {
        &mut self.side
    }

    pub fn halfmove_clock_mut(&mut self) -> &mut u8 {
        &mut self.seventy_move_counter
    }

    pub fn set_fullmove_clock(&mut self, fullmove_clock: u16) {
        self.ply = (fullmove_clock as usize - 1) * 2 + usize::from(self.side == Colour::Black);
    }

    pub const fn zobrist_key(&self) -> u64 {
        self.key
    }

    pub const fn pawn_key(&self) -> u64 {
        self.pawn_key
    }

    pub fn non_pawn_key(&self, colour: Colour) -> u64 {
        self.non_pawn_key[colour]
    }

    pub const fn minor_key(&self) -> u64 {
        self.minor_key
    }

    pub const fn major_key(&self) -> u64 {
        self.major_key
    }

    #[cfg(debug_assertions)]
    pub const fn all_keys(&self) -> (u64, u64, [u64; 2], u64, u64) {
        (
            self.key,
            self.pawn_key,
            self.non_pawn_key,
            self.minor_key,
            self.major_key,
        )
    }

    pub fn n_men(&self) -> u8 {
        #![allow(clippy::cast_possible_truncation)]
        self.pieces.occupied().count() as u8
    }

    pub const fn ply(&self) -> usize {
        self.ply
    }

    pub const fn threats(&self) -> &Threats {
        &self.threats
    }

    pub fn king_sq(&self, side: Colour) -> Square {
        debug_assert!(side == Colour::White || side == Colour::Black);
        debug_assert_eq!(self.pieces.king::<White>().count(), 1);
        debug_assert_eq!(self.pieces.king::<Black>().count(), 1);
        let sq = match side {
            Colour::White => self.pieces.king::<White>().first(),
            Colour::Black => self.pieces.king::<Black>().first(),
        };
        debug_assert_eq!(self.pieces.piece_at(sq).unwrap().colour(), side);
        debug_assert_eq!(
            self.pieces.piece_at(sq).unwrap().piece_type(),
            PieceType::King
        );
        sq
    }

    pub const fn in_check(&self) -> bool {
        self.threats.checkers.non_empty()
    }

    pub fn zero_height(&mut self) {
        self.height = 0;
    }

    pub const fn height(&self) -> usize {
        self.height
    }

    pub const fn turn(&self) -> Colour {
        self.side
    }

    pub fn generate_pos_keys(&self) -> (u64, u64, [u64; 2], u64, u64) {
        let mut key = 0;
        let mut pawn_key = 0;
        let mut non_pawn_key = [0; 2];
        let mut minor_key = 0;
        let mut major_key = 0;
        self.pieces.visit_pieces(|sq, piece| {
            hash_piece(&mut key, piece, sq);
            if piece.piece_type() == PieceType::Pawn {
                hash_piece(&mut pawn_key, piece, sq);
            } else {
                hash_piece(&mut non_pawn_key[piece.colour()], piece, sq);
                if piece.piece_type() == PieceType::King {
                    hash_piece(&mut major_key, piece, sq);
                    hash_piece(&mut minor_key, piece, sq);
                } else if matches!(piece.piece_type(), PieceType::Ferz | PieceType::Rook) {
                    hash_piece(&mut major_key, piece, sq);
                } else {
                    hash_piece(&mut minor_key, piece, sq);
                }
            }
        });

        if self.side == Colour::White {
            hash_side(&mut key);
        }

        debug_assert!(self.seventy_move_counter <= 140);

        (key, pawn_key, non_pawn_key, minor_key, major_key)
    }

    pub fn regenerate_zobrist(&mut self) {
        (
            self.key,
            self.pawn_key,
            self.non_pawn_key,
            self.minor_key,
            self.major_key,
        ) = self.generate_pos_keys();
    }

    pub fn regenerate_threats(&mut self) {
        self.threats = self.generate_threats(self.side.flip());
    }

    pub fn generate_threats(&self, side: Colour) -> Threats {
        if side == Colour::White {
            self.generate_threats_from::<White>()
        } else {
            self.generate_threats_from::<Black>()
        }
    }

    pub fn generate_threats_from<C: Col>(&self) -> Threats {
        let mut threats = SquareSet::EMPTY;
        let mut checkers = SquareSet::EMPTY;

        let their_pawns = self.pieces.pawns::<C>();
        let their_alfils = self.pieces.alfils::<C>();
        let their_ferzes = self.pieces.ferzes::<C>();
        let their_knights = self.pieces.knights::<C>();
        let their_rooks = self.pieces.rooks::<C>();
        let their_king = self.king_sq(C::COLOUR);
        let blockers = self.pieces.occupied();

        // compute threats
        threats |= pawn_attacks::<C>(their_pawns);

        for sq in their_alfils {
            threats |= alfil_attacks(sq);
        }
        for sq in their_ferzes {
            threats |= ferz_attacks(sq);
        }
        for sq in their_knights {
            threats |= knight_attacks(sq);
        }
        for sq in their_rooks {
            threats |= rook_attacks(sq, blockers);
        }

        threats |= king_attacks(their_king);

        // compute checkers
        let our_king = self.king_sq(C::Opposite::COLOUR);
        let king_bb = our_king.as_set();
        let backwards_from_king = pawn_attacks::<C::Opposite>(king_bb);
        checkers |= backwards_from_king & their_pawns;

        let alfil_attacks = alfil_attacks(our_king);

        checkers |= alfil_attacks & their_alfils;

        let ferz_attacks = ferz_attacks(our_king);

        checkers |= ferz_attacks & their_ferzes;

        let knight_attacks = knight_attacks(our_king);

        checkers |= knight_attacks & their_knights;

        let rook_attacks = rook_attacks(our_king, blockers);

        checkers |= rook_attacks & their_rooks;

        Threats {
            all: threats,
            /* pawn: pawn_threats, minor: minor_threats, rook: rook_threats, */ checkers,
        }
    }

    pub fn reset(&mut self) {
        self.pieces.reset();
        self.piece_array = [None; 64];
        self.side = Colour::White;
        self.seventy_move_counter = 0;
        self.height = 0;
        self.ply = 0;
        self.key = 0;
        self.pawn_key = 0;
        self.threats = Threats::default();
        self.history.clear();
    }

    pub fn set_from_fen(&mut self, fen: &str) -> anyhow::Result<()> {
        if !fen.is_ascii() {
            bail!(format!("FEN string is not ASCII: {fen}"));
        }

        let mut rank = Rank::Eight;
        let mut file = File::A;

        self.reset();

        let fen_chars = fen.as_bytes();
        let split_idx = fen_chars
            .iter()
            .position(|&c| c == b' ')
            .with_context(|| format!("FEN string is missing space: {fen}"))?;
        let (board_part, info_part) = fen_chars.split_at(split_idx);

        for &c in board_part {
            let mut count = 1;
            let piece;
            match c {
                b'P' => piece = Some(Piece::WP),
                b'R' => piece = Some(Piece::WR),
                b'N' => piece = Some(Piece::WN),
                b'B' => piece = Some(Piece::WA),
                b'Q' => piece = Some(Piece::WF),
                b'K' => piece = Some(Piece::WK),
                b'p' => piece = Some(Piece::BP),
                b'r' => piece = Some(Piece::BR),
                b'n' => piece = Some(Piece::BN),
                b'b' => piece = Some(Piece::BA),
                b'q' => piece = Some(Piece::BF),
                b'k' => piece = Some(Piece::BK),
                b'1'..=b'8' => {
                    piece = None;
                    count = c - b'0';
                }
                b'/' => {
                    rank = rank.sub(1).unwrap();
                    file = File::A;
                    continue;
                }
                c => {
                    bail!(
                        "FEN string is invalid, got unexpected character: \"{}\"",
                        c as char
                    );
                }
            }

            for _ in 0..count {
                let sq = Square::from_rank_file(rank, file);
                if let Some(piece) = piece {
                    // this is only ever run once, as count is 1 for non-empty pieces.
                    self.add_piece(sq, piece);
                }
                file = file.add(1).unwrap_or(File::H);
            }
        }

        let mut info_parts = info_part[1..].split(|&c| c == b' ');

        self.set_side(info_parts.next())?;
        let mut info_parts = info_parts.skip(2);
        self.set_halfmove(info_parts.next())?;
        self.set_fullmove(info_parts.next())?;

        (
            self.key,
            self.pawn_key,
            self.non_pawn_key,
            self.minor_key,
            self.major_key,
        ) = self.generate_pos_keys();
        self.threats = self.generate_threats(self.side.flip());

        Ok(())
    }

    pub fn set_startpos(&mut self) {
        self.set_from_fen(Self::STARTING_FEN)
            .expect("for some reason, STARTING_FEN is now broken.");
    }

    #[cfg(test)]
    pub fn from_fen(fen: &str) -> anyhow::Result<Self> {
        let mut out = Self::new();
        out.set_from_fen(fen)?;
        Ok(out)
    }

    fn set_side(&mut self, side_part: Option<&[u8]>) -> anyhow::Result<()> {
        self.side = match side_part {
            Some([b'w']) => Colour::White,
            Some([b'b']) => Colour::Black,
            Some(other) => {
                bail!(format!(
                    "FEN string is invalid, expected side to be 'w' or 'b', got \"{}\"",
                    std::str::from_utf8(other).unwrap_or("<invalid utf8>")
                ))
            }
            None => bail!("FEN string is invalid, expected side part."),
        };
        Ok(())
    }

    fn set_halfmove(&mut self, halfmove_part: Option<&[u8]>) -> anyhow::Result<()> {
        match halfmove_part {
            None => bail!("FEN string is invalid, expected halfmove clock part.".to_string()),
            Some(halfmove_clock) => {
                self.seventy_move_counter = std::str::from_utf8(halfmove_clock)
                    .with_context(|| "FEN string is invalid, expected halfmove clock part to be valid UTF-8")?
                    .parse::<u8>()
                    .with_context(|| {
                        format!(
                            "FEN string is invalid, expected halfmove clock part to be a number, got \"{}\"",
                            std::str::from_utf8(halfmove_clock).unwrap_or("<invalid utf8>")
                        )
                    })?;
            }
        }

        Ok(())
    }

    fn set_fullmove(&mut self, fullmove_part: Option<&[u8]>) -> anyhow::Result<()> {
        match fullmove_part {
            None => bail!("FEN string is invalid, expected fullmove number part.".to_string()),
            Some(fullmove_number) => {
                let fullmove_number = std::str::from_utf8(fullmove_number)
                    .with_context(
                        || "FEN string is invalid, expected fullmove number part to be valid UTF-8",
                    )?
                    .parse::<usize>()
                    .with_context(
                        || "FEN string is invalid, expected fullmove number part to be a number",
                    )?;
                self.ply = (fullmove_number - 1) * 2;
                if self.side == Colour::Black {
                    self.ply += 1;
                }
            }
        }

        Ok(())
    }

    /// Determines if `sq` is attacked by `side`
    pub fn sq_attacked(&self, sq: Square, side: Colour) -> bool {
        if side == Colour::White {
            self.sq_attacked_by::<White>(sq)
        } else {
            self.sq_attacked_by::<Black>(sq)
        }
    }

    pub fn sq_attacked_by<C: Col>(&self, sq: Square) -> bool {
        // we remove this check because the board actually *can*
        // be in an inconsistent state when we call this, as it's
        // used to determine if a move is legal, and we'd like to
        // only do a lot of the make_move work *after* we've
        // determined that the move is legal.
        // #[cfg(debug_assertions)]
        // self.check_validity().unwrap();

        if C::WHITE == (self.side == Colour::Black) {
            return self.threats.all.contains_square(sq);
        }

        let sq_bb = sq.as_set();
        let our_pawns = self.pieces.pawns::<C>();
        let our_alfils = self.pieces.alfils::<C>();
        let our_ferzes = self.pieces.ferzes::<C>();
        let our_knights = self.pieces.knights::<C>();
        let our_rooks = self.pieces.rooks::<C>();
        let our_king = self.pieces.king::<C>();
        let blockers = self.pieces.occupied();

        // pawns
        let attacks = pawn_attacks::<C>(our_pawns);
        if (attacks & sq_bb).non_empty() {
            return true;
        }

        // alfils
        let alfil_attacks_from_this_square = movegen::alfil_attacks(sq);
        if (our_alfils & alfil_attacks_from_this_square).non_empty() {
            return true;
        }

        // ferzes
        let ferz_attacks_from_this_square = movegen::ferz_attacks(sq);
        if (our_ferzes & ferz_attacks_from_this_square).non_empty() {
            return true;
        }

        // knights
        let knight_attacks_from_this_square = movegen::knight_attacks(sq);
        if (our_knights & knight_attacks_from_this_square).non_empty() {
            return true;
        }

        // rooks
        let rook_attacks_from_this_square = movegen::rook_attacks(sq, blockers);
        if (our_rooks & rook_attacks_from_this_square).non_empty() {
            return true;
        }

        // king
        let king_attacks_from_this_square = movegen::king_attacks(sq);
        if (our_king & king_attacks_from_this_square).non_empty() {
            return true;
        }

        false
    }

    /// Checks whether a move is pseudo-legal.
    /// This means that it is a legal move, except for the fact that it might leave the king in check.
    pub fn is_pseudo_legal(&self, m: Move) -> bool {
        let from = m.from();
        let to = m.to();

        let moved_piece = self.piece_at(from);
        let captured_piece = self.piece_at(to);

        let Some(moved_piece) = moved_piece else {
            return false;
        };

        if moved_piece.colour() != self.side {
            return false;
        }

        if let Some(captured_piece) = captured_piece
            && captured_piece.colour() == self.side
        {
            return false;
        }

        if moved_piece.piece_type() != PieceType::Pawn && m.is_promo() {
            return false;
        }

        if moved_piece.piece_type() == PieceType::Pawn {
            let should_be_promoting = to > Square::H7 || to < Square::A2;
            if should_be_promoting && !m.is_promo() {
                return false;
            }
            if captured_piece.is_none() {
                return Some(to) == from.pawn_push(self.side);
            }
            // pawn capture
            if self.side == Colour::White {
                return (pawn_attacks::<White>(from.as_set()) & to.as_set()).non_empty();
            }
            return (pawn_attacks::<Black>(from.as_set()) & to.as_set()).non_empty();
        }

        (to.as_set()
            & movegen::attacks_by_type(moved_piece.piece_type(), from, self.pieces.occupied()))
        .non_empty()
    }

    pub fn any_attacked(&self, squares: SquareSet, by: Colour) -> bool {
        if by == self.side.flip() {
            (squares & self.threats.all).non_empty()
        } else {
            for sq in squares {
                if self.sq_attacked(sq, by) {
                    return true;
                }
            }
            false
        }
    }

    pub fn add_piece(&mut self, sq: Square, piece: Piece) {
        self.pieces.set_piece_at(sq, piece);
        *self.piece_at_mut(sq) = Some(piece);
    }

    /// Gets the piece that will be moved by the given move.
    pub fn moved_piece(&self, m: Move) -> Option<Piece> {
        let idx = m.from();
        self.piece_array[idx]
    }

    /// Gets the piece that will be captured by the given move.
    pub fn captured_piece(&self, m: Move) -> Option<Piece> {
        let idx = m.to();
        self.piece_array[idx]
    }

    /// Determines whether this move would be a capture in the current position.
    pub fn is_capture(&self, m: Move) -> bool {
        self.captured_piece(m).is_some()
    }

    /// Determines whether this move would be tactical in the current position.
    pub fn is_tactical(&self, m: Move) -> bool {
        m.is_promo() || self.is_capture(m)
    }

    /// Gets the piece at the given square.
    pub fn piece_at(&self, sq: Square) -> Option<Piece> {
        self.piece_array[sq]
    }

    /// Gets a mutable reference to the piece at the given square.
    pub fn piece_at_mut(&mut self, sq: Square) -> &mut Option<Piece> {
        &mut self.piece_array[sq]
    }

    pub fn make_move_simple(&mut self, m: Move) -> bool {
        self.make_move_base(m, &mut UpdateBuffer::default())
    }

    #[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
    pub fn make_move_base(&mut self, m: Move, update_buffer: &mut UpdateBuffer) -> bool {
        #[cfg(debug_assertions)]
        self.check_validity().unwrap();

        let from = m.from();
        let to = m.to();
        let side = self.side;
        let Some(piece) = self.moved_piece(m) else {
            return false;
        };
        let captured = self.captured_piece(m);

        let saved_state = Undo {
            seventy_move_counter: self.seventy_move_counter,
            threats: self.threats,
            piece_layout: self.pieces,
            piece_array: self.piece_array,
            key: self.key,
            pawn_key: self.pawn_key,
            non_pawn_key: self.non_pawn_key,
            minor_key: self.minor_key,
            major_key: self.major_key,
        };

        if m.is_promo() {
            // just remove the source piece, as a different piece will be arriving here
            update_buffer.clear_piece(from, piece);
        } else {
            update_buffer.move_piece(from, to, piece);
        }

        self.seventy_move_counter += 1;

        if let Some(captured) = captured {
            self.seventy_move_counter = 0;
            self.pieces.clear_piece_at(to, captured);
            update_buffer.clear_piece(to, captured);
        }

        if piece.piece_type() == PieceType::Pawn {
            self.seventy_move_counter = 0;
        }

        if m.is_promo() {
            let promo = Piece::new(side, PieceType::Ferz);
            debug_assert!(promo.piece_type().legal_promo());
            self.pieces.clear_piece_at(from, piece);
            self.pieces.set_piece_at(to, promo);
            update_buffer.add_piece(to, promo);
        } else {
            self.pieces.move_piece(from, to, piece);
        }

        self.side = self.side.flip();

        // reversed in_check fn, as we have now swapped sides
        if self.sq_attacked(self.king_sq(self.side.flip()), self.side) {
            // this would be a function but we run into borrow checker issues
            // because it's currently not smart enough to realize that we're
            // borrowing disjoint parts of the board.
            let Undo {
                seventy_move_counter,
                piece_layout,
                ..
            } = saved_state;

            // self.height -= 1;
            // self.ply -= 1;
            self.side = self.side.flip();
            // self.key = key;
            // self.pawn_key = pawn_key;
            // self.non_pawn_key = non_pawn_key;
            // self.minor_key = minor_key;
            // self.major_key = major_key;
            // self.material_key = material_key;
            self.seventy_move_counter = seventy_move_counter;
            // self.threats = threats;
            self.pieces = piece_layout;
            // self.piece_array = piece_array;
            return false;
        }

        let mut key = self.key;
        let mut pawn_key = self.pawn_key;
        let mut non_pawn_key = self.non_pawn_key;
        let mut minor_key = self.minor_key;
        let mut major_key = self.major_key;

        hash_side(&mut key);
        for &FeatureUpdate { sq, piece } in update_buffer.subs() {
            self.piece_array[sq] = None;
            hash_piece(&mut key, piece, sq);
            if piece.piece_type() == PieceType::Pawn {
                hash_piece(&mut pawn_key, piece, sq);
            } else {
                hash_piece(&mut non_pawn_key[piece.colour()], piece, sq);
                if piece.piece_type() == PieceType::King {
                    hash_piece(&mut major_key, piece, sq);
                    hash_piece(&mut minor_key, piece, sq);
                } else if matches!(piece.piece_type(), PieceType::Ferz | PieceType::Rook) {
                    hash_piece(&mut major_key, piece, sq);
                } else {
                    hash_piece(&mut minor_key, piece, sq);
                }
            }
        }
        for &FeatureUpdate { sq, piece } in update_buffer.adds() {
            self.piece_array[sq] = Some(piece);
            hash_piece(&mut key, piece, sq);
            if piece.piece_type() == PieceType::Pawn {
                hash_piece(&mut pawn_key, piece, sq);
            } else {
                hash_piece(&mut non_pawn_key[piece.colour()], piece, sq);
                if piece.piece_type() == PieceType::King {
                    hash_piece(&mut major_key, piece, sq);
                    hash_piece(&mut minor_key, piece, sq);
                } else if matches!(piece.piece_type(), PieceType::Ferz | PieceType::Rook) {
                    hash_piece(&mut major_key, piece, sq);
                } else {
                    hash_piece(&mut minor_key, piece, sq);
                }
            }
        }
        self.key = key;
        self.pawn_key = pawn_key;
        self.non_pawn_key = non_pawn_key;
        self.minor_key = minor_key;
        self.major_key = major_key;

        self.ply += 1;
        self.height += 1;

        self.threats = self.generate_threats(self.side.flip());

        self.history.push(saved_state);

        #[cfg(debug_assertions)]
        self.check_validity().unwrap();

        true
    }

    pub fn unmake_move_base(&mut self) {
        // we remove this check because the board actually *can*
        // be in an inconsistent state when we call this, as we
        // may be unmaking a move that was determined to be
        // illegal, and as such the full make_move hasn't been
        // run yet.
        // #[cfg(debug_assertions)]
        // self.check_validity().unwrap();

        let undo = self.history.last().expect("No move to unmake!");

        let Undo {
            seventy_move_counter,
            threats,
            piece_layout,
            piece_array,
            key,
            pawn_key,
            non_pawn_key,
            minor_key,
            major_key,
            ..
        } = undo;

        self.height -= 1;
        self.ply -= 1;
        self.side = self.side.flip();
        self.key = *key;
        self.pawn_key = *pawn_key;
        self.non_pawn_key = *non_pawn_key;
        self.minor_key = *minor_key;
        self.major_key = *major_key;
        self.seventy_move_counter = *seventy_move_counter;
        self.threats = *threats;
        self.pieces = *piece_layout;
        self.piece_array = *piece_array;

        self.history.pop();

        #[cfg(debug_assertions)]
        self.check_validity().unwrap();
    }

    pub fn make_nullmove(&mut self) {
        #[cfg(debug_assertions)]
        self.check_validity().unwrap();
        debug_assert!(!self.in_check());

        self.history.push(Undo {
            threats: self.threats,
            key: self.key,
            ..Default::default()
        });

        let mut key = self.key;
        hash_side(&mut key);
        self.key = key;

        self.side = self.side.flip();
        self.ply += 1;
        self.height += 1;

        self.threats = self.generate_threats(self.side.flip());

        #[cfg(debug_assertions)]
        self.check_validity().unwrap();
    }

    pub fn unmake_nullmove(&mut self) {
        #[cfg(debug_assertions)]
        self.check_validity().unwrap();

        self.height -= 1;
        self.ply -= 1;
        self.side = self.side.flip();

        let Undo { threats, key, .. } = self.history.last().expect("No move to unmake!");

        self.threats = *threats;
        self.key = *key;

        self.history.pop();

        #[cfg(debug_assertions)]
        self.check_validity().unwrap();
    }

    pub fn last_move_was_nullmove(&self) -> bool {
        if let Some(Undo { piece_layout, .. }) = self.history.last() {
            piece_layout.all_kings().is_empty()
        } else {
            false
        }
    }

    /// Makes a guess about the new position key after a move.
    /// This is a cheap estimate, and will fail for promotions.
    pub fn key_after(&self, m: Move) -> u64 {
        let src = m.from();
        let tgt = m.to();
        let piece = self.moved_piece(m).unwrap();
        let captured = self.piece_at(tgt);

        let mut new_key = self.key;
        hash_side(&mut new_key);
        hash_piece(&mut new_key, piece, src);
        hash_piece(&mut new_key, piece, tgt);

        if let Some(captured) = captured {
            hash_piece(&mut new_key, captured, tgt);
        }

        new_key
    }

    pub fn key_after_null_move(&self) -> u64 {
        let mut new_key = self.key;
        hash_side(&mut new_key);
        new_key
    }

    /// Parses a move in the UCI format and returns a move or a reason why it couldn't be parsed.
    pub fn parse_uci(&self, uci: &str) -> anyhow::Result<Move> {
        let san_bytes = uci.as_bytes();
        if !(4..=5).contains(&san_bytes.len()) {
            bail!("invalid length: {}", san_bytes.len());
        }
        if !(b'a'..=b'h').contains(&san_bytes[0]) {
            bail!("invalid from_square file: {}", san_bytes[0] as char);
        }
        if !(b'1'..=b'8').contains(&san_bytes[1]) {
            bail!("invalid from_square rank: {}", san_bytes[1] as char);
        }
        if !(b'a'..=b'h').contains(&san_bytes[2]) {
            bail!("invalid to_square file: {}", san_bytes[2] as char);
        }
        if !(b'1'..=b'8').contains(&san_bytes[3]) {
            bail!("invalid to_square rank: {}", san_bytes[3] as char);
        }
        if san_bytes.len() == 5 && san_bytes[4] != b'q' {
            bail!("invalid promotion piece: {}", san_bytes[4] as char);
        }

        let from = Square::from_rank_file(
            Rank::from_index(san_bytes[1] - b'1').context("unknown")?,
            File::from_index(san_bytes[0] - b'a').context("unknown")?,
        );
        let to = Square::from_rank_file(
            Rank::from_index(san_bytes[3] - b'1').context("unknown")?,
            File::from_index(san_bytes[2] - b'a').context("unknown")?,
        );

        let mut list = MoveList::new();
        self.generate_moves(&mut list);

        list.iter_moves()
            .copied()
            .find(|&m| {
                m.from() == from
                    && m.to() == to
                    && (san_bytes.len() == 4 || m.is_promo() == (san_bytes[4] == b'q'))
            })
            .with_context(|| format!("illegal move: {}", uci))
    }

    pub fn san(&mut self, m: Move) -> Option<String> {
        let check_char = match self.gives(m) {
            CheckState::None => "",
            CheckState::Check => "+",
            CheckState::Checkmate => "#",
        };
        let to_sq = m.to();
        let moved_piece = self.piece_at(m.from())?;
        let is_capture = self.is_capture(m);
        let piece_prefix = match moved_piece.piece_type() {
            PieceType::Pawn if !is_capture => "",
            PieceType::Pawn => &"abcdefgh"[m.from().file() as usize..=m.from().file() as usize],
            PieceType::Alfil => "B",
            PieceType::Ferz => "Q",
            PieceType::Knight => "N",
            PieceType::Rook => "R",
            PieceType::King => "K",
        };
        let possible_ambiguous_attackers = if moved_piece.piece_type() == PieceType::Pawn {
            SquareSet::EMPTY
        } else {
            movegen::attacks_by_type(moved_piece.piece_type(), to_sq, self.pieces.occupied())
                & self.pieces.piece_bb(moved_piece)
        };
        let needs_disambiguation =
            possible_ambiguous_attackers.count() > 1 && moved_piece.piece_type() != PieceType::Pawn;
        let from_file = SquareSet::FILES[m.from().file()];
        let from_rank = SquareSet::RANKS[m.from().rank()];
        let can_be_disambiguated_by_file = (possible_ambiguous_attackers & from_file).count() == 1;
        let can_be_disambiguated_by_rank = (possible_ambiguous_attackers & from_rank).count() == 1;
        let needs_both = !can_be_disambiguated_by_file && !can_be_disambiguated_by_rank;
        let must_be_disambiguated_by_file = needs_both || can_be_disambiguated_by_file;
        let must_be_disambiguated_by_rank =
            needs_both || (can_be_disambiguated_by_rank && !can_be_disambiguated_by_file);
        let disambiguator1 = if needs_disambiguation && must_be_disambiguated_by_file {
            &"abcdefgh"[m.from().file() as usize..=m.from().file() as usize]
        } else {
            ""
        };
        let disambiguator2 = if needs_disambiguation && must_be_disambiguated_by_rank {
            &"12345678"[m.from().rank() as usize..=m.from().rank() as usize]
        } else {
            ""
        };
        let capture_sigil = if is_capture { "x" } else { "" };
        let promo_str = if m.is_promo() { "=Q" } else { "" };
        let san = format!(
            "{piece_prefix}{disambiguator1}{disambiguator2}{capture_sigil}{to_sq}{promo_str}{check_char}"
        );
        Some(san)
    }

    pub fn gives(&mut self, m: Move) -> CheckState {
        if !self.make_move_simple(m) {
            return CheckState::None;
        }
        let gives_check = self.in_check();
        if gives_check {
            let mut ml = MoveList::new();
            self.generate_moves(&mut ml);
            for &m in ml.iter_moves() {
                if !self.make_move_simple(m) {
                    continue;
                }
                // we found a legal move, so m does not give checkmate.
                self.unmake_move_base();
                self.unmake_move_base();
                return CheckState::Check;
            }
            // we didn't return, so there were no legal moves,
            // so m gives checkmate.
            self.unmake_move_base();
            return CheckState::Checkmate;
        }
        self.unmake_move_base();
        CheckState::None
    }

    /// Has the current position occurred before in the current game?
    pub fn is_repetition(&self) -> bool {
        let mut counter = 0;
        // distance to the last irreversible move
        let moves_since_zeroing = self.seventy_move_counter() as usize;
        // a repetition is first possible at four ply back:
        for (dist_back, u) in self
            .history
            .iter()
            .rev()
            .enumerate()
            .take(moves_since_zeroing)
            .skip(3)
            .step_by(2)
        {
            if u.key == self.key {
                // in-tree, can twofold:
                if dist_back < self.height {
                    return true;
                }
                // partially materialised, proper threefold:
                counter += 1;
                if counter >= 2 {
                    return true;
                }
            }
        }
        false
    }

    /// Should we consider the current position a draw?
    pub fn is_draw(&self) -> bool {
        (self.seventy_move_counter >= 140 || self.is_repetition()) && self.height != 0
    }

    pub fn legal_moves(&mut self) -> Vec<Move> {
        let mut move_list = MoveList::new();
        self.generate_moves(&mut move_list);
        let mut legal_moves = Vec::new();
        for &m in move_list.iter_moves() {
            if self.make_move_simple(m) {
                self.unmake_move_base();
                legal_moves.push(m);
            }
        }
        legal_moves
    }

    pub const fn seventy_move_counter(&self) -> u8 {
        self.seventy_move_counter
    }

    pub fn has_insufficient_material<C: Col>(&self) -> bool {
        //TODO
        self.pieces.king::<C>() == self.pieces.our_pieces::<C>()
    }

    pub const fn full_move_number(&self) -> usize {
        self.ply / 2 + 1
    }

    pub fn make_random_move(&mut self, rng: &mut ThreadRng) -> Option<Move> {
        let mut ml = MoveList::new();
        self.generate_moves(&mut ml);
        let self::movegen::MoveListEntry { mov, .. } = ml.choose(rng)?;
        self.make_move_simple(*mov);
        Some(*mov)
    }

    pub fn is_insufficient_material(&self) -> bool {
        self.has_insufficient_material::<White>() && self.has_insufficient_material::<Black>()
    }

    fn is_bare_king<C: Col>(&self) -> bool {
        let our_kings = self.pieces.king::<C>();
        let our_pieces = self.pieces.our_pieces::<C>();

        if our_pieces != our_kings {
            return false;
        }

        let their_kings = self.pieces.king::<C::Opposite>();
        let their_pieces = self.pieces.their_pieces::<C>() & !their_kings;

        if their_pieces.count() != 1 {
            return false;
        }

        let our_king_attacks = king_attacks(self.king_sq(C::COLOUR));
        let their_king_attacks = king_attacks(self.king_sq(C::Opposite::COLOUR));

        let our_king_attacks = our_king_attacks & !their_king_attacks;

        (their_pieces & !our_king_attacks).non_empty()
    }

    pub fn outcome(&mut self) -> GameOutcome {
        if self.seventy_move_counter >= 140 {
            return GameOutcome::Draw(DrawType::SeventyMoves);
        }
        let mut reps = 1;
        for undo in self.history.iter().rev().skip(1).step_by(2) {
            if undo.key == self.key {
                reps += 1;
                if reps == 3 {
                    return GameOutcome::Draw(DrawType::Repetition);
                }
            }
            // optimisation: if the seventy move counter was zeroed, then any prior positions will not be repetitions.
            if undo.seventy_move_counter == 0 {
                break;
            }
        }
        if self.is_insufficient_material() {
            return GameOutcome::Draw(DrawType::InsufficientMaterial);
        }
        if self.is_bare_king::<White>() {
            return GameOutcome::BlackWin(WinType::BareKing);
        }
        if self.is_bare_king::<Black>() {
            return GameOutcome::WhiteWin(WinType::BareKing);
        }
        let mut move_list = MoveList::new();
        self.generate_moves(&mut move_list);
        let mut legal_moves = false;
        for &m in move_list.iter_moves() {
            if self.make_move_simple(m) {
                self.unmake_move_base();
                legal_moves = true;
                break;
            }
        }
        if legal_moves {
            GameOutcome::Ongoing
        } else {
            match self.side {
                Colour::White => GameOutcome::BlackWin(WinType::Mate),
                Colour::Black => GameOutcome::WhiteWin(WinType::Mate),
            }
        }
    }

    #[cfg(debug_assertions)]
    pub fn assert_mated(&mut self) {
        assert!(self.in_check());
        let mut move_list = MoveList::new();
        self.generate_moves(&mut move_list);
        for &mv in move_list.iter_moves() {
            assert!(!self.make_move_simple(mv));
        }
    }

    #[allow(clippy::identity_op)]
    pub fn material_count(&self) -> u32 {
        let pawn_material = 1 * self.pieces.all_pawns().count();
        let alfil_material = 1 * self.pieces.all_alfils().count();
        let ferz_material = 2 * self.pieces.all_ferzes().count();
        let knight_material = 4 * self.pieces.all_knights().count();
        let rook_material = 6 * self.pieces.all_rooks().count();

        pawn_material + alfil_material + ferz_material + knight_material + rook_material
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GameOutcome {
    WhiteWin(WinType),
    BlackWin(WinType),
    Draw(DrawType),
    Ongoing,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WinType {
    Mate,
    BareKing,
    TB,
    Adjudication,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DrawType {
    TB,
    SeventyMoves,
    Repetition,
    InsufficientMaterial,
    Adjudication,
}

impl GameOutcome {
    pub const fn as_packed_u8(self) -> u8 {
        // 0 for black win, 1 for draw, 2 for white win
        match self {
            Self::WhiteWin(_) => 2,
            Self::BlackWin(_) => 0,
            Self::Draw(_) => 1,
            Self::Ongoing => panic!("Game is not over!"),
        }
    }
}

impl Default for Board {
    fn default() -> Self {
        let mut out = Self::new();
        out.set_startpos();
        out
    }
}

impl Display for Board {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), std::fmt::Error> {
        let mut counter = 0;
        for rank in Rank::all().rev() {
            for file in File::all() {
                let sq = Square::from_rank_file(rank, file);
                let piece = self.piece_at(sq);
                if let Some(piece) = piece {
                    if counter != 0 {
                        write!(f, "{counter}")?;
                    }
                    counter = 0;
                    write!(f, "{piece}")?;
                } else {
                    counter += 1;
                }
            }
            if counter != 0 {
                write!(f, "{counter}")?;
            }
            counter = 0;
            if rank != Rank::One {
                write!(f, "/")?;
            }
        }

        if self.side == Colour::White {
            write!(f, " w")?;
        } else {
            write!(f, " b")?;
        }
        write!(f, " - -")?;
        write!(f, " {}", self.seventy_move_counter)?;
        write!(f, " {}", self.ply / 2 + 1)?;

        Ok(())
    }
}

impl std::fmt::UpperHex for Board {
    fn fmt(&self, f: &mut Formatter) -> Result<(), std::fmt::Error> {
        for rank in Rank::all().rev() {
            write!(f, "{} ", rank as u8 + 1)?;
            for file in File::all() {
                let sq = Square::from_rank_file(rank, file);
                if let Some(piece) = self.piece_at(sq) {
                    write!(f, "{piece} ")?;
                } else {
                    write!(f, ". ")?;
                }
            }
            writeln!(f)?;
        }

        writeln!(f, "  a b c d e f g h")?;
        writeln!(f, "FEN: {self}")?;

        Ok(())
    }
}

mod tests {
    #[test]
    fn read_fen_validity() {
        use super::Board;

        let mut board_1 = Board::new();
        board_1
            .set_from_fen(Board::STARTING_FEN)
            .expect("setfen failed.");
        board_1.check_validity().unwrap();

        let board_2 = Board::from_fen(Board::STARTING_FEN).expect("setfen failed.");
        board_2.check_validity().unwrap();

        assert_eq!(board_1, board_2);
    }

    #[test]
    fn game_end_states() {
        use super::Board;
        use super::{DrawType, GameOutcome, WinType};
        use crate::{shatranj::shatranjmove::Move, shatranj::types::Square};

        let mut seventymove_draw =
            Board::from_fen("rnbqkb1r/pppppppp/5n2/8/3N4/8/PPPPPPPP/RNBQKB1R b KQkq - 140 2")
                .unwrap();
        assert_eq!(
            seventymove_draw.outcome(),
            GameOutcome::Draw(DrawType::SeventyMoves)
        );
        let mut draw_repetition = Board::default();
        assert_eq!(draw_repetition.outcome(), GameOutcome::Ongoing);
        draw_repetition.make_move_simple(Move::new(Square::G1, Square::F3));
        draw_repetition.make_move_simple(Move::new(Square::B8, Square::C6));
        assert_eq!(draw_repetition.outcome(), GameOutcome::Ongoing);
        draw_repetition.make_move_simple(Move::new(Square::F3, Square::G1));
        draw_repetition.make_move_simple(Move::new(Square::C6, Square::B8));
        assert_eq!(draw_repetition.outcome(), GameOutcome::Ongoing);
        draw_repetition.make_move_simple(Move::new(Square::G1, Square::F3));
        draw_repetition.make_move_simple(Move::new(Square::B8, Square::C6));
        assert_eq!(draw_repetition.outcome(), GameOutcome::Ongoing);
        draw_repetition.make_move_simple(Move::new(Square::F3, Square::G1));
        draw_repetition.make_move_simple(Move::new(Square::C6, Square::B8));
        assert_eq!(
            draw_repetition.outcome(),
            GameOutcome::Draw(DrawType::Repetition)
        );
        let mut stalemate = Board::from_fen("7k/1p1R4/1P4R1/8/8/8/8/K7 b - - 0 1").unwrap();
        assert_eq!(stalemate.outcome(), GameOutcome::WhiteWin(WinType::Mate));
        let mut insufficient_material_bare_kings =
            Board::from_fen("8/8/5k2/8/8/2K5/8/8 b - - 0 1").unwrap();
        assert_eq!(
            insufficient_material_bare_kings.outcome(),
            GameOutcome::Draw(DrawType::InsufficientMaterial)
        );
        let mut bare_king = Board::from_fen("8/8/8/2KRk3/8/8/8/8 w - - 0 1").unwrap();
        assert_eq!(
            bare_king.outcome(),
            GameOutcome::WhiteWin(WinType::BareKing)
        );
        let mut bare_king_can_recapture =
            Board::from_fen("8/8/8/1K1Rk3/8/8/8/8 b - - 0 1").unwrap();
        assert_eq!(bare_king_can_recapture.outcome(), GameOutcome::Ongoing);
        let mut insufficient_material_knights =
            Board::from_fen("8/8/5k2/8/2N5/2K2N2/8/8 b - - 0 1").unwrap();
        assert_eq!(
            insufficient_material_knights.outcome(),
            GameOutcome::Ongoing
        );
        // using FIDE rules.
    }

    /*
    #[test]
    fn fen_round_trip() {
        use crate::shatranj::board::Board;
        use std::{
            fs::File,
            io::{BufRead, BufReader},
        };

        let fens = BufReader::new(File::open("epds/perftsuite.epd").unwrap())
            .lines()
            .map(|l| l.unwrap().split_once(';').unwrap().0.trim().to_owned())
            .collect::<Vec<_>>();
        let mut board = Board::new();
        for fen in fens {
            board.set_from_fen(&fen).expect("setfen failed.");
            let fen_2 = board.to_string();
            assert_eq!(fen, fen_2);
        }
    }
    */

    #[test]
    fn threat_generation_white() {
        use super::Board;
        use crate::shatranj::squareset::SquareSet;

        let board = Board::from_fen("3k4/8/8/5N2/8/1P6/8/K1Q1RB2 b - - 0 1").unwrap();
        assert_eq!(
            board.threats.all,
            SquareSet::from_inner(0x1050_9810_9dd8_1b2e)
        );
    }

    #[test]
    fn threat_generation_black() {
        use super::Board;
        use crate::shatranj::squareset::SquareSet;

        let board = Board::from_fen("2br1q1k/8/6p1/8/2n5/8/8/4K3 w - - 0 1").unwrap();
        assert_eq!(
            board.threats.all,
            SquareSet::from_inner(0x74d8_1bb9_0819_0a08)
        );
    }

    #[test]
    fn key_after_works_for_simple_moves() {
        use super::Board;
        use crate::shatranj::shatranjmove::Move;
        use crate::shatranj::types::Square;
        let mut board = Board::default();
        let mv = Move::new(Square::E2, Square::E3);
        let key = board.key_after(mv);
        board.make_move_simple(mv);
        assert_eq!(board.key, key);
    }

    #[test]
    fn key_after_works_for_captures() {
        use super::Board;
        use crate::shatranj::shatranjmove::Move;
        use crate::shatranj::types::Square;
        let mut board = Board::from_fen(
            "r1bqkb1r/ppp2ppp/2n5/3np1N1/2B5/8/PPPP1PPP/RNBQK2R w - - 0 6"
        )
        .unwrap();
        // Nxf7!!
        let mv = Move::new(Square::G5, Square::F7);
        let key = board.key_after(mv);
        board.make_move_simple(mv);
        assert_eq!(board.key, key);
    }

    #[test]
    fn key_after_works_for_nullmove() {
        use super::Board;
        let mut board = Board::default();
        let key = board.key_after_null_move();
        board.make_nullmove();
        assert_eq!(board.key, key);
    }
}
