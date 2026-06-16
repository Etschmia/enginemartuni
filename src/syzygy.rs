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
    BitBoard, Board, CastleRights, ChessMove, Color, File, MoveGen, Piece, Rank, Square, EMPTY,
};
use pyrrhic_rs::{
    Color as TbColor, DtzProbeValue, EngineAdapter, Piece as TbPiece, TableBases, WdlProbeResult,
};
use std::fs;
use std::path::Path;

/// Magic-Bytes am Dateianfang gültiger Syzygy-Tabellen (de Mans Format),
/// empirisch gegen den 3-4-5-Satz verifiziert. WDL = `.rtbw`, DTZ = `.rtbz`.
/// Dienen dem Integritäts-Guard (siehe `verify_tables` / `load`).
const WDL_MAGIC: [u8; 4] = [0x71, 0xe8, 0x23, 0x5d];
const DTZ_MAGIC: [u8; 4] = [0xd7, 0x66, 0x0c, 0xa5];

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
        // Integritäts-Guard VOR dem mmap: eine defekte/truncierte Tabelle würde
        // sonst beim Probing einen SIGBUS auslösen (Engine-Crash, nicht
        // abfangbar). Lieber Tablebases ganz abschalten als crashen.
        if let Err(e) = verify_tables(path) {
            println!("info string Syzygy: deaktiviert — {}", e);
            return None;
        }
        match TableBases::<ChessAdapter>::new(path) {
            Ok(tb) => {
                let pyrrhic_max = tb.max_pieces();
                if pyrrhic_max == 0 {
                    // Pfad existiert, aber keine ladbaren Tabellen gefunden.
                    None
                } else {
                    let detected_max = detect_max_pieces(path);
                    let max_pieces = detected_max
                        .map(|detected| pyrrhic_max.min(detected))
                        .unwrap_or(pyrrhic_max);
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
        if !self.probeable(board) {
            return None;
        }
        let (bb, turn) = position_bitboards(board);
        // ep = 0 (en passant in `probeable` ausgeschlossen). WDL ignoriert
        // rule50; die 50-Züge-Grenze prüft die Suche selbst (halfmove >= 100).
        match self.tb.probe_wdl(
            bb[0], bb[1], bb[2], bb[3], bb[4], bb[5], bb[6], bb[7], 0, turn,
        ) {
            Ok(WdlProbeResult::Win) => Some(TB_WIN - ply),
            Ok(WdlProbeResult::Loss) => Some(-(TB_WIN - ply)),
            Ok(WdlProbeResult::Draw)
            | Ok(WdlProbeResult::CursedWin)
            | Ok(WdlProbeResult::BlessedLoss) => Some(0),
            Err(_) => None,
        }
    }

    /// DTZ-**Wurzel**-Probe: liefert den 50-Züge-sicher konvertierenden Zug
    /// (DTZ-optimal) plus einen Score (Win/Draw/Loss aus Wurzelsicht), oder
    /// `None` wenn nicht probebar bzw. kein eindeutiger Zug (→ normale Suche).
    ///
    /// `root` trägt (als `DtzResult`) den von Fathom empfohlenen Bestzug mit
    /// `from`/`to`/`promotion`. Wir spielen ihn direkt — in einem ≤N-Steine-
    /// Endspiel ist der DTZ-optimale Zug der beste Zug; die Suche kann ihn nicht
    /// schlagen. Das ersetzt die fehleranfällige Konversions-Heuristik und
    /// vermeidet die 50-Züge-Remis-Klasse (Damen-/Turm-Geschiebe ohne Fortschritt).
    ///
    /// Hinweis: „DTZ-optimal" = gewinnt unter Beachtung der 50-Züge-Regel, nicht
    /// zwingend „mattet in den wenigsten Zügen" — gelegentlich wirkt der Zug
    /// umständlich, ist aber beweisbar korrekt.
    ///
    /// Der Legalitätscheck am Ende ist ein Sicherheitsnetz gegen eine
    /// Fehlabbildung (Index/Promotion): findet sich der Zug nicht unter den
    /// legalen Wurzelzügen, geben wir `None` zurück und suchen normal weiter.
    pub fn probe_root_move(&self, board: &Board, halfmove: u8) -> Option<(ChessMove, i32)> {
        if !self.probeable(board) {
            return None;
        }
        let (bb, turn) = position_bitboards(board);
        let res = self
            .tb
            .probe_root(
                bb[0], bb[1], bb[2], bb[3], bb[4], bb[5], bb[6], bb[7],
                halfmove as u32,
                0,
                turn,
            )
            .ok()?;

        let r = match res.root {
            DtzProbeValue::DtzResult(r) => r,
            // Stalemate/Checkmate/Failed → an einer Ongoing-Wurzel praktisch nie;
            // defensiv auf normale Suche zurückfallen.
            _ => return None,
        };

        let from = sq_from_index(r.from_square as u64);
        let to = sq_from_index(r.to_square as u64);
        let promo = match r.promotion {
            TbPiece::Queen => Some(Piece::Queen),
            TbPiece::Rook => Some(Piece::Rook),
            TbPiece::Bishop => Some(Piece::Bishop),
            TbPiece::Knight => Some(Piece::Knight),
            // Pawn/King = Sentinel „keine Promotion".
            _ => None,
        };
        let mv = ChessMove::new(from, to, promo);

        if !MoveGen::new_legal(board).any(|m| m == mv) {
            return None;
        }

        let score = match r.wdl {
            WdlProbeResult::Win => TB_WIN,
            WdlProbeResult::Loss => -TB_WIN,
            _ => 0,
        };
        Some((mv, score))
    }

    /// Gemeinsame Probe-Vorbedingungen (Syzygy): Steinzahl ≤ `max_pieces`, keine
    /// Rochaderechte, kein en passant (v1). Genutzt von WDL- und DTZ-Probe.
    fn probeable(&self, board: &Board) -> bool {
        board.combined().popcnt() <= self.max_pieces
            && board.castle_rights(Color::White) == CastleRights::NoRights
            && board.castle_rights(Color::Black) == CastleRights::NoRights
            && board.en_passant().is_none()
    }
}

/// Extrahiert die acht u64-Bitboards (Reihenfolge wie von Pyrrhic erwartet:
/// white, black, kings, queens, rooks, bishops, knights, pawns) plus
/// Seite-am-Zug (`true` = Weiß) aus dem `chess::Board`.
fn position_bitboards(board: &Board) -> ([u64; 8], bool) {
    (
        [
            board.color_combined(Color::White).0,
            board.color_combined(Color::Black).0,
            board.pieces(Piece::King).0,
            board.pieces(Piece::Queen).0,
            board.pieces(Piece::Rook).0,
            board.pieces(Piece::Bishop).0,
            board.pieces(Piece::Knight).0,
            board.pieces(Piece::Pawn).0,
        ],
        board.side_to_move() == Color::White,
    )
}

/// Integritäts-Guard: prüft alle `.rtbw`/`.rtbz` unter den (Doppelpunkt-
/// getrennten) Verzeichnissen auf gültige Magic-Bytes.
///
/// **Warum:** Eine truncierte oder durch einen abgebrochenen Download
/// verfälschte Tabelle würde beim mmap-gestützten Probing über das Dateiende
/// hinaus lesen → **SIGBUS** → harter Engine-Crash. SIGBUS ist ein
/// Hardware-Signal und **nicht** von `catch_unwind` fangbar, daher muss die
/// Prüfung VOR dem mmap (`TableBases::new`) passieren.
///
/// **Best-effort:** Magic + Lesbarkeit der ersten 4 Bytes. Fängt die
/// realistischen Defekte (0-Byte, HTML-Fehlerseite, falscher Inhalt). Eine exakt
/// an einer Seitengrenze mit gültigem Header abgeschnittene Datei bleibt
/// unentdeckt — dagegen hilft nur die einmalige Magic-/Checksummen-Prüfung beim
/// Download. Gibt `Err` mit den defekten Dateinamen zurück.
fn verify_tables(path: &str) -> Result<(), String> {
    let mut bad: Vec<String> = Vec::new();
    for dir in path.split(':').map(str::trim).filter(|s| !s.is_empty()) {
        let entries = match fs::read_dir(dir) {
            Ok(e) => e,
            // Nicht lesbares Verzeichnis: kein Defekt-Befund hier — pyrrhic
            // meldet einen leeren/fehlenden Pfad selbst (max_pieces == 0).
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let p = entry.path();
            let want = match p.extension().and_then(|e| e.to_str()) {
                Some("rtbw") => WDL_MAGIC,
                Some("rtbz") => DTZ_MAGIC,
                _ => continue,
            };
            let ok = read_magic(&p).map(|m| m == want).unwrap_or(false);
            if !ok {
                bad.push(
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("?")
                        .to_string(),
                );
            }
        }
    }
    if bad.is_empty() {
        Ok(())
    } else {
        bad.sort();
        let shown = bad.iter().take(8).cloned().collect::<Vec<_>>().join(", ");
        Err(format!(
            "{} defekte/truncierte Tabellendatei(en): {}",
            bad.len(),
            shown
        ))
    }
}

fn read_magic(p: &Path) -> std::io::Result<[u8; 4]> {
    use std::io::Read;
    let mut f = fs::File::open(p)?;
    let mut buf = [0u8; 4];
    f.read_exact(&mut buf)?;
    Ok(buf)
}

fn detect_max_pieces(path: &str) -> Option<u32> {
    let mut max_pieces = 0;
    for dir in path.split(':').map(str::trim).filter(|s| !s.is_empty()) {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            match p.extension().and_then(|e| e.to_str()) {
                Some("rtbw") | Some("rtbz") => {}
                _ => continue,
            }
            let Some(stem) = p.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if let Some(count) = table_name_piece_count(stem) {
                max_pieces = max_pieces.max(count);
            }
        }
    }
    (max_pieces > 0).then_some(max_pieces)
}

