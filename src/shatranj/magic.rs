use crate::{
    shatranj::{squareset::SquareSet, types::Square},
    rng::XorShiftState,
};

macro_rules! cfor {
    ($init: stmt; $cond: expr; $step: expr; $body: block) => {
        {
            $init
            while $cond {
                $body;

                $step;
            }
        }
    }
}

/// The number of relevant bits in the bitboard representation of the attack squareset.
/// For example, on a1, a rook has 14 squares it can move to
/// along the file or rank - but the contents of a8 and h1 are irrelevant to movegen,
/// so the number of relevant bits on the board is 12.
#[rustfmt::skip]
pub static ROOK_REL_BITS: [i32; 64] = [
    12, 11, 11, 11, 11, 11, 11, 12,
    11, 10, 10, 10, 10, 10, 10, 11,
    11, 10, 10, 10, 10, 10, 10, 11,
    11, 10, 10, 10, 10, 10, 10, 11,
    11, 10, 10, 10, 10, 10, 10, 11,
    11, 10, 10, 10, 10, 10, 10, 11,
    11, 10, 10, 10, 10, 10, 10, 11,
    12, 11, 11, 11, 11, 11, 11, 12,
];

const fn set_occupancy(index: usize, bits_in_mask: i32, mut attack_mask: SquareSet) -> SquareSet {
    let mut occupancy = SquareSet::EMPTY;

    let mut count = 0;
    while count < bits_in_mask {
        let square = attack_mask.first();
        attack_mask = attack_mask.remove_square(square);
        // this bitwise AND seems really weird
        if index & (1 << count) != 0 {
            occupancy = occupancy.add_square(square);
        }
        count += 1;
    }

    occupancy
}

const fn mask_rook_attacks(sq: Square) -> SquareSet {
    let mut attacks = 0;

    // file and rank
    let (mut f, mut r);

    // target files and ranks
    let tr = sq.rank() as i32;
    let tf = sq.file() as i32;

    cfor!(r = tr + 1; r <= 6; r += 1; {
        attacks |= 1 << (r * 8 + tf);
    });
    cfor!(r = tr - 1; r >= 1; r -= 1; {
        attacks |= 1 << (r * 8 + tf);
    });
    cfor!(f = tf + 1; f <= 6; f += 1; {
        attacks |= 1 << (tr * 8 + f);
    });
    cfor!(f = tf - 1; f >= 1; f -= 1; {
        attacks |= 1 << (tr * 8 + f);
    });

    SquareSet::from_inner(attacks)
}

const fn rook_attacks_on_the_fly(square: Square, block: SquareSet) -> SquareSet {
    let mut attacks = 0;

    // so sue me
    let block = block.inner();

    // file and rank
    let (mut f, mut r);

    // target files and ranks
    let tr = square.rank() as i32;
    let tf = square.file() as i32;

    cfor!(r = tr + 1; r <= 7; r += 1; {
        let sq_bb = 1 << (r * 8 + tf);
        attacks |= sq_bb;
        if block & sq_bb != 0 {
            break;
        }
    });
    cfor!(r = tr - 1; r >= 0; r -= 1; {
        let sq_bb = 1 << (r * 8 + tf);
        attacks |= sq_bb;
        if block & sq_bb != 0 {
            break;
        }
    });
    cfor!(f = tf + 1; f <= 7; f += 1; {
        let sq_bb = 1 << (tr * 8 + f);
        attacks |= sq_bb;
        if block & sq_bb != 0 {
            break;
        }
    });
    cfor!(f = tf - 1; f >= 0; f -= 1; {
        let sq_bb = 1 << (tr * 8 + f);
        attacks |= sq_bb;
        if block & sq_bb != 0 {
            break;
        }
    });

    SquareSet::from_inner(attacks)
}

/**************************************\
|       Generating magic numbers       |
|                :3                    |
\**************************************/

fn find_magic(square: Square, relevant_bits: i32) -> u64 {
    // occupancies array
    let mut occupancies = vec![SquareSet::EMPTY; 4096];

    // attacks array
    let mut attacks = vec![SquareSet::EMPTY; 4096];

    // used indices array
    let mut used_indices = vec![SquareSet::EMPTY; 4096];

    // mask piece attack
    let mask_attack = mask_rook_attacks(square);

    // occupancy variations
    let occupancy_variations = 1 << relevant_bits;

    // loop over all occupancy variations
    cfor!(let mut count = 0; count < occupancy_variations; count += 1; {
        // init occupancies
        occupancies[count] = set_occupancy(count, relevant_bits, mask_attack);

        // init attacks
        attacks[count] = rook_attacks_on_the_fly(square, occupancies[count]);
    });

    let mut rng = XorShiftState::new();
    // test the magic!
    cfor!(let mut random_count = 0; random_count < 100_000_000; random_count += 1; {
        let magic = rng.random_few_bits();

        if ((mask_attack.inner().wrapping_mul(magic)) & 0xFF00_0000_0000_0000).count_ones() < 6 {
            continue;
        }

        // reset used attacks
        cfor!(let mut i = 0; i < used_indices.len(); i += 1; {
            used_indices[i] = SquareSet::EMPTY;
        });

        // init count & fail flag
        let (mut count, mut fail);

        // test magic index
        cfor!((count, fail) = (0, false); !fail && count < occupancy_variations; count += 1; {
            #[allow(clippy::cast_possible_truncation)]
            let magic_index = ((occupancies[count].inner().wrapping_mul(magic)) >> (64 - relevant_bits)) as usize;

            // if got free index
            if used_indices[magic_index].is_empty() {
                // assign attack map
                used_indices[magic_index] = attacks[count];
            }

            // otherwise, fail if we have a collision
            else if used_indices[magic_index] != attacks[count] {
                fail = true;
            }
        });

        if !fail {
            return magic;
        }
    });

    // on fail
    panic!("magic number failed");
}

