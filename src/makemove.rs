// move making doesn't actually happen here,
// it happens in board.rs, but there are
// utility functions here that are used in
// the Board::make_move() function.

use crate::{
    lookups::{PIECE_KEYS, SIDE_KEY},
    shatranj::{piece::Piece, types::Square},
};

pub fn hash_piece(key: &mut u64, piece: Piece, sq: Square) {
    let piece_key = PIECE_KEYS[piece][sq];
    *key ^= piece_key;
}

pub fn hash_side(key: &mut u64) {
    *key ^= SIDE_KEY;
}
