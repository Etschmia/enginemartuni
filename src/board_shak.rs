//! Generischer shakmaty-Adapter fuer die "kleinen" Lichess-Varianten:
//! Antichess (Raeuberschach), King of the Hill, Horde, Three-Check und
//! Racing Kings.
//!
//! Vorbild ist `src/board_atomic.rs`: shakmaty liefert die regelkonforme
//! Zuggenerierung und das Spielende (`variant_outcome`), der Adapter
//! spiegelt den Zustand in die kleinen `chess`-Werttypen (`BitBoard`,
//! `Square`, `Piece`, `ChessMove`), damit Martunis Suche und Bewertung
//! unveraendert generisch ueber `EngineBoard` laufen. Statt fuenf fast
//! identische Adapter zu pflegen, ist dieser EINE Adapter ueber das kleine
//! Trait `ShakVariant` parametrisiert — die Varianten unterscheiden sich
//! fuer den Adapter nur in ihrem shakmaty-Positionstyp und der
//! `VariantKind`-Kennung, die Eval und Suche zum Dispatch nutzen.
//!
//! Besonderheiten gegenueber Atomic:
//!   - Koenigslose Seiten (Antichess nach Koenigsverlust, Weiss in Horde):
//!     `king_square` liefert nur einen Platzhalter (A1/A8), `has_king` ist
//!     `false`. Alle Koenigs-Terme der Eval sind darauf geguardet.
//!   - Varianten-SIEG der Seite am Zug (`is_variant_win`): in Antichess
//!     gewinnt, wer keine Steine oder keine Zuege mehr hat; in Racing Kings
//!     gewinnt Weiss, wenn Schwarz nicht mehr nachziehen konnte und Weiss
//!     wieder am Zug waere. Die Suche prueft das bei leerer Zugliste.
//!   - Varianten-REMIS (`is_variant_draw`): Racing Kings, beide Koenige
//!     auf der 8. Reihe.
//!   - Three-Check: shakmaty zaehlt die verbleibenden Schachs mit und nimmt
//!     sie in den Zobrist-Hash auf; Lichess-FENs mit "3+3"-Feld werden
//!     direkt geparst.
//!   - Antichess-Umwandlung in einen Koenig ist erlaubt (`e7e8k`).
//!
//! Alle fuenf Varianten nutzen die Standard-Rochadenotation (`e1g1`).
//! Polyglot-Buch und Syzygy sind aus (orthodoxe Daten passen nicht).

use crate::backend::{EngineBoard, MoveGenLike, VariantKind};
use chess::{BitBoard, Board, BoardStatus, ChessMove, Color, Piece, Square, ALL_SQUARES, EMPTY};
use shakmaty::fen::Fen;
use shakmaty::uci::UciMove;
use shakmaty::variant::{Antichess, Horde, KingOfTheHill, RacingKings, ThreeCheck};
use shakmaty::zobrist::{Zobrist64, ZobristHash};
use shakmaty::{
    CastlingMode, Color as ShakColor, EnPassantMode, FromSetup, KnownOutcome, Move as ShakMove,
    Outcome, Position as ShakPositionTrait, Role, Square as ShakSquare,
};
use std::sync::Arc;

/// Bindeglied zwischen einem shakmaty-Positionstyp und Martunis
/// `VariantKind`. Fuer jede unterstuetzte Variante genau eine Impl.
pub trait ShakVariant:
    ShakPositionTrait + FromSetup + Clone + Default + Send + Sync + 'static
{
    const KIND: VariantKind;
}

impl ShakVariant for Antichess {
    const KIND: VariantKind = VariantKind::Antichess;
}

impl ShakVariant for KingOfTheHill {
    const KIND: VariantKind = VariantKind::KingOfTheHill;
}

impl ShakVariant for Horde {
    const KIND: VariantKind = VariantKind::Horde;
}

impl ShakVariant for ThreeCheck {
    const KIND: VariantKind = VariantKind::ThreeCheck;
}

impl ShakVariant for RacingKings {
    const KIND: VariantKind = VariantKind::RacingKings;
}

pub type BoardAntichess = BoardShak<Antichess>;
pub type BoardKingOfTheHill = BoardShak<KingOfTheHill>;
pub type BoardHorde = BoardShak<Horde>;
pub type BoardThreeCheck = BoardShak<ThreeCheck>;
pub type BoardRacingKings = BoardShak<RacingKings>;

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

