#![allow(unused_imports)]

use crate::shatranj::{
    board::Board,
    piece::{Colour, Piece},
    types::{Rank, Square},
};

impl Board {
    #[cfg(debug_assertions)]
    #[allow(
        clippy::cognitive_complexity,
        clippy::too_many_lines,
        clippy::similar_names,
        clippy::cast_possible_truncation,
        dead_code
    )]
    pub fn check_validity(&self) -> anyhow::Result<()> {
        use anyhow::bail;

        // check turn

        if self.side != Colour::White && self.side != Colour::Black {
            bail!("invalid side: {:?}", self.side);
        }

        // check square-set / piece array coherency
        for sq in Square::all() {
            let piece = self.piece_array[sq];
            if self.pieces.piece_at(sq) != piece {
                bail!(
                    "square-set / piece array coherency corrupt: expected square {} to be '{:?}' but was '{:?}'",
                    sq,
                    piece,
                    self.piece_at(sq)
                );
            }
        }

        if !(self.side == Colour::White || self.side == Colour::Black) {
            bail!(
                "side is corrupt: expected WHITE or BLACK, got {:?}",
                self.side
            );
        }
        if self.generate_pos_keys() != self.all_keys() {
            bail!(
                "key is corrupt: expected {:?}, got {:?}",
                self.generate_pos_keys(),
                self.all_keys()
            );
        }

        // the seventy-move counter is allowed to be *exactly* 140, to allow a finished game to be
        // created.
        if self.seventy_move_counter > 140 {
            bail!(
                "seventy move counter is corrupt: expected 0-140, got {}",
                self.seventy_move_counter
            );
        }

        // check there are the correct number of kings for each side
        if self.pieces.piece_bb(Piece::WK).count() != 1 {
            bail!(
                "white king count is corrupt: expected 1, got {}",
                self.pieces.piece_bb(Piece::WK).count()
            );
        }
        if self.pieces.piece_bb(Piece::BK).count() != 1 {
            bail!(
                "black king count is corrupt: expected 1, got {}",
                self.pieces.piece_bb(Piece::BK).count()
            );
        }

        if self.piece_at(self.king_sq(Colour::White)) != Some(Piece::WK) {
            bail!(
                "white king square is corrupt: expected white king, got {:?}",
                self.piece_at(self.king_sq(Colour::White))
            );
        }
        if self.piece_at(self.king_sq(Colour::Black)) != Some(Piece::BK) {
            bail!(
                "black king square is corrupt: expected black king, got {:?}",
                self.piece_at(self.king_sq(Colour::Black))
            );
        }

        Ok(())
    }
}
