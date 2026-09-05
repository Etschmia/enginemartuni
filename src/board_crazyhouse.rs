//! Crazyhouse-Backend auf shakmaty-Basis.
//!
//! Neben der regelkonformen Brettlogik verwaltet shakmaty die Taschen,
//! umgewandelte Figuren und Drop-Zuege. Martunis Suche spricht weiterhin
//! `chess::ChessMove`; ein Drop wird kollisionsfrei als `to -> to` codiert,
//! wobei das Promotion-Feld die eingesetzte Figur traegt. An der UCI-Grenze
//! wird daraus wieder die uebliche Notation `N@f7`.

use crate::backend::{EngineBoard, MoveGenLike, VariantKind};
use chess::{BitBoard, Board, BoardStatus, ChessMove, Color, Piece, Square, ALL_SQUARES, EMPTY};
use shakmaty::fen::Fen;
use shakmaty::uci::UciMove;
use shakmaty::variant::Crazyhouse;
use shakmaty::zobrist::{Zobrist64, ZobristHash};
use shakmaty::{
    CastlingMode, Color as ShakColor, EnPassantMode, Move as ShakMove,
    Position as ShakPositionTrait, Role, Square as ShakSquare,
};
use std::sync::Arc;

#[inline]
fn sq(s: ShakSquare) -> Square {
    ALL_SQUARES[s as usize]
}

#[inline]
fn piece_of_role(role: Role) -> Piece {
    match role {
        Role::Pawn => Piece::Pawn,
        Role::Knight => Piece::Knight,
        Role::Bishop => Piece::Bishop,
        Role::Rook => Piece::Rook,
        Role::Queen => Piece::Queen,
        Role::King => Piece::King,
    }
}

#[inline]
fn role_of_piece(piece: Piece) -> Role {
    match piece {
        Piece::Pawn => Role::Pawn,
        Piece::Knight => Role::Knight,
        Piece::Bishop => Role::Bishop,
        Piece::Rook => Role::Rook,
        Piece::Queen => Role::Queen,
        Piece::King => Role::King,
    }
}

fn cm_of(m: &ShakMove) -> ChessMove {
    match UciMove::from_move(*m, CastlingMode::Standard) {
        UciMove::Normal { from, to, promotion } => ChessMove::new(
            sq(from),
            sq(to),
            promotion.map(piece_of_role),
        ),
        UciMove::Put { role, to } => {
            let to = sq(to);
            ChessMove::new(to, to, Some(piece_of_role(role)))
        }
        UciMove::Null => unreachable!("Crazyhouse erzeugt keine Nullzuege"),
    }
}

#[derive(Clone)]
struct MoveEntry {
    cm: ChessMove,
    sm: ShakMove,
}

#[derive(Clone)]
pub struct BoardCrazyhouse {
    pos: Crazyhouse,
    pieces: [BitBoard; 6],
    colors: [BitBoard; 2],
    combined: BitBoard,
    checkers: BitBoard,
    kings: [Square; 2],
    ep_pawn: Option<Square>,
    hash: u64,
    moves: Arc<Vec<MoveEntry>>,
}

impl BoardCrazyhouse {
    fn from_pos(pos: Crazyhouse) -> Self {
        let b = pos.board().clone();
        let pieces = [
            BitBoard(b.by_role(Role::Pawn).0),
            BitBoard(b.by_role(Role::Knight).0),
            BitBoard(b.by_role(Role::Bishop).0),
            BitBoard(b.by_role(Role::Rook).0),
            BitBoard(b.by_role(Role::Queen).0),
            BitBoard(b.by_role(Role::King).0),
        ];
        let colors = [
            BitBoard(b.by_color(ShakColor::White).0),
            BitBoard(b.by_color(ShakColor::Black).0),
        ];
        let kings = [
            sq(b.king_of(ShakColor::White).expect("Crazyhouse braucht weissen Koenig")),
            sq(b.king_of(ShakColor::Black).expect("Crazyhouse braucht schwarzen Koenig")),
        ];
        let ep_pawn = pos.ep_square(EnPassantMode::Legal).map(|target| {
            let idx = target as usize;
            let pawn_idx = if idx / 8 == 5 { idx - 8 } else { idx + 8 };
            ALL_SQUARES[pawn_idx]
        });
        let hash = pos.zobrist_hash::<Zobrist64>(EnPassantMode::Legal).0;
        let moves = Arc::new(
            pos.legal_moves()
                .iter()
                .map(|m| MoveEntry { cm: cm_of(m), sm: *m })
                .collect(),
        );

        Self {
            checkers: BitBoard(pos.checkers().0),
            combined: BitBoard(b.occupied().0),
            pieces,
            colors,
            kings,
            ep_pawn,
            hash,
            moves,
            pos,
        }
    }