#[inline]
fn shak_color(c: Color) -> ShakColor {
    match c {
        Color::White => ShakColor::White,
        Color::Black => ShakColor::Black,
    }
}

/// Standard-UCI-Notation (Rochade `e1g1`). Antichess kann in einen Koenig
/// umwandeln — das Promotion-Feld traegt dann `Piece::King`.
fn cm_of(m: &ShakMove) -> ChessMove {
    let uci = UciMove::from_move(*m, CastlingMode::Standard);
    ChessMove::new(
        sq(uci.from().expect("Varianten-Backend erzeugt keine Put-/Null-Zuege")),
        sq(uci.to().expect("Varianten-Backend erzeugt keine Put-/Null-Zuege")),
        uci.promotion().map(piece_of_role),
    )
}

/// Spielstand aus Sicht der Seite am Zug, aus `variant_outcome` abgeleitet.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Terminal {
    Ongoing,
    /// Die Seite am Zug hat nach Variantenregeln gewonnen.
    Win,
    /// Die Seite am Zug hat nach Variantenregeln verloren.
    Loss,
    Draw,
}

#[derive(Clone)]
struct MoveEntry {
    cm: ChessMove,
    sm: ShakMove,
}

#[derive(Clone)]
pub struct BoardShak<P: ShakVariant> {
    pos: P,
    pieces: [BitBoard; 6],
    colors: [BitBoard; 2],
    combined: BitBoard,
    checkers: BitBoard,
    /// Koenigsfelder; fehlt der Koenig (Antichess, Horde-Weiss), steht hier
    /// ein Platzhalter (A1/A8) und `has_king` ist `false`.
    kings: [Square; 2],
    has_king: [bool; 2],
    ep_pawn: Option<Square>,
    hash: u64,
    moves: Arc<Vec<MoveEntry>>,
    terminal: Terminal,
}

impl<P: ShakVariant> BoardShak<P> {
    fn from_pos(pos: P) -> Self {
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
        let wk = b.king_of(ShakColor::White);
        let bk = b.king_of(ShakColor::Black);
        let kings = [
            wk.map(sq).unwrap_or(Square::A1),
            bk.map(sq).unwrap_or(Square::A8),
        ];
        let has_king = [wk.is_some(), bk.is_some()];
        // ep_square liefert das ZIELfeld (z. B. e6); die chess-Crate-
        // Konvention will das Feld des schlagbaren BAUERN (e5). Ein Zielfeld
        // auf Reihe 6 gehoert zu einem weissen Bauern (eine Reihe darunter),
        // sonst zu einem schwarzen (eine Reihe darueber). Horde-Bauern von
        // Reihe 1 erzeugen in shakmaty keinen ep-Fall (nur Reihe 2 → 4).
        let ep_pawn = pos.ep_square(EnPassantMode::Legal).map(|target| {
            let idx = target as usize;
            let pawn_idx = if idx / 8 == 5 { idx - 8 } else { idx + 8 };
            ALL_SQUARES[pawn_idx]
        });
        let hash = pos.zobrist_hash::<Zobrist64>(EnPassantMode::Legal).0;

        let mut moves: Vec<MoveEntry> = pos
            .legal_moves()
            .iter()
            .map(|m| MoveEntry {
                cm: cm_of(m),
                sm: *m,
            })
            .collect();

        // Spielende nach Variantenregeln. Antichess ist der teure Sonderfall:
        // shakmatys `variant_outcome` prueft dort ueber `is_stalemate` ein
        // zweites Mal die komplette Zugliste. Da Antichess kein Schach kennt,
        // ist "keine legalen Zuege" (schliesst "keine Steine" ein) bereits
        // exakt die Gewinnbedingung der Seite am Zug — wir lesen sie direkt
        // von der schon erzeugten Zugliste ab.
        let terminal = if P::KIND == VariantKind::Antichess {
            if moves.is_empty() {
                Terminal::Win
            } else {
                Terminal::Ongoing
            }
        } else {
            match pos.variant_outcome() {
                Outcome::Known(KnownOutcome::Decisive { winner }) => {
                    if winner == pos.turn() {
                        Terminal::Win
                    } else {
                        Terminal::Loss
                    }
                }
                Outcome::Known(KnownOutcome::Draw) => Terminal::Draw,
                Outcome::Unknown => Terminal::Ongoing,
            }
        };
        // Invariante fuer die Suche: Varianten-Endstellung ⇒ leere Zugliste.
        // shakmaty haelt das fuer KotH/Three-Check/Racing Kings selbst ein;
        // fuer per FEN gesetzte Kuriositaeten (z. B. Horde ohne weisse
        // Steine, aber Schwarz am Zug) erzwingen wir es hier.
        if terminal != Terminal::Ongoing {
            moves.clear();
        }

        Self {
            checkers: BitBoard(pos.checkers().0),
            combined: BitBoard(b.occupied().0),
            pieces,
            colors,
            kings,
            has_king,
            ep_pawn,
            hash,
            moves: Arc::new(moves),
            terminal,
            pos,
        }
    }