#[allow(
    dead_code,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap
)]
pub fn init_magics() {
    println!("Generating rook magics...");
    println!("static ROOK_MAGICS: [u64; 64] = [");
    for (square, &relbits) in Square::all().zip(ROOK_REL_BITS.iter()) {
        let magic = find_magic(square, relbits);
        let magic_str = format!("{magic:016X}");
        // split into blocks of four
        let magic_str = magic_str.chars().collect::<Vec<char>>();
        let magic_str = magic_str
            .chunks(4)
            .map(|chunk| chunk.iter().collect::<String>())
            .collect::<Vec<String>>()
            .join("_");
        println!("    0x{magic_str},");
    }
    println!("];");
    println!("Done!");
}

macro_rules! init_masks_with {
    ($attack_function:ident) => {{
        let mut masks = [SquareSet::EMPTY; 64];
        cfor!(let mut square = 0; square < 64; square += 1; {
            masks[square as usize] = $attack_function(Square::new_clamped(square));
        });
        masks
    }};
}

pub static ROOK_MASKS: [SquareSet; 64] = init_masks_with!(mask_rook_attacks);

#[allow(clippy::large_stack_arrays)]
pub static ROOK_ATTACKS: [[SquareSet; 4096]; 64] =
    unsafe { std::mem::transmute(*include_bytes!("../../embeds/orthogonal_attacks.bin")) };

pub static ROOK_MAGICS: [u64; 64] = [
    0x2080_0010_2080_4000,
    0x0240_2000_4001_5005,
    0x6900_2002_1100_0840,
    0x0200_0821_1004_4200,
    0x0080_0400_0800_8002,
    0x0280_1104_0080_0200,
    0x0200_0808_C704_0200,
    0x0200_0046_0088_2304,
    0x1900_8000_8040_002C,
    0x0002_4000_2000_5000,
    0x0100_8080_1000_2000,
    0x0026_0010_4120_0A00,
    0x1041_8080_0400_0800,
    0x1040_8080_0200_0400,
    0x8960_8080_0100_8200,
    0x0420_8001_0010_6080,
    0x0340_0080_0484_4820,
    0xA010_10C0_0040_2001,
    0x0100_8080_1000_2000,
    0x0108_0080_8010_0008,
    0x0181_0100_1004_0800,
    0x0400_8080_0200_0401,
    0x0800_1400_0248_0130,
    0x0100_0200_2310_408C,
    0x9000_4000_8020_8000,
    0x0000_4010_8020_0080,
    0x0123_2004_8010_0188,
    0x0108_0080_8010_0008,
    0xB010_0400_8080_0800,
    0x4A0A_0400_8002_0080,
    0x1200_5004_0002_3148,
    0x0904_0106_0000_4484,
    0x0000_8041_0200_2200,
    0x0000_8041_0200_2200,
    0x0040_1000_8080_2000,
    0x0000_8010_0080_0800,
    0xB010_0400_8080_0800,
    0x0002_0020_0404_0010,
    0x4002_3081_0400_0812,
    0x4040_0041_0200_0084,
    0x0040_0240_8002_8020,
    0x0490_0440_2004_4000,
    0x0040_1000_2000_8080,
    0x0440_0800_1000_8080,
    0x5000_0400_0800_8080,
    0x2000_C004_2008_0110,
    0x0808_0100_0200_8080,
    0x0000_0400_AC42_0009,
    0x0000_8041_0200_2200,
    0x0000_8041_0200_2200,
    0x0040_1000_2000_8080,
    0x0110_0088_0010_8180,
    0x8028_0004_0080_0880,
    0x3086_6010_0440_4801,
    0x8000_0102_0850_0400,
    0x0220_1889_0054_0200,
    0x0001_0020_12C1_8001,
    0x0400_2045_0082_1206,
    0x4000_2008_4200_8012,
    0xB481_0010_0020_0409,
    0x0001_0070_0208_0025,
    0x4001_0002_0400_0801,
    0x2038_1140_9002_1804,
    0x0000_0080_4502_2C02,
];
