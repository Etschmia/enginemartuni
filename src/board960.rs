//! Chess960-Backend auf shakmaty-Basis.
//!
//! `Board960` implementiert `EngineBoard` (src/backend.rs) und macht damit
//! die komplette Suche/Eval fuer Chess960 nutzbar, ohne den Standard-Pfad
//! (jordanbray `chess`) anzufassen. Arbeitsteilung:
//!
//!   - shakmaty haelt den SPIELZUSTAND (Position, 960-Rochaderechte mit
//!     freien Turm-Startfeldern, Shredder-/X-FEN-Parsing, Zobrist-Hash)
//!     und generiert die legalen Zuege.
//!   - Beim Bau eines `Board960` werden Bitboards, Koenigsfelder, Checkers
//!     und die Zugliste EINMAL in `chess`-Typen gespiegelt. Eval, SEE,
//!     PSTs und alle statischen Angriffstabellen rechnen dann exakt wie im
//!     Standard-Backend auf diesen Bitboards — kein Term muss 960 "kennen".
//!
//! Rochade-Codierung: `ChessMove { from: Koenigsfeld, to: Turmfeld }` —
//! die UCI_Chess960-Notation (z. B. `e1h1`). `move_to_uci` druckt sie damit
//! unveraendert korrekt; `is_capture` behandelt sie als Nicht-Schlagzug.
//!
//! Bewusste v1-Vereinfachungen (Kosten nur im 960-Modus):
//!   - Zugliste wird pro Knoten eager erzeugt (kein Lazy-Staging wie die
//!     chess-Crate-MoveGen) und der Zobrist-Hash pro Stellung komplett neu
//!     berechnet — Korrektheit vor NPS.
//!   - Polyglot-Buch bleibt aus (`as_std() == None`), Syzygy funktioniert
//!     ueber die generische Probe (Endspiele sind in 960 identisch).

use crate::backend::{EngineBoard, MoveGenLike};
use chess::{BitBoard, Board, BoardStatus, ChessMove, Color, Piece, Square, ALL_SQUARES, EMPTY};
use shakmaty::fen::Fen;
use shakmaty::uci::UciMove;
use shakmaty::zobrist::{Zobrist64, ZobristHash};
use shakmaty::{
    CastlingMode, Chess, Color as ShakColor, EnPassantMode, Move as ShakMove,
    Position as ShakPositionTrait, Role, Square as ShakSquare,
};

// --- Typ-Konvertierungen ---------------------------------------------------
// Beide Crates nummerieren Felder identisch (A1=0 … H8=63, zeilenweise),
// die Umrechnung ist also ein reiner Index-Transfer.

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

/// shakmaty-Zug → `ChessMove` (Rochade als Koenig-x-Turm, s. Modulkopf).
fn cm_of(m: &ShakMove) -> ChessMove {
    match *m {
        ShakMove::Normal {
            from, to, promotion, ..
        } => ChessMove::new(sq(from), sq(to), promotion.map(piece_of_role)),
        ShakMove::EnPassant { from, to } => ChessMove::new(sq(from), sq(to), None),
        ShakMove::Castle { king, rook } => ChessMove::new(sq(king), sq(rook), None),
        // Put ist Crazyhouse-only — in Chess960 nicht erzeugbar.
        ShakMove::Put { .. } => unreachable!("Put-Move in Chess960"),
    }
}

#[derive(Clone)]
struct MoveEntry {
    cm: ChessMove,
    sm: ShakMove,
}

#[derive(Clone)]
pub struct Board960 {
    pos: Chess,
    // In chess-Typen gespiegelter Zustand (einmal beim Bau berechnet):
    pieces: [BitBoard; 6],
    colors: [BitBoard; 2],
    combined: BitBoard,
    checkers: BitBoard,
    kings: [Square; 2],
    /// chess-Crate-Semantik: Feld des en-passant-schlagbaren BAUERN (e5),
    /// nicht das Zielfeld (e6).
    ep_pawn: Option<Square>,
    hash: u64,
    /// Legale Zuege dieser Stellung (eager, s. Modulkopf).
    moves: Vec<MoveEntry>,
}

impl Board960 {
    fn from_pos(pos: Chess) -> Self {
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
            sq(b.king_of(ShakColor::White).expect("Weiss hat einen Koenig")),
            sq(b.king_of(ShakColor::Black).expect("Schwarz hat einen Koenig")),
        ];
        // shakmaty liefert das ZIELfeld (e6); chess-Semantik will das
        // Bauernfeld (e5) → eine Reihe zurueck Richtung Zugseite des Bauern.
        let ep_pawn = pos.ep_square(EnPassantMode::Legal).map(|t| {
            let idx = t as usize;
            let pawn_idx = if idx / 8 == 5 { idx - 8 } else { idx + 8 };
            ALL_SQUARES[pawn_idx]
        });
        let hash = pos.zobrist_hash::<Zobrist64>(EnPassantMode::Legal).0;
        let moves = pos
            .legal_moves()
            .iter()
            .map(|m| MoveEntry { cm: cm_of(m), sm: *m })
            .collect();