    fn find_move(&self, mv: ChessMove) -> Option<&MoveEntry> {
        self.moves.iter().find(|entry| entry.cm == mv)
    }

    /// FEN der aktuellen Stellung (inkl. Three-Check-Feld, z. B. `3+3`).
    /// Fuer Tests und Debug-Ausgaben.
    #[allow(dead_code)]
    pub fn to_fen(&self) -> String {
        Fen::from_position(&self.pos, EnPassantMode::Legal).to_string()
    }
}

impl<P: ShakVariant> EngineBoard for BoardShak<P> {
    type Gen = GenShak;

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
        match self.terminal {
            // Aus Sicht der UCI-Schleife ist ein entschiedenes Spiel
            // "Checkmate" (kein bestmove mehr) — egal, wer gewonnen hat.
            Terminal::Win | Terminal::Loss => BoardStatus::Checkmate,
            Terminal::Draw => BoardStatus::Stalemate,
            Terminal::Ongoing => {
                if !self.moves.is_empty() {
                    BoardStatus::Ongoing
                } else if self.checkers != EMPTY {
                    BoardStatus::Checkmate
                } else {
                    BoardStatus::Stalemate
                }
            }
        }
    }

    fn make_move_new(&self, mv: ChessMove) -> Self {
        let entry = self
            .find_move(mv)
            .unwrap_or_else(|| panic!("BoardShak::make_move_new: illegaler Zug {}", mv));
        let mut pos = self.pos.clone();
        pos.play_unchecked(entry.sm);
        Self::from_pos(pos)
    }

    // Null-Move-Pruning ist fuer alle Varianten dieses Adapters aus:
    // Schlagzwang (Antichess), Zielfeld-Siege (KotH/Racing Kings) und
    // Schachzaehlung (Three-Check) machen "passen ist nie besser als ziehen"
    // zu einer unzuverlaessigen Annahme.
    fn null_move(&self) -> Option<Self> {
        None
    }

    fn legal_gen(&self) -> GenShak {
        GenShak {
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
        P::KIND
    }

    fn has_king(&self, color: Color) -> bool {
        self.has_king[color.to_index()]
    }

    fn is_variant_loss(&self) -> bool {
        self.terminal == Terminal::Loss
    }

    fn is_variant_win(&self) -> bool {
        self.terminal == Terminal::Win
    }

    fn is_variant_draw(&self) -> bool {
        self.terminal == Terminal::Draw
    }

    fn checks_remaining(&self, color: Color) -> Option<u8> {
        self.pos
            .remaining_checks()
            .map(|rc| u32::from(*rc.get(shak_color(color))) as u8)
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
        self.pos.castles().has_color(shak_color(color))
    }

    fn as_std(&self) -> Option<&Board> {
        None
    }

    /// Startstellung der VARIANTE (Horde: 36 weisse Bauern, Racing Kings:
    /// beide Lager auf Reihe 1/2) — `position startpos` meint im
    /// Varianten-Backend genau diese.
    fn startpos() -> Self {
        Self::from_pos(P::default())
    }

    fn from_fen(fen: &str) -> Result<Self, String> {
        let parsed =
            Fen::from_ascii(fen.trim().as_bytes()).map_err(|e| format!("Invalid FEN: {}", e))?;
        let pos: P = parsed
            .into_position(CastlingMode::Standard)
            .map_err(|e| format!("Invalid {:?} position: {}", P::KIND, e))?;
        Ok(Self::from_pos(pos))
    }

    fn parse_uci_move(&self, uci: &str) -> Result<ChessMove, String> {
        let parsed = UciMove::from_ascii(uci.trim().as_bytes())
            .map_err(|_| format!("Invalid UCI move: {}", uci))?;
        let m = parsed
            .to_move(&self.pos)
            .map_err(|_| format!("Illegal {:?} move: {}", P::KIND, uci))?;
        Ok(cm_of(&m))
    }
}

