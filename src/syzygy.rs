//! Syzygy-Tablebase-Anbindung (3-4-5-Steiner) über `pyrrhic-rs`.
//!
//! `pyrrhic-rs` ist ein sicherer Rust-Wrapper um **Pyrrhic** (gepflegter Fork
//! von Ronald de Mans C-Referenz **Fathom**) — derselbe Probing-Code, den auch
//! Stockfish-Derivate einbinden. Wir nutzen nur das **WDL-Probing**
//! (Win/Draw/Loss) in der Suche; das DTZ-Wurzel-Probing (50-Züge-sichere
//! Konversion) folgt in einer späteren Phase.
//!
//! ## Wie es funktioniert
//! Die C-Bibliothek braucht für ihre interne Zuggenerierung Angriffs-Bitboards.
//! Statt eines eigenen Board-Modells reicht sie über das `EngineAdapter`-Trait
//! sechs *statische* Callback-Funktionen an — die delegieren wir 1:1 an die
//! Angriffstabellen der `chess`-Crate (`get_*_moves` / `get_pawn_attacks`).
//! So bleibt unsere Board-Repräsentation (jordanbray `chess`) unangetastet, und
//! wir vermeiden den FEN-Roundtrip, den `shakmaty-syzygy` pro Probe bräuchte.
//!
//! ## Sicherheit / Verhalten
//! - **Default-off:** Ohne gesetzten `SyzygyPath` wird `Syzygy` nie geladen
//!   (`None`); jeder Probe-Pfad in der Suche entfällt → bit-exakt zur Engine
//!   ohne Tablebases. Erst ein nicht-leerer Pfad aktiviert das Probing.
//! - **Konservative Gates** (siehe `probe_wdl_score`): Syzygy setzt voraus,
//!   dass keine Rochaderechte bestehen. En-passant-Stellungen lassen wir in
//!   dieser Version aus (die `chess`-EP-Semantik weicht vom Fathom-Zielfeld ab)
//!   — Auslassen ist immer korrekt, es fällt nur auf die normale Suche zurück.

use chess::{
    get_bishop_moves, get_king_moves, get_knight_moves, get_pawn_attacks, get_rook_moves,
    BitBoard, Board, CastleRights, Color, File, Piece, Rank, Square, EMPTY,
};
use pyrrhic_rs::{Color as TbColor, EngineAdapter, TableBases, WdlProbeResult};

/// TB-Gewinn-Basiswert. Bewusst **über** jeder normalen Bewertung, aber
/// **unter** `MATE_THRESHOLD` (99_000) der Suche angesiedelt: so gehen echte
/// Matts einem Tablebase-Gewinn vor, und die knotenrelative TT-Mate-Normierung
/// (`mate_score_to_tt`/`mate_score_from_tt`) springt für TB-Scores nicht an.
/// `-ply` bevorzugt schnellere Gewinne (näher an der Wurzel = höherer Score).
pub const TB_WIN: i32 = 90_000;

/// Adapter, der der C-Bibliothek die Angriffs-Bitboards der `chess`-Crate
/// bereitstellt. Zustandslos (Unit-Struct); `EngineAdapter` verlangt `Clone`.
#[derive(Clone)]
struct ChessAdapter;

impl EngineAdapter for ChessAdapter {
    fn pawn_attacks(color: TbColor, sq: u64) -> u64 {
        let c = if color == TbColor::White {
            Color::White
        } else {
            Color::Black
        };
        // jordanbrays get_pawn_attacks maskiert das Ergebnis mit `blockers`;
        // mit `!EMPTY` (alle Felder belegt) erhalten wir den vollen
        // Angriffssatz, den Pyrrhic erwartet (belegungs-unabhängig).
        get_pawn_attacks(sq_from_index(sq), c, !EMPTY).0
    }

    fn knight_attacks(sq: u64) -> u64 {
        get_knight_moves(sq_from_index(sq)).0
    }

    fn king_attacks(sq: u64) -> u64 {
        get_king_moves(sq_from_index(sq)).0
    }

    fn bishop_attacks(sq: u64, occ: u64) -> u64 {
        get_bishop_moves(sq_from_index(sq), BitBoard(occ)).0
    }

    fn rook_attacks(sq: u64, occ: u64) -> u64 {
        get_rook_moves(sq_from_index(sq), BitBoard(occ)).0
    }

    fn queen_attacks(sq: u64, occ: u64) -> u64 {
        (get_bishop_moves(sq_from_index(sq), BitBoard(occ))
            | get_rook_moves(sq_from_index(sq), BitBoard(occ)))
        .0
    }
}

/// Pyrrhic übergibt das Feld als 0–63-Index (a1=0, b1=1, …, h8=63), nicht als
/// Bitboard. Umrechnung in `chess::Square` über Rang (`>> 3`) und Linie (`& 7`).
#[inline]
fn sq_from_index(sq: u64) -> Square {
    Square::make_square(
        Rank::from_index(((sq >> 3) & 7) as usize),
        File::from_index((sq & 7) as usize),
    )
}

/// Geladene Tablebase-Instanz. Hält den `pyrrhic-rs`-Handle (mmapt die Dateien
/// einmalig) und die größte unterstützte Steinzahl.
pub struct Syzygy {
    tb: TableBases<ChessAdapter>,
    max_pieces: u32,
}