        Board960 {
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
        self.moves.iter().find(|e| e.cm == mv)
    }
}

impl EngineBoard for Board960 {
    type Gen = Gen960;

    #[inline]
    fn pieces(&self, piece: Piece) -> &BitBoard {
        &self.pieces[piece.to_index()]
    }

    #[inline]
    fn color_combined(&self, color: Color) -> &BitBoard {
        &self.colors[color.to_index()]
    }

    #[inline]
    fn combined(&self) -> &BitBoard {
        &self.combined
    }

    #[inline]
    fn side_to_move(&self) -> Color {
        match self.pos.turn() {
            ShakColor::White => Color::White,
            ShakColor::Black => Color::Black,
        }
    }

    #[inline]
    fn king_square(&self, color: Color) -> Square {
        self.kings[color.to_index()]
    }

    #[inline]
    fn checkers(&self) -> &BitBoard {
        &self.checkers
    }

    fn piece_on(&self, square: Square) -> Option<Piece> {
        self.pos
            .board()
            .role_at(ShakSquare::new(square.to_index() as u32))
            .map(piece_of_role)
    }

    #[inline]
    fn en_passant(&self) -> Option<Square> {
        self.ep_pawn
    }

    #[inline]
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
            .unwrap_or_else(|| panic!("Board960::make_move_new: illegaler Zug {}", mv));
        let mut pos = self.pos.clone();
        pos.play_unchecked(entry.sm);
        Board960::from_pos(pos)
    }

    fn null_move(&self) -> Option<Self> {
        if self.checkers != EMPTY {
            return None;
        }
        self.pos.clone().swap_turn().ok().map(Board960::from_pos)
    }

    fn legal_gen(&self) -> Gen960 {
        Gen960 {
            moves: self
                .moves
                .iter()
                .map(|e| (e.cm, BitBoard::from_square(e.cm.get_dest())))
                .collect(),
            yielded: vec![false; self.moves.len()],
            mask: !EMPTY,
        }
    }

    fn is_capture(&self, mv: ChessMove) -> bool {
        let dest_bb = BitBoard::from_square(mv.get_dest());
        let us = self.side_to_move();
        if self.colors[(!us).to_index()] & dest_bb != EMPTY {
            return true;
        }
        // Eigene Figur auf dem Zielfeld = Rochade (Koenig x eigener Turm).
        if self.colors[us.to_index()] & dest_bb != EMPTY {
            return false;
        }
        // en passant: Bauer wechselt die Linie auf ein leeres Feld
        self.piece_on(mv.get_source()) == Some(Piece::Pawn)
            && mv.get_source().get_file() != mv.get_dest().get_file()
    }

    fn has_castle_rights(&self) -> bool {
        self.pos.castles().any()
    }

    fn as_std(&self) -> Option<&Board> {
        // Kein Standard-Board vorhanden → Polyglot-Buch bleibt im 960-Modus aus.
        None
    }

    fn startpos() -> Self {
        Board960::from_pos(Chess::default())
    }

    fn from_fen(fen: &str) -> Result<Self, String> {
        let parsed = Fen::from_ascii(fen.trim().as_bytes())
            .map_err(|e| format!("Invalid FEN: {}", e))?;
        let pos: Chess = parsed
            .into_position(CastlingMode::Chess960)
            .map_err(|e| format!("Invalid position: {}", e))?;
        Ok(Board960::from_pos(pos))
    }

    fn parse_uci_move(&self, uci: &str) -> Result<ChessMove, String> {
        let parsed = UciMove::from_ascii(uci.trim().as_bytes())
            .map_err(|_| format!("Invalid UCI move: {}", uci))?;
        let m = parsed
            .to_move(&self.pos)
            .map_err(|_| format!("Illegal move: {}", uci))?;
        Ok(cm_of(&m))
    }
}

/// Zuggenerator im `chess::MoveGen`-Stil: Zielfeld-Maske nachtraeglich
/// setzbar, bereits ausgegebene Zuege werden nach Maskenwechsel nicht
/// wiederholt (Quiescence: erst Captures, dann `!EMPTY` fuer den Rest).
pub struct Gen960 {
    moves: Vec<(ChessMove, BitBoard)>,
    yielded: Vec<bool>,
    mask: BitBoard,
}

impl Iterator for Gen960 {
    type Item = ChessMove;

    fn next(&mut self) -> Option<ChessMove> {
        for i in 0..self.moves.len() {
            if !self.yielded[i] && self.moves[i].1 & self.mask != EMPTY {
                self.yielded[i] = true;
                return Some(self.moves[i].0);
            }
        }
        None
    }
}

impl MoveGenLike for Gen960 {
    fn set_iterator_mask(&mut self, mask: BitBoard) {
        self.mask = mask;
    }