/// Cursor ueber die vorab erzeugte Zugliste mit Zielfeld-Maske — identisch
/// zu `GenAtomic`. `yielded` merkt sich pro Zug, ob er schon ausgegeben
/// wurde, damit ein Maskenwechsel (Quiescence: erst Captures, dann Rest)
/// keinen Zug doppelt liefert. 256 Bit reichen: shakmatys MoveList fasst
/// maximal 256 Zuege (Horde hat viele Bauern, aber deutlich weniger Zuege).
pub struct GenShak {
    moves: Arc<Vec<MoveEntry>>,
    yielded: [u64; 4],
    mask: BitBoard,
    cursor: usize,
}

impl Iterator for GenShak {
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

impl MoveGenLike for GenShak {
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

    fn perft<P: ShakVariant>(board: &BoardShak<P>, depth: u32) -> u64 {
        if depth == 0 {
            return 1;
        }
        board
            .legal_gen()
            .map(|mv| perft(&board.make_move_new(mv), depth - 1))
            .sum()
    }

    fn check_perft<P: ShakVariant>(fens: &[&str]) {
        for fen in fens {
            let board = BoardShak::<P>::from_fen(fen)
                .unwrap_or_else(|e| panic!("{:?}: FEN {} ungueltig: {}", P::KIND, fen, e));
            let reference: P = Fen::from_ascii(fen.as_bytes())
                .unwrap()
                .into_position(CastlingMode::Standard)
                .unwrap();
            for depth in 1..=3 {
                assert_eq!(
                    perft(&board, depth),
                    shakmaty::perft(&reference, depth),
                    "{:?} perft({}) fuer {}",
                    P::KIND,
                    depth,
                    fen
                );
            }
        }
        // Startstellung des Backends == Startstellung der Variante.
        let start = BoardShak::<P>::startpos();
        assert_eq!(perft(&start, 2), shakmaty::perft(&P::default(), 2));
    }

    #[test]
    fn antichess_perft_matches_shakmaty() {
        check_perft::<Antichess>(&[
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w - - 0 1",
            // Nach 1.e3 b5 2.Bxb5: Schwarz hat Schlagzwang (c6xb5 / Nc6? nein).
            "rnbqkbnr/p1pppppp/8/1B6/8/4P3/PPPP1PPP/RNBQK1NR b - - 0 2",
        ]);
    }

    #[test]
    fn kingofthehill_perft_matches_shakmaty() {
        check_perft::<KingOfTheHill>(&[
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            "r1bqkbnr/pppp1ppp/2n5/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R w KQkq - 2 3",
        ]);
    }

    #[test]
    fn horde_perft_matches_shakmaty() {
        check_perft::<Horde>(&[
            "rnbqkbnr/pppppppp/8/1PP2PP1/PPPPPPPP/PPPPPPPP/PPPPPPPP/PPPPPPPP w kq - 0 1",
            // Nach 1.b5 e6 2.c5: Bauern auf Reihe 1 duerfen noch zwei Felder.
            "rnbqkbnr/pppp1ppp/4p3/1PP2PP1/P2PPPPP/PPPPPPPP/PPPPPPPP/PPPPPPPP b kq - 0 2",
        ]);
    }

    #[test]
    fn threecheck_perft_matches_shakmaty() {
        check_perft::<ThreeCheck>(&[
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 3+3 0 1",
            "rnbqkb1r/pppp1ppp/5n2/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R w KQkq - 2+3 2 3",
        ]);
    }

    #[test]
    fn racingkings_perft_matches_shakmaty() {
        check_perft::<RacingKings>(&[
            "8/8/8/8/8/8/krbnNBRK/qrbnNBRQ w - - 0 1",
            // Nach 1.Kh3 Ka3: beide Koenige unterwegs.
            "8/8/8/8/8/k6K/1rbnNBR1/qrbnNBRQ w - - 2 2",
        ]);
    }