fn table_name_piece_count(stem: &str) -> Option<u32> {
    let mut seen_separator = false;
    let mut count = 0;
    for ch in stem.chars() {
        if ch == 'v' {
            seen_separator = true;
            continue;
        }
        if matches!(ch, 'K' | 'Q' | 'R' | 'B' | 'N' | 'P') {
            count += 1;
        } else {
            return None;
        }
    }
    (seen_separator && count >= 2).then_some(count)
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

    /// Integritäts-Guard: gültige Magic → Ok; eine Datei mit falscher Magic
    /// (truncierter/abgebrochener Download) → Err, der die Datei benennt. Genau
    /// das verhindert den SIGBUS beim mmap-Probing.
    #[test]
    fn verify_tables_flags_bad_magic() {
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("martuni_syz_{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let dirstr = dir.to_str().unwrap();

        // Nur gültige Magic-Bytes → Ok.
        fs::File::create(dir.join("KRvK.rtbw"))
            .unwrap()
            .write_all(&WDL_MAGIC)
            .unwrap();
        assert!(verify_tables(dirstr).is_ok());

        // Eine Datei mit falscher Magic → Err, die sie benennt.
        fs::File::create(dir.join("KQvK.rtbz"))
            .unwrap()
            .write_all(b"XXXX")
            .unwrap();
        let res = verify_tables(dirstr);
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("KQvK.rtbz"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn table_name_piece_count_counts_both_sides() {
        assert_eq!(table_name_piece_count("KQvK"), Some(3));
        assert_eq!(table_name_piece_count("KPPvKPP"), Some(6));
        assert_eq!(table_name_piece_count("KQK"), None);
        assert_eq!(table_name_piece_count("KXvK"), None);
    }

    #[test]
    fn detect_max_pieces_uses_table_filenames() {
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!(
            "martuni_syz_detect_{}",
            std::process::id()
        ));
        let _ = fs::create_dir_all(&dir);
        fs::File::create(dir.join("KQvK.rtbw"))
            .unwrap()
            .write_all(&WDL_MAGIC)
            .unwrap();
        fs::File::create(dir.join("KPPvKPP.rtbz"))
            .unwrap()
            .write_all(&DTZ_MAGIC)
            .unwrap();

        assert_eq!(detect_max_pieces(dir.to_str().unwrap()), Some(6));

        let _ = fs::remove_dir_all(&dir);
    }
}