    fn count_remaining(&self) -> usize {
        self.yielded.iter().filter(|y| !**y).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Perft ueber die EngineBoard-Schnittstelle (legal_gen + make_move_new)
    /// — testet die komplette Konvertierungsschicht, nicht shakmaty selbst.
    fn perft(board: &Board960, depth: u32) -> u64 {
        if depth == 0 {
            return 1;
        }
        let mut nodes = 0;
        for mv in board.legal_gen() {
            let nb = board.make_move_new(mv);
            nodes += perft(&nb, depth - 1);
        }
        nodes
    }

    #[test]
    fn perft_startpos_matches_known_values() {
        let b = Board960::startpos();
        assert_eq!(perft(&b, 1), 20);
        assert_eq!(perft(&b, 2), 400);
        assert_eq!(perft(&b, 3), 8_902);
        assert_eq!(perft(&b, 4), 197_281);
    }

    /// Adapter-Perft gegen shakmaty::perft — beide muessen fuer dieselbe
    /// Stellung identisch zaehlen. FRC-Positionen mit Rochade-Situationen.
    #[test]
    fn perft_frc_positions_match_shakmaty() {
        let fens = [
            // FRC-Startstellungen (Shredder-FEN-Rochaderechte)
            "bqnb1rkr/pp3ppp/3ppn2/2p5/5P2/P2P4/NPP1P1PP/BQ1BNRKR w HFhf - 2 9",
            "nrbbnkqr/ppppp1pp/8/5p2/8/1P4P1/P1PPPP1P/NRBBNKQR w HBhb - 0 9",
            "rkbqnbnr/ppp1pppp/8/3p4/3P4/8/PPP1PPPP/RKBQNBNR w HAha - 0 9",
            // Mitten in einer 960-Partie, beide Seiten rochadeberechtigt
            "2rkr3/5pp1/8/1p2p3/1P2P3/3P4/5PP1/2RKR3 w ECec - 0 1",
        ];
        for fen in fens {
            let b = Board960::from_fen(fen).expect("FEN parst");
            let shak: Chess = Fen::from_ascii(fen.as_bytes())
                .unwrap()
                .into_position(CastlingMode::Chess960)
                .unwrap();
            for depth in 1..=3 {
                assert_eq!(
                    perft(&b, depth),
                    shakmaty::perft(&shak, depth),
                    "Perft-Abweichung bei {fen} depth {depth}"
                );
            }
        }
    }

    #[test]
    fn castling_move_is_king_takes_rook_and_no_capture() {
        // 960-Stellung, Weiss kann rochieren: Koenig d1, Turm e1/c1 (ECec).
        let b = Board960::from_fen("2rkr3/2pppp2/8/8/8/8/2PPPP2/2RKR3 w ECec - 0 1").unwrap();
        let castles: Vec<ChessMove> = b
            .legal_gen()
            .filter(|m| {
                b.piece_on(m.get_source()) == Some(Piece::King)
                    && b.piece_on(m.get_dest()) == Some(Piece::Rook)
            })
            .collect();
        assert_eq!(castles.len(), 2, "beide Rochaden (d1e1, d1c1) erwartet");
        for c in &castles {
            assert!(!b.is_capture(*c), "Rochade darf kein Capture sein");
            let nb = b.make_move_new(*c);
            // Nach der Rochade steht der Koenig auf g1 bzw. c1 (960-Regel).
            let ksq = nb.king_square(Color::White);
            assert!(
                ksq == Square::G1 || ksq == Square::C1,
                "Koenig nach Rochade auf {ksq}"
            );
        }
    }

    #[test]
    fn parse_uci_castling_960_notation() {
        let b = Board960::from_fen("2rkr3/2pppp2/8/8/8/8/2PPPP2/2RKR3 w ECec - 0 1").unwrap();
        // UCI_Chess960-Notation: Koenig x eigener Turm.
        let m = b.parse_uci_move("d1e1").expect("d1e1 = kurze Rochade");
        let nb = b.make_move_new(m);
        assert_eq!(nb.king_square(Color::White), Square::G1);
        assert_eq!(nb.piece_on(Square::F1), Some(Piece::Rook));
    }

    #[test]
    fn en_passant_square_uses_pawn_semantics() {
        // Nach 1. e4 (Standard-Geometrie) mit schwarzem Bauern d4: chess-
        // Semantik verlangt das BAUERN-Feld e4, nicht das Zielfeld e3.
        let b = Board960::from_fen(
            "rnbqkbnr/ppp1pppp/8/8/3p4/8/PPPPPPPP/RNBQKBNR w KQkq - 0 3",
        )
        .unwrap();
        let after_e4 = b.make_move_new(b.parse_uci_move("e2e4").unwrap());
        assert_eq!(after_e4.en_passant(), Some(Square::E4));
    }

    #[test]
    fn hash_changes_and_startpos_status() {
        let b = Board960::startpos();
        assert_eq!(b.status(), BoardStatus::Ongoing);
        let nb = b.make_move_new(b.parse_uci_move("e2e4").unwrap());
        assert_ne!(b.get_hash(), nb.get_hash());
        assert_eq!(b.side_to_move(), Color::White);
        assert_eq!(nb.side_to_move(), Color::Black);
    }
}