impl Syzygy {
    /// Lädt die Tabellen vom (Doppelpunkt-getrennten) Pfad.
    /// `None` bei leerem Pfad, Ladefehler oder wenn keine Tabellen gefunden
    /// wurden — der Aufrufer fällt dann lautlos auf die normale Suche zurück.
    pub fn load(path: &str) -> Option<Syzygy> {
        if path.trim().is_empty() {
            return None;
        }
        match TableBases::<ChessAdapter>::new(path) {
            Ok(tb) => {
                let max_pieces = tb.max_pieces();
                if max_pieces == 0 {
                    // Pfad existiert, aber keine ladbaren Tabellen gefunden.
                    None
                } else {
                    Some(Syzygy { tb, max_pieces })
                }
            }
            Err(_) => None,
        }
    }

    /// Größte unterstützte Steinzahl (z. B. 5 für den 3-4-5-Satz).
    pub fn max_pieces(&self) -> u32 {
        self.max_pieces
    }

    /// WDL-Probe für einen **inneren** Suchknoten (`ply > 0`).
    ///
    /// Liefert einen knotenrelativen Score:
    /// - `Win`  → `TB_WIN - ply`
    /// - `Loss` → `-(TB_WIN - ply)`
    /// - `Draw` / `CursedWin` / `BlessedLoss` → `0`
    ///   (Cursed/Blessed sind durch die 50-Züge-Regel zum Remis „entschärfte"
    ///   Gewinne/Verluste → konservativ als Remis gewertet.)
    ///
    /// Gibt `None` zurück, wenn nicht probebar: zu viele Steine, bestehende
    /// Rochaderechte, en-passant-Stellung oder fehlende Tabelle für das
    /// Material. In allen `None`-Fällen sucht der Aufrufer normal weiter.
    pub fn probe_wdl_score(&self, board: &Board, ply: i32) -> Option<i32> {
        // --- Gates (Syzygy-Vorbedingungen) ---
        if board.combined().popcnt() > self.max_pieces {
            return None;
        }
        if board.castle_rights(Color::White) != CastleRights::NoRights
            || board.castle_rights(Color::Black) != CastleRights::NoRights
        {
            return None;
        }
        if board.en_passant().is_some() {
            return None;
        }

        // --- Bitboards extrahieren (jeweils als u64) ---
        let white = board.color_combined(Color::White).0;
        let black = board.color_combined(Color::Black).0;
        let kings = board.pieces(Piece::King).0;
        let queens = board.pieces(Piece::Queen).0;
        let rooks = board.pieces(Piece::Rook).0;
        let bishops = board.pieces(Piece::Bishop).0;
        let knights = board.pieces(Piece::Knight).0;
        let pawns = board.pieces(Piece::Pawn).0;
        let turn = board.side_to_move() == Color::White;

        // ep = 0 (en passant oben ausgeschlossen). WDL ignoriert rule50;
        // die 50-Züge-Grenze prüft die Suche selbst (halfmove >= 100).
        match self
            .tb
            .probe_wdl(white, black, kings, queens, rooks, bishops, knights, pawns, 0, turn)
        {
            Ok(WdlProbeResult::Win) => Some(TB_WIN - ply),
            Ok(WdlProbeResult::Loss) => Some(-(TB_WIN - ply)),
            Ok(WdlProbeResult::Draw)
            | Ok(WdlProbeResult::CursedWin)
            | Ok(WdlProbeResult::BlessedLoss) => Some(0),
            Err(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    /// Index→Square-Mapping muss zur a1=0-Konvention passen.
    #[test]
    fn sq_from_index_corners() {
        assert_eq!(sq_from_index(0), Square::make_square(Rank::First, File::A));
        assert_eq!(sq_from_index(7), Square::make_square(Rank::First, File::H));
        assert_eq!(sq_from_index(63), Square::make_square(Rank::Eighth, File::H));
    }

    /// Adapter liefert dieselben Angriffe wie die chess-Crate (Stichproben).
    #[test]
    fn adapter_matches_chess_attacks() {
        let e4 = Square::make_square(Rank::Fourth, File::E);
        assert_eq!(
            ChessAdapter::knight_attacks(e4.to_index() as u64),
            get_knight_moves(e4).0
        );
        assert_eq!(
            ChessAdapter::king_attacks(e4.to_index() as u64),
            get_king_moves(e4).0
        );
        let occ = 0u64;
        assert_eq!(
            ChessAdapter::rook_attacks(e4.to_index() as u64, occ),
            get_rook_moves(e4, BitBoard(occ)).0
        );
    }

    /// Gates: zu viele Steine / Rochaderechte / en passant → kein Probe-Versuch
    /// (None), unabhängig davon ob Tabellen geladen sind. Wir bauen eine
    /// Syzygy-Instanz ohne echte Tabellen nicht — stattdessen prüfen wir die
    /// Gate-Bedingungen direkt an `Board`-Stellungen, damit der Test auch ohne
    /// heruntergeladene Tabellen läuft.
    #[test]
    fn gates_reject_castling_and_too_many_pieces() {
        // Startstellung: 32 Steine + volle Rochaderechte → beide Gates greifen.
        let start = Board::default();
        assert!(start.combined().popcnt() > 5);
        assert!(start.castle_rights(Color::White) != CastleRights::NoRights);

        // Reine KRvK-Stellung: 3 Steine, keine Rochaderechte, kein ep.
        let krk = Board::from_str("8/8/8/4k3/8/8/4K3/4R3 w - - 0 1").unwrap();
        assert_eq!(krk.combined().popcnt(), 3);
        assert_eq!(krk.castle_rights(Color::White), CastleRights::NoRights);
        assert_eq!(krk.castle_rights(Color::Black), CastleRights::NoRights);
        assert!(krk.en_passant().is_none());
    }
}