    #[test]
    fn threecheck_fen_roundtrip_and_remaining_checks() {
        let fen = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 2+1 0 1";
        let board = BoardThreeCheck::from_fen(fen).unwrap();
        assert_eq!(board.checks_remaining(Color::White), Some(2));
        assert_eq!(board.checks_remaining(Color::Black), Some(1));
        assert_eq!(board.to_fen(), fen);
        // Lichess-Startformat mit "3+3" und shakmaty-Alternativformat "+0+0".
        let a = BoardThreeCheck::from_fen(
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 3+3 0 1",
        )
        .unwrap();
        let b = BoardThreeCheck::from_fen(
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - +0+0 0 1",
        )
        .unwrap();
        assert_eq!(a.get_hash(), b.get_hash());
        assert_eq!(a.get_hash(), BoardThreeCheck::startpos().get_hash());
        // Nicht-Three-Check-Backends kennen keine Schachzaehlung.
        assert_eq!(BoardKingOfTheHill::startpos().checks_remaining(Color::White), None);
    }

    #[test]
    fn threecheck_third_check_is_terminal() {
        // Weiss hat noch genau ein Schach zu geben: Qh5xf7+ ist das dritte.
        let board = BoardThreeCheck::from_fen(
            "rnb1kbnr/pppp1ppp/8/4p2Q/4P3/8/PPPP1PPP/RNB1KBNR w KQkq - 1+3 0 3",
        )
        .unwrap();
        let mv = board.parse_uci_move("h5f7").unwrap();
        let after = board.make_move_new(mv);
        assert_eq!(after.checks_remaining(Color::White), Some(0));
        assert!(after.is_variant_loss());
        assert!(!after.is_variant_win());
        assert_eq!(after.legal_gen().count(), 0);
        assert_eq!(after.status(), BoardStatus::Checkmate);
    }

    #[test]
    fn antichess_without_king_parses_and_has_king_false() {
        let board = BoardAntichess::from_fen("8/8/8/3p4/8/8/3P4/8 w - - 0 1").unwrap();
        assert!(!board.has_king(Color::White));
        assert!(!board.has_king(Color::Black));
        assert_eq!(board.king_square(Color::White), Square::A1);
        assert_eq!(board.king_square(Color::Black), Square::A8);
        assert_eq!(board.status(), BoardStatus::Ongoing);
        // Koenig ist normale Figur: Startstellung hat beide, kann aber
        // geschlagen werden.
        assert!(BoardAntichess::startpos().has_king(Color::White));
    }

    #[test]
    fn antichess_no_pieces_or_no_moves_is_win_for_side_to_move() {
        // Weiss hat keine Steine mehr und ist am Zug → Weiss gewinnt.
        let board = BoardAntichess::from_fen("8/8/8/3p4/8/8/8/8 w - - 0 1").unwrap();
        assert!(board.is_variant_win());
        assert!(!board.is_variant_loss());
        assert_eq!(board.legal_gen().count(), 0);
        assert_eq!(board.status(), BoardStatus::Checkmate);
        // Patt: weisser Bauer blockiert, kein Zug → Weiss gewinnt ebenfalls.
        let stuck = BoardAntichess::from_fen("8/8/8/8/3p4/3P4/8/8 w - - 0 1").unwrap();
        assert!(stuck.is_variant_win());
        // Schlagzwang: aus der Zugliste bleibt nur der Schlagzug.
        let must = BoardAntichess::from_fen("8/8/8/4p3/3P4/8/8/8 w - - 0 1").unwrap();
        let moves: Vec<ChessMove> = must.legal_gen().collect();
        assert_eq!(moves.len(), 1);
        assert!(must.is_capture(moves[0]));
    }

    #[test]
    fn antichess_king_promotion_roundtrip() {
        let board = BoardAntichess::from_fen("8/3P4/8/8/8/8/8/7k w - - 0 1").unwrap();
        let mv = board.parse_uci_move("d7d8k").unwrap();
        assert_eq!(mv.get_promotion(), Some(Piece::King));
        assert_eq!(crate::position::move_to_uci(mv), "d7d8k");
        let after = board.make_move_new(mv);
        assert_eq!(after.piece_on(Square::D8), Some(Piece::King));
    }

