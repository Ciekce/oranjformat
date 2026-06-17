#![allow(clippy::cast_possible_truncation)]

use crate::rng::XorShiftState;

/// Implements a C-style for loop, for use in const fn.
#[macro_export]
macro_rules! cfor {
    ($init: stmt; $cond: expr; $step: expr; $body: block) => {
        {
            $init
            #[allow(while_true)]
            while $cond {
                $body;

                $step;
            }
        }
    }
}

const fn init_hash_keys() -> ([[u64; 64]; 12], u64) {
    let mut state = XorShiftState::new();
    let mut piece_keys = [[0; 64]; 12];
    cfor!(let mut index = 0; index < 12; index += 1; {
        cfor!(let mut sq = 0; sq < 64; sq += 1; {
            let key;
            (key, state) = state.next_self();
            piece_keys[index][sq] = key;
        });
    });
    let key;
    (key, _) = state.next_self();
    let side_key = key;
    (piece_keys, side_key)
}

pub static PIECE_KEYS: [[u64; 64]; 12] = init_hash_keys().0;
pub const SIDE_KEY: u64 = init_hash_keys().1;

mod tests {
    #[test]
    fn all_piece_keys_different() {
        use crate::lookups::PIECE_KEYS;
        let mut hashkeys = PIECE_KEYS.iter().flat_map(|&k| k).collect::<Vec<u64>>();
        hashkeys.sort_unstable();
        let len_before = hashkeys.len();
        hashkeys.dedup();
        let len_after = hashkeys.len();
        assert_eq!(len_before, len_after);
    }
}