    fn find_move(&self, mv: ChessMove) -> Option<&MoveEntry> {
        self.moves.iter().find(|entry| entry.cm == mv)
    }
}

impl EngineBoard for BoardCrazyhouse {
    type Gen = GenCrazyhouse;

    fn pieces(&self, piece: Piece) -> &BitBoard {
        &self.pieces[piece.to_index()]
    }

    fn color_combined(&self, color: Color) -> &BitBoard {
        &self.colors[color.to_index()]
    }

    fn combined(&self) -> &BitBoard {
        &self.combined
    }

    fn side_to_move(&self) -> Color {
        match self.pos.turn() {
            ShakColor::White => Color::White,
            ShakColor::Black => Color::Black,
        }
    }

    fn king_square(&self, color: Color) -> Square {
        self.kings[color.to_index()]
    }

    fn checkers(&self) -> &BitBoard {
        &self.checkers
    }

    fn piece_on(&self, square: Square) -> Option<Piece> {
        self.pos
            .board()
            .role_at(ShakSquare::new(square.to_index() as u32))
            .map(piece_of_role)
    }

    fn en_passant(&self) -> Option<Square> {
        self.ep_pawn
    }

    fn get_hash(&self) -> u64 {
        self.hash
    }

    fn status(&self) -> BoardStatus {
        if !self.moves.is_empty() {
            BoardStatus::Ongoing
        } else if self.checkers != EMPTY {
            BoardStatus::Checkmate
        } else {
            BoardStatus::Stalemate
        }
    }

    fn make_move_new(&self, mv: ChessMove) -> Self {
        let entry = self
            .find_move(mv)
            .unwrap_or_else(|| panic!("BoardCrazyhouse::make_move_new: illegaler Zug {}", mv));
        let mut pos = self.pos.clone();
        pos.play_unchecked(entry.sm);
        Self::from_pos(pos)
    }

    // Null-Move-, Futility- und andere orthodoxe Annahmen bleiben im
    // Variantenpfad aus. Drops veraendern die Zugzwang-Semantik grundlegend.
    fn null_move(&self) -> Option<Self> {
        None
    }

    fn legal_gen(&self) -> GenCrazyhouse {
        GenCrazyhouse {
            moves: Arc::clone(&self.moves),
            yielded: vec![0; self.moves.len().div_ceil(64)],
            mask: !EMPTY,
            cursor: 0,
        }
    }

    fn uses_standard_rules(&self) -> bool {
        false
    }

    fn variant_kind(&self) -> VariantKind {
        VariantKind::Crazyhouse
    }

    fn is_drop(&self, mv: ChessMove) -> bool {
        self.find_move(mv)
            .map(|entry| matches!(entry.sm, ShakMove::Put { .. }))
            .unwrap_or(false)
    }

    fn pocket_count(&self, color: Color, piece: Piece) -> u8 {
        let shak = match color {
            Color::White => ShakColor::White,
            Color::Black => ShakColor::Black,
        };
        *self
            .pos
            .pockets()
            .expect("Crazyhouse hat Taschen")
            .get(shak)
            .get(role_of_piece(piece))
    }

    fn is_capture(&self, mv: ChessMove) -> bool {
        self.find_move(mv)
            .map(|entry| entry.sm.is_capture())
            .unwrap_or(false)
    }

    fn resets_halfmove(&self, mv: ChessMove) -> bool {
        self.find_move(mv)
            .map(|entry| entry.sm.is_zeroing())
            .unwrap_or(false)
    }

    fn has_castle_rights(&self) -> bool {
        self.pos.castles().any()
    }

