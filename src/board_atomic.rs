//! Atomic-Chess-Backend auf shakmaty-Basis.
//!
//! shakmaty ist fuer Chess960 bereits eine Infrastruktur-Abhaengigkeit und
//! bildet hier auch die Atomic-Regeln ab: Jeder Schlag entfernt Schlagfigur
//! und Opfer sowie alle angrenzenden Nicht-Bauern. Wer den gegnerischen
//! Koenig explodiert, gewinnt; Schach, Koenigsnaehe, Rochade und en passant
//! werden von shakmatys `Atomic`-Position regelkonform behandelt.
//!
//! Wie `Board960` spiegelt dieser Adapter den Zustand in die kleinen
//! `chess`-Werttypen, damit Martunis eigene Suche und Bewertung unveraendert
//! generisch bleiben. Polyglot und Syzygy sind bewusst deaktiviert, weil
//! deren Daten orthodoxe Schachregeln voraussetzen.

use crate::backend::{EngineBoard, MoveGenLike, VariantKind};
use chess::{BitBoard, Board, BoardStatus, ChessMove, Color, Piece, Square, ALL_SQUARES, EMPTY};
use shakmaty::fen::Fen;
use shakmaty::uci::UciMove;
use shakmaty::variant::Atomic;
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
fn piece_of_role(r: Role) -> Piece {
    match r {
        Role::Pawn => Piece::Pawn,
        Role::Knight => Piece::Knight,
        Role::Bishop => Piece::Bishop,
        Role::Rook => Piece::Rook,
        Role::Queen => Piece::Queen,
        Role::King => Piece::King,
    }
}

/// Atomic nutzt Standard-UCI-Rochade (`e1g1`), nicht die im Chess960-
/// Adapter benoetigte Koenig-x-Turm-Notation.
fn cm_of(m: &ShakMove) -> ChessMove {
    let uci = UciMove::from_move(*m, CastlingMode::Standard);
    ChessMove::new(
        sq(uci.from().expect("Atomic erzeugt keine Put-/Null-Zuege")),
        sq(uci.to().expect("Atomic erzeugt keine Put-/Null-Zuege")),
        uci.promotion().map(piece_of_role),
    )
}

#[derive(Clone)]
struct MoveEntry {
    cm: ChessMove,
    sm: ShakMove,
}

#[derive(Clone)]
pub struct BoardAtomic {
    pos: Atomic,
    pieces: [BitBoard; 6],
    colors: [BitBoard; 2],
    combined: BitBoard,
    checkers: BitBoard,
    // In einer terminalen Atomic-Stellung fehlt der explodierte Koenig.
    // Die Platzhalter werden nie evaluiert: `status`/`is_variant_loss`
    // schneiden die Stellung vorher als Matt ab.
    kings: [Square; 2],
    ep_pawn: Option<Square>,
    hash: u64,
    moves: Arc<Vec<MoveEntry>>,
    variant_loss: bool,
}

impl BoardAtomic {
    fn from_pos(pos: Atomic) -> Self {
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
            b.king_of(ShakColor::White).map(sq).unwrap_or(Square::A1),
            b.king_of(ShakColor::Black).map(sq).unwrap_or(Square::A8),
        ];
        let variant_loss = b.king_of(pos.turn()).is_none();
        let ep_pawn = pos.ep_square(EnPassantMode::Legal).map(|target| {
            let idx = target as usize;
            let pawn_idx = if idx / 8 == 5 { idx - 8 } else { idx + 8 };
            ALL_SQUARES[pawn_idx]
        });
        let hash = pos.zobrist_hash::<Zobrist64>(EnPassantMode::Legal).0;
        let moves = Arc::new(
            pos.legal_moves()
                .iter()
                .map(|m| MoveEntry {
                    cm: cm_of(m),
                    sm: *m,
                })
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
            variant_loss,
            pos,
        }
    }

    fn find_move(&self, mv: ChessMove) -> Option<&MoveEntry> {
        self.moves.iter().find(|entry| entry.cm == mv)
    }
}

impl EngineBoard for BoardAtomic {
    type Gen = GenAtomic;

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
        if self.variant_loss {
            BoardStatus::Checkmate
        } else if !self.moves.is_empty() {
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
            .unwrap_or_else(|| panic!("BoardAtomic::make_move_new: illegaler Zug {}", mv));
        let mut pos = self.pos.clone();
        pos.play_unchecked(entry.sm);
        Self::from_pos(pos)
    }

