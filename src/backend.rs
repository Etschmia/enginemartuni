//! Backend-Abstraktion fuer die Brett-Repraesentation.
//!
//! Hintergrund (Chess960, 21.07.2026): die jordanbray-`chess`-Crate kann kein
//! Chess960 (keine Shredder-FEN-Rochaderechte, Zuggenerierung kennt nur die
//! Standard-Rochade). Statt die Crate zu ERSETZEN (Tobias-Entscheid: behalten!)
//! wird sie hinter dieses Trait gestellt: Suche, Eval und Endspiel-Erkennung
//! arbeiten generisch gegen `EngineBoard`, und es gibt zwei Implementierungen:
//!
//!   - `chess::Board`      — Standard-Schach, unveraendert der Live-Pfad.
//!     Alle Methoden reichen 1:1 an die Crate durch (duenne Huelle, bit-exakt).
//!   - `Board960`          — Chess960 auf shakmaty-Basis (src/board960.rs).
//!
//! Die "gemeinsame Sprache" bleiben die kleinen `chess`-Werttypen (`BitBoard`,
//! `Square`, `Piece`, `Color`, `ChessMove`) — sie sind reine Daten ohne Bezug
//! zur Brett-Implementierung. Damit bleiben Eval-Terme, PSTs, SEE und die
//! statischen Angriffstabellen (`get_bishop_moves` & Co.) fuer beide Backends
//! identisch nutzbar.
//!
//! Konvention fuer Rochadezuege im 960-Backend: `ChessMove` codiert sie als
//! "Koenig schlaegt eigenen Turm" (Quellfeld = Koenig, Zielfeld = Turm) — das
//! ist exakt die UCI_Chess960-Notation (z. B. `e1h1`), `move_to_uci` druckt
//! sie damit ohne Sonderfall korrekt. Deshalb ist `is_capture` eine Methode
//! des Backends: ein "Koenig auf eigenen Turm"-Zug saehe fuer die naive
//! Feld-belegt-Pruefung wie ein Schlagzug aus.

use chess::{BitBoard, Board, BoardStatus, ChessMove, Color, MoveGen, Piece, Square};

/// Zuggenerator-Schnittstelle im Stil von `chess::MoveGen`: Iterator ueber
/// legale Zuege mit nachtraeglich setzbarer Zielfeld-Maske. Bereits
/// ausgegebene Zuege werden nach einem Maskenwechsel nicht erneut geliefert
/// (Quiescence-Muster: erst Captures per Maske, dann Rest per `!EMPTY`).
pub trait MoveGenLike: Iterator<Item = ChessMove> {
    fn set_iterator_mask(&mut self, mask: BitBoard);
    /// Anzahl der noch nicht ausgegebenen Zuege (maskenunabhaengig).
    fn count_remaining(&self) -> usize;
}

impl MoveGenLike for MoveGen {
    fn set_iterator_mask(&mut self, mask: BitBoard) {
        // Inherent-Methode der Crate (gewinnt gegen die Trait-Methode).
        MoveGen::set_iterator_mask(self, mask);
    }

    fn count_remaining(&self) -> usize {
        self.len()
    }
}

/// Brett-Schnittstelle der Suche/Eval. Die Methodennamen und Signaturen
/// spiegeln bewusst `chess::Board`, damit die bestehenden Aufrufstellen
/// unveraendert bleiben (nur die Funktionssignaturen werden generisch).
pub trait EngineBoard: Clone + Send + Sync + 'static {
    type Gen: MoveGenLike;

    fn pieces(&self, piece: Piece) -> &BitBoard;
    fn color_combined(&self, color: Color) -> &BitBoard;
    fn combined(&self) -> &BitBoard;
    fn side_to_move(&self) -> Color;
    fn king_square(&self, color: Color) -> Square;
    fn checkers(&self) -> &BitBoard;
    fn piece_on(&self, square: Square) -> Option<Piece>;
    /// Feld des Bauern, der en passant geschlagen werden kann (chess-Crate-
    /// Semantik: das BAUERN-Feld, z. B. e5 — nicht das Zielfeld e6).
    fn en_passant(&self) -> Option<Square>;
    fn get_hash(&self) -> u64;
    fn status(&self) -> BoardStatus;
    fn make_move_new(&self, mv: ChessMove) -> Self;
    fn null_move(&self) -> Option<Self>;
    fn legal_gen(&self) -> Self::Gen;

    /// `true` fuer Varianten, deren Taktik/Endspiele nicht den orthodoxen
    /// Schachregeln folgen. Damit koennen Suche, SEE und Tablebases ihre
    /// Standard-Annahmen gezielt abschalten, ohne den Standard-/960-Pfad zu
    /// veraendern.
    fn uses_standard_rules(&self) -> bool {
        true
    }

    /// Varianten-Niederlage der Seite am Zug (z. B. explodierter Koenig in
    /// Atomic). Orthodoxes Matt wird weiterhin ueber `checkers + 0 Zuege`
    /// erkannt.
    fn is_variant_loss(&self) -> bool {
        false
    }

    /// Schlagzug? (inkl. en passant; Rochade zaehlt NICHT als Schlag,
    /// obwohl sie im 960-Backend als "Koenig x eigener Turm" codiert ist).
    fn is_capture(&self, mv: ChessMove) -> bool;

    /// Hat irgendeine Seite noch Rochaderechte? (Syzygy-Gate)
    fn has_castle_rights(&self) -> bool;

    /// Hat DIESE Seite noch Rochaderechte? Fuer den Eval-Rochade-Anreiz
    /// (960-Lookback 26.07.2026): solange die Rechte bestehen, wurde noch
    /// nicht rochiert → kleiner MG-Malus als Tempo-Nudge. Fuehrt die Seite
    /// die Rochade aus, verschwinden die Rechte und damit der Malus.
    /// (Verliert sie die Rechte durch Koenigs-/Turmzuege, faengt stattdessen
    /// die Zentrums-/Flanken-Strafe in `pawn_shield_score` den Koenig ab.)
    fn has_castle_rights_for(&self, color: Color) -> bool;

    /// Standard-Sicht auf die Stellung, falls das Backend eine hat.
    /// Wird fuer die Polyglot-Buchsuche genutzt (Buecher sind Standard-
    /// Schach); das 960-Backend liefert `None` → kein Buchzugriff.
    fn as_std(&self) -> Option<&Board>;

    // --- Konstruktion / UCI-Anbindung (Position-Tracking) -----------------

    fn startpos() -> Self;
    fn from_fen(fen: &str) -> Result<Self, String>;
    /// UCI-Zugtext in einen legalen Zug uebersetzen (Backend-Notation:
    /// Standard e1g1 bzw. 960 "Koenig x Turm" e1h1).
    fn parse_uci_move(&self, uci: &str) -> Result<ChessMove, String>;
}