    #[test]
    fn kingofthehill_center_king_is_terminal() {
        let board =
            BoardKingOfTheHill::from_fen("4k3/8/8/8/8/3K4/8/8 w - - 0 1").unwrap();
        let mv = board.parse_uci_move("d3d4").unwrap();
        let after = board.make_move_new(mv);
        assert!(after.is_variant_loss());
        assert_eq!(after.legal_gen().count(), 0);
        assert_eq!(after.status(), BoardStatus::Checkmate);
        // Standard-Rochadenotation.
        let castle = BoardKingOfTheHill::from_fen("4k3/8/8/8/8/8/8/R3K2R w KQ - 0 1").unwrap();
        let after = castle.make_move_new(castle.parse_uci_move("e1g1").unwrap());
        assert_eq!(after.king_square(Color::White), Square::G1);
    }

    #[test]
    fn horde_white_has_no_king_and_loses_without_pieces() {
        let start = BoardHorde::startpos();
        assert!(!start.has_king(Color::White));
        assert!(start.has_king(Color::Black));
        assert_eq!(start.pieces(Piece::Pawn).popcnt(), 44);
        assert_eq!(start.status(), BoardStatus::Ongoing);
        // Bauern auf Reihe 1 duerfen zwei Felder ziehen (a1a3 im Startzug
        // nicht, da a2 besetzt — aber nach Freizug schon). Hier: Doppel-
        // schritt eines Reihe-1-Bauern in einer Testkonstruktion.
        let dbl = BoardHorde::from_fen("4k3/8/8/8/8/8/8/3P4 w - - 0 1").unwrap();
        assert!(dbl.parse_uci_move("d1d3").is_ok());
        // Letzter weisser Bauer wird geschlagen → Weiss am Zug ohne Steine
        // → Varianten-Niederlage fuer Weiss.
        let board = BoardHorde::from_fen("8/8/8/8/3k4/8/3P4/8 b - - 0 1").unwrap();
        let mv = board.parse_uci_move("d4d3").unwrap();
        let mid = board.make_move_new(mv);
        let cap = mid.parse_uci_move("d2d3").unwrap_err();
        assert!(cap.contains("Illegal"));
        let end = BoardHorde::from_fen("8/8/3k4/8/8/8/8/8 w - - 0 1").unwrap();
        assert!(end.is_variant_loss());
        assert!(!end.has_king(Color::White));
        assert_eq!(end.legal_gen().count(), 0);
        assert_eq!(end.status(), BoardStatus::Checkmate);
    }

    #[test]
    fn racingkings_outcomes() {
        // Weiss erreicht Reihe 8, Schwarz kann nicht nachziehen → nach dem
        // schwarzen Zug (Zugliste leer, da Spielende) ist Weiss am Zug und
        // hat GEWONNEN.
        let board = BoardRacingKings::from_fen("1K6/8/8/8/8/8/8/k7 b - - 0 1").unwrap();
        assert!(board.is_variant_loss());
        assert_eq!(board.legal_gen().count(), 0);
        // Weiss auf Reihe 8, Schwarz kann direkt nachziehen → Remis.
        let race = BoardRacingKings::from_fen("K7/7k/8/8/8/8/8/8 b - - 0 1").unwrap();
        assert!(!race.is_variant_loss());
        let mv = race.parse_uci_move("h7h8").unwrap();
        let after = race.make_move_new(mv);
        assert!(after.is_variant_draw());
        assert_eq!(after.status(), BoardStatus::Stalemate);
        // Weiss am Zug, nur weisser Koenig auf Reihe 8 → Sieg der Seite am Zug.
        let win = BoardRacingKings::from_fen("K7/8/8/8/8/8/8/7k w - - 0 1").unwrap();
        assert!(win.is_variant_win());
        assert_eq!(win.status(), BoardStatus::Checkmate);
        // Kein Schachgebot erlaubt: Rg1-a1+ und Rg1-g2+ (Reihe des Koenigs)
        // sind keine legalen Zuege, Rg1-g3 schon.
        let quiet = BoardRacingKings::from_fen("8/8/8/8/8/8/k7/6RK w - - 0 1").unwrap();
        assert!(quiet.parse_uci_move("g1a1").is_err());
        assert!(quiet.parse_uci_move("g1g2").is_err());
        assert!(quiet.parse_uci_move("g1g3").is_ok());
        assert!(!BoardRacingKings::startpos().has_castle_rights());
    }
}