    fn has_castle_rights_for(&self, color: Color) -> bool {
        let shak = match color {
            Color::White => ShakColor::White,
            Color::Black => ShakColor::Black,
        };
        self.pos.castles().has_color(shak)
    }

    fn as_std(&self) -> Option<&Board> {
        None
    }

    fn startpos() -> Self {
        Self::from_pos(Crazyhouse::default())
    }

    fn from_fen(fen: &str) -> Result<Self, String> {
        let parsed =
            Fen::from_ascii(fen.trim().as_bytes()).map_err(|e| format!("Invalid FEN: {}", e))?;
        let pos: Crazyhouse = parsed
            .into_position(CastlingMode::Standard)
            .map_err(|e| format!("Invalid crazyhouse position: {}", e))?;
        Ok(Self::from_pos(pos))
    }

    fn parse_uci_move(&self, uci: &str) -> Result<ChessMove, String> {
        let parsed = UciMove::from_ascii(uci.trim().as_bytes())
            .map_err(|_| format!("Invalid UCI move: {}", uci))?;
        let m = parsed
            .to_move(&self.pos)
            .map_err(|_| format!("Illegal crazyhouse move: {}", uci))?;
        Ok(cm_of(&m))
    }
}

pub struct GenCrazyhouse {
    moves: Arc<Vec<MoveEntry>>,
    yielded: Vec<u64>,
    mask: BitBoard,
    cursor: usize,
}

impl Iterator for GenCrazyhouse {
    type Item = ChessMove;

    fn next(&mut self) -> Option<Self::Item> {
        while self.cursor < self.moves.len() {
            let i = self.cursor;
            self.cursor += 1;
            if self.yielded[i / 64] & (1u64 << (i % 64)) != 0 {
                continue;
            }
            let cm = self.moves[i].cm;
            if BitBoard::from_square(cm.get_dest()) & self.mask != EMPTY {
                self.yielded[i / 64] |= 1u64 << (i % 64);
                return Some(cm);
            }
        }
        None
    }
}

impl MoveGenLike for GenCrazyhouse {
    fn set_iterator_mask(&mut self, mask: BitBoard) {
        self.mask = mask;
        self.cursor = 0;
    }

    fn count_remaining(&self) -> usize {
        let done: u32 = self.yielded.iter().map(|word| word.count_ones()).sum();
        self.moves.len() - done as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::move_to_uci;

    fn perft(board: &BoardCrazyhouse, depth: u32) -> u64 {
        if depth == 0 {
            return 1;
        }
        board
            .legal_gen()
            .map(|mv| perft(&board.make_move_new(mv), depth - 1))
            .sum()
    }

    #[test]
    fn adapter_perft_matches_shakmaty() {
        let fens = [
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR[] w KQkq - 0 1",
            "4k3/8/8/8/8/8/8/4K3[PNq] w - - 0 1",
        ];
        for fen in fens {
            let board = BoardCrazyhouse::from_fen(fen).unwrap();
            let reference: Crazyhouse = Fen::from_ascii(fen.as_bytes())
                .unwrap()
                .into_position(CastlingMode::Standard)
                .unwrap();
            for depth in 1..=2 {
                assert_eq!(perft(&board, depth), shakmaty::perft(&reference, depth));
            }
        }
    }

    #[test]
    fn drop_roundtrips_through_common_move_encoding() {
        let board = BoardCrazyhouse::from_fen("4k3/8/8/8/8/8/8/4K3[N] w - - 0 1").unwrap();
        let mv = board.parse_uci_move("N@f7").unwrap();
        assert!(board.is_drop(mv));
        assert_eq!(move_to_uci(mv), "N@f7");
        let after = board.make_move_new(mv);
        assert_eq!(after.piece_on(Square::F7), Some(Piece::Knight));
        assert_eq!(after.pocket_count(Color::White, Piece::Knight), 0);
    }

    #[test]
    fn capture_adds_captured_role_to_pocket() {
        let board = BoardCrazyhouse::from_fen("4k3/8/8/3n4/4P3/8/8/4K3[] w - - 0 1").unwrap();
        let after = board.make_move_new(board.parse_uci_move("e4d5").unwrap());
        assert_eq!(after.pocket_count(Color::White, Piece::Knight), 1);
    }
}
