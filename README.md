oranjformat is a crate for the game data representation used by the [oranj](https://github.com/Ciekce/oranj) shatranj engine, derived from [viriformat](https://github.com/cosmobobak/viriformat).

## Specification

All integers in oranjformat are little-endian.

The square indexing used represents A1=0, H1=7, A8=56, H8=63.

An oranjformat file consists of one or more `Game`s concatenated together.

A `Game` consists of a modified marlinformat `PackedBoard` followed by zero or more `Move` and `Score` pairs, terminated by four zero bytes.

A `PackedBoard` is a structure of:
- A 64-bit occupied-piece bitboard.
- A 32-entry array of 4-bit pieces, where the `i`th entry corresponds to the `i`th least-significant set bit in the occupied-piece bitboard.
  - The lower three bits of a piece corresponds to its type: pawn is 0, alfil is 1, ferz is 2, knight is 3, rook is 4, king is 5.
  - A piece type of 6 or 7 is never valid.
  - A piece has its most-significant bit clear if it is a white piece, and set if it is a black piece.
  - Nonexistent piece entries may be left at zero.
- An 8-bit side-to-move field.
  - The most-significant bit is clear if white is to move, and set if black is to move.
- An 8-bit halfmove clock.
  - (This field may be left at zero.)
- A 16-bit fullmove counter.
  - (This field may be left at zero.)
- A `Score` for the position.
  - (This field may be left at zero.)
- An 8-bit game-result field; a black win is 0, a draw is 1, a white win is 2. No other values are valid.
- An unused extra byte.

A `Move` is a structure packed into a 16-bit integer:
- A 6-bit from square.
- A 6-bit to square.
- A 1-bit promotion flag.
  - This bit is set if the move is a promotion, and clear if not.
- Three unused bits.
  - These bits must be left clear.

A `Score` is a signed 16-bit integer representing a white-relative score for said `Move`.

# Example

\<TODO\>