impl EngineBoard for Board {
    type Gen = MoveGen;

    #[inline]
    fn pieces(&self, piece: Piece) -> &BitBoard {
        Board::pieces(self, piece)
    }

    #[inline]
    fn color_combined(&self, color: Color) -> &BitBoard {
        Board::color_combined(self, color)
    }

    #[inline]
    fn combined(&self) -> &BitBoard {
        Board::combined(self)
    }

    #[inline]
    fn side_to_move(&self) -> Color {
        Board::side_to_move(self)
    }

    #[inline]
    fn king_square(&self, color: Color) -> Square {
        Board::king_square(self, color)
    }

    #[inline]
    fn checkers(&self) -> &BitBoard {
        Board::checkers(self)
    }

    #[inline]
    fn piece_on(&self, square: Square) -> Option<Piece> {
        Board::piece_on(self, square)
    }

    #[inline]
    fn en_passant(&self) -> Option<Square> {
        Board::en_passant(*self)
    }

    #[inline]
    fn get_hash(&self) -> u64 {
        Board::get_hash(self)
    }

    #[inline]
    fn status(&self) -> BoardStatus {
        Board::status(self)
    }

    #[inline]
    fn make_move_new(&self, mv: ChessMove) -> Self {
        Board::make_move_new(self, mv)
    }

    #[inline]
    fn null_move(&self) -> Option<Self> {
        Board::null_move(self)
    }

    #[inline]
    fn legal_gen(&self) -> MoveGen {
        MoveGen::new_legal(self)
    }

    #[inline]
    fn is_capture(&self, mv: ChessMove) -> bool {
        if self.piece_on(mv.get_dest()).is_some() {
            return true;
        }
        // en passant: Bauer wechselt die Linie auf ein leeres Feld
        self.piece_on(mv.get_source()) == Some(Piece::Pawn)
            && mv.get_source().get_file() != mv.get_dest().get_file()
            && self.piece_on(mv.get_dest()).is_none()
    }

    #[inline]
    fn has_castle_rights(&self) -> bool {
        self.castle_rights(Color::White) != chess::CastleRights::NoRights
            || self.castle_rights(Color::Black) != chess::CastleRights::NoRights
    }

    #[inline]
    fn has_castle_rights_for(&self, color: Color) -> bool {
        self.castle_rights(color) != chess::CastleRights::NoRights
    }

    #[inline]
    fn as_std(&self) -> Option<&Board> {
        Some(self)
    }

    fn startpos() -> Self {
        Board::default()
    }

    fn from_fen(fen: &str) -> Result<Self, String> {
        use std::str::FromStr;
        Board::from_str(fen).map_err(|e| format!("Invalid FEN: {}", e))
    }

    fn parse_uci_move(&self, uci: &str) -> Result<ChessMove, String> {
        use std::str::FromStr;
        let uci = uci.trim();
        if uci.len() < 4 || uci.len() > 5 {
            return Err(format!("Invalid UCI move: {}", uci));
        }

        let from = Square::from_str(&uci[0..2])
            .map_err(|_| format!("Invalid source square: {}", &uci[0..2]))?;
        let to = Square::from_str(&uci[2..4])
            .map_err(|_| format!("Invalid target square: {}", &uci[2..4]))?;

        let promotion = if uci.len() == 5 {
            match uci.as_bytes()[4] {
                b'q' => Some(Piece::Queen),
                b'r' => Some(Piece::Rook),
                b'b' => Some(Piece::Bishop),
                b'n' => Some(Piece::Knight),
                c => return Err(format!("Invalid promotion piece: {}", c as char)),
            }
        } else {
            None
        };

        let candidate = ChessMove::new(from, to, promotion);
        for m in MoveGen::new_legal(self) {
            if m == candidate {
                return Ok(candidate);
            }
        }

        Err(format!("Illegal move: {}", uci))
    }
}