    // Null-Move-Pruning ist fuer Atomic deaktiviert. Ein erfundener Passzug
    // waere wegen der explosionsbasierten Schachdefinition besonders riskant.
    fn null_move(&self) -> Option<Self> {
        None
    }

    fn legal_gen(&self) -> GenAtomic {
        GenAtomic {
            moves: Arc::clone(&self.moves),
            yielded: [0; 4],
            mask: !EMPTY,
            cursor: 0,
        }
    }

    fn uses_standard_rules(&self) -> bool {
        false
    }

    fn variant_kind(&self) -> VariantKind {
        VariantKind::Atomic
    }

    fn is_variant_loss(&self) -> bool {
        self.variant_loss
    }

    fn is_capture(&self, mv: ChessMove) -> bool {
        self.find_move(mv)
            .map(|entry| entry.sm.is_capture())
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
        Self::from_pos(Atomic::default())
    }

    fn from_fen(fen: &str) -> Result<Self, String> {
        let parsed =
            Fen::from_ascii(fen.trim().as_bytes()).map_err(|e| format!("Invalid FEN: {}", e))?;
        let pos: Atomic = parsed
            .into_position(CastlingMode::Standard)
            .map_err(|e| format!("Invalid atomic position: {}", e))?;
        Ok(Self::from_pos(pos))
    }

    fn parse_uci_move(&self, uci: &str) -> Result<ChessMove, String> {
        let parsed = UciMove::from_ascii(uci.trim().as_bytes())
            .map_err(|_| format!("Invalid UCI move: {}", uci))?;
        let m = parsed
            .to_move(&self.pos)
            .map_err(|_| format!("Illegal atomic move: {}", uci))?;
        Ok(cm_of(&m))
    }
}

pub struct GenAtomic {
    moves: Arc<Vec<MoveEntry>>,
    yielded: [u64; 4],
    mask: BitBoard,
    cursor: usize,
}

impl Iterator for GenAtomic {
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

impl MoveGenLike for GenAtomic {
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

    fn perft(board: &BoardAtomic, depth: u32) -> u64 {
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
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            "rn5r/pp4pp/2p3Nn/5p2/1b2P1PP/8/PPP2P2/R1B1KB1R b KQ - 0 9",
        ];
        for fen in fens {
            let board = BoardAtomic::from_fen(fen).unwrap();
            let reference: Atomic = Fen::from_ascii(fen.as_bytes())
                .unwrap()
                .into_position(CastlingMode::Standard)
                .unwrap();
            for depth in 1..=3 {
                assert_eq!(perft(&board, depth), shakmaty::perft(&reference, depth));
            }
        }
    }

    #[test]
    fn capture_explodes_king_and_is_terminal_loss() {
        let board = BoardAtomic::from_fen("4k3/4p3/8/8/8/8/8/4R1K1 w - - 0 1").unwrap();
        let mv = board.parse_uci_move("e1e7").unwrap();
        assert!(board.is_capture(mv));
        let after = board.make_move_new(mv);
        assert_eq!(after.pieces(Piece::King).popcnt(), 1);
        assert!(after.is_variant_loss());
        assert_eq!(after.status(), BoardStatus::Checkmate);
    }

    #[test]
    fn adjacent_pawn_survives_explosion() {
        let board = BoardAtomic::from_fen("7k/8/8/3rP3/8/1B6/8/7K w - - 0 1").unwrap();
        let after = board.make_move_new(board.parse_uci_move("b3d5").unwrap());
        assert_eq!(after.piece_on(Square::D5), None);
        assert_eq!(after.piece_on(Square::E5), Some(Piece::Pawn));
    }

    #[test]
    fn startpos_and_standard_castling_notation() {
        assert_eq!(BoardAtomic::startpos().legal_gen().count(), 20);
        let board = BoardAtomic::from_fen("4k3/8/8/8/8/8/8/R3K2R w KQ - 0 1").unwrap();
        let mv = board.parse_uci_move("e1g1").expect("Standard-UCI-Rochade");
        let after = board.make_move_new(mv);
        assert_eq!(after.king_square(Color::White), Square::G1);
        assert_eq!(after.piece_on(Square::F1), Some(Piece::Rook));
    }
}
