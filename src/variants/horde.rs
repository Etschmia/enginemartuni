//! Horde — varianten-spezifische Bewertung.
//!
//! Regeln: Weiss (die "Horde") hat 36 Bauern und keinen Koenig
//! (`has_king(White)` ist `false`, `king_square(White)` ist nur ein
//! Platzhalter!), Schwarz spielt mit dem normalen Lager. Bauern auf der
//! ersten Reihe duerfen zwei Felder ziehen (ohne en-passant-Folge). Weiss
//! gewinnt durch MATT, Schwarz durch SCHLAGEN ALLER weissen Steine. Beides
//! erkennt der Adapter (`board_shak.rs`) regelkonform ueber shakmaty; die
//! Suche sieht das Spielende ueber die leere Zugliste (`terminal_score`).
//!
//! Was die generische Bewertung (`base`) fuer Horde schon richtig macht:
//! Material (Bauer 100 / Figur 300 / Turm 500 / Dame 900 — das ist auch in
//! Horde die passende Tauschwaehrung: eine Leichtfigur fuer drei Bauern
//! ist ungefaehr ausgeglichen), Bauernstruktur (Phalanx, isolierte
//! Bauern), Vormarsch ueber die Bauern-PST, Figurenaktivitaet und die
//! Koenigssicherheit von Schwarz gegen UMGEWANDELTE Figuren. Fuer Weiss
//! gibt es keine King-Safety (`has_king` false → 0), das ist korrekt: es
//! gibt nichts zu schuetzen.
//!
//! Was `base` NICHT weiss — und dieses Modul obendrauf legt (alle Terme
//! aus Sicht von Weiss, positiv = gut fuer die Horde):
//!
//!   1. Reserve-Abschlag: der 30. Bauer ist nicht so viel wert wie der
//!      10. Die Mattkraft der Horde saettigt — Bauern in den hinteren
//!      Reihen sind Nachschub, keine Angreifer. Deshalb zaehlen Bauern
//!      jenseits einer Grundausstattung etwas weniger als 100.
//!   2. Lebensressource: Schwarz gewinnt, wenn ALLE weissen Steine weg
//!      sind. Eine kleine Restarmee kann gegen ein volles schwarzes Lager
//!      nicht mehr mattsetzen und wird nur noch aufgefressen — das ist
//!      mehr als der lineare Materialverlust, deshalb ein nichtlinearer
//!      Malus fuer wenige verbliebene weisse Steine, skaliert mit der
//!      schwarzen Figurenarmee (die "Jaeger").
//!   3. Bauernkette: ein von einem anderen Bauern gedeckter Bauer kann
//!      von Schwarz nicht kostenlos geschlagen werden (Figur gegen Bauer
//!      ist fuer Schwarz ein schlechtes Geschaeft). Die Kette ist das
//!      Rueckgrat der Horde.
//!   4. Druck auf Bauern: schwarze Figuren (und der schwarze Koenig, der
//!      in Horde ein aktiver Jaeger ist), die weisse Bauern angreifen —
//!      besonders UNGEDECKTE. Das ist der Abtausch-Plan von Schwarz in
//!      Zahlen.
//!   5. Vormarsch: Horde-Bauern muessen nach vorne, um Matt zu drohen und
//!      umzuwandeln (eine Umwandlung ist der einzige Weg zu einer
//!      "richtigen" Figur). Freibauern doppelt, weil Schwarz sie mit einer
//!      Figur blockieren MUSS.
//!   6. Sturm auf den schwarzen Koenig: die generische King-Safety zaehlt
//!      nur Offiziere als Angreifer — in Horde sind die Bauern der Angriff.
//!      Weisse Bauern nahe am schwarzen Koenig und weisse Bauernangriffe
//!      auf seine Nachbarfelder sind das Mattnetz.
//!
//! Alle Werte in Centipawns (Skala wie die Standard-Eval). Signatur und
//! Konvention siehe `crate::variants` (Modul-Doku).

use crate::backend::EngineBoard;
use crate::endgame::chebyshev;
use crate::eval::is_passed;
use crate::eval_config::EvalParams;
use chess::{
    get_bishop_moves, get_king_moves, get_knight_moves, get_rook_moves, BitBoard, Color, Piece,
    EMPTY,
};

// ---------------------------------------------------------------------------
// Term 1: Reserve-Abschlag.
//
// Die ersten FULL_VALUE_PAWNS Bauern zaehlen den vollen Bauernwert aus
// `base` (100). Jeder weitere Bauer bekommt RESERVE_PAWN_DISCOUNT
// abgezogen, ist also nur noch 75 wert. Warum 16? Das ist die Grosse
// eines "normalen" Doppel-Lagers an Bauern — mehr Bauern als das kann
// Weiss vorne gar nicht gleichzeitig einsetzen, die Reihen 1/2 sind
// Nachschub. Die Folge fuer die Suche: solange Weiss > 16 Bauern hat,
// gibt Schwarz eine Leichtfigur erst fuer VIER Bauern her (4 * 75 = 300),
// nicht schon fuer drei — genau die Horde-Faustregel, dass Schwarz frueh
// nicht "billig" tauschen soll.
// ---------------------------------------------------------------------------
const FULL_VALUE_PAWNS: i32 = 16;
const RESERVE_PAWN_DISCOUNT: i32 = 25;

// ---------------------------------------------------------------------------
// Term 2: Lebensressource — Malus fuer eine kleine weisse Restarmee.
//
// Index = Groesse der weissen Armee in Bauern-Einheiten (Bauer 1,
// Leichtfigur 3, Turm 5, Dame 9 — umgewandelte Figuren zaehlen also mit,
// eine Dame ist eine ganze kleine Horde). Ab 10 Einheiten kein Malus.
// Index 0 (kein Stein mehr) ist eigentlich das Spielende und wird von der
// Suche nie bewertet; der Wert steht fuer das UCI-Kommando `eval`.
//
// Der Malus wird mit der schwarzen "Jaeger-Armee" skaliert (siehe
// `hunter_phase`, 0..=24): gegen einen nackten schwarzen Koenig sind drei
// Bauern kein Verlust, sondern eine Umwandlung in spe (Malus 0); gegen
// Dame und Tuerme werden sie nur noch abgeraeumt (voller Malus).
// Groessenordnung: zwei Bauern gegen ein volles Lager ≈ -600 zusaetzlich
// zum Material — praktisch entschieden, aber unterhalb der Mate-Skala.
// ---------------------------------------------------------------------------
const LOW_ARMY_PENALTY: [i32; 10] = [1200, 800, 600, 450, 330, 240, 160, 100, 50, 20];
const HUNTER_PHASE_MAX: i32 = 24;

// ---------------------------------------------------------------------------
// Term 3: Bauernkette — Bonus pro weissem Bauern, der von einem anderen
// weissen Bauern gedeckt ist. Klein pro Bauer, aber bei 30 Bauern
// spuerbar: die Suche soll die Kette geschlossen halten, statt Bauern
// einzeln vorzuschicken. (Die Phalanx nebeneinander bewertet `base`
// schon; hier geht es um die DIAGONALE Deckung, die den Bauern schuetzt.)
// ---------------------------------------------------------------------------
const CHAIN_SUPPORT_BONUS: i32 = 5;

// ---------------------------------------------------------------------------
// Term 4: Druck auf weisse Bauern durch schwarze Steine.
//
// Ein angegriffener, UNGEDECKTER weisser Bauer haengt: Schwarz nimmt ihn
// umsonst — eine Drohung, die die Quiescence zwar sieht, wenn Schwarz am
// Zug ist, aber nicht, wenn Weiss am Zug ist und den Bauern erst noch
// retten muss. Ein angegriffener, GEDECKTER Bauer haengt nicht, bindet
// aber Kraefte und kann Ziel eines Figurenopfers werden → kleiner Malus.
// Aus Sicht von Weiss beides negativ; fuer Schwarz ist das der Anreiz,
// die Figuren dorthin zu stellen, wo sie Bauern anvisieren.
// ---------------------------------------------------------------------------
const HANGING_PAWN_MALUS: i32 = 18;
const PRESSED_PAWN_MALUS: i32 = 4;

// ---------------------------------------------------------------------------
// Term 5: Vormarsch. Index = Reihe des weissen Bauern (0 = Reihe 1,
// 7 = Reihe 8, dort steht nie ein Bauer). Zusaetzlich zur Bauern-PST von
// `base`, die im Horde-Mittelspiel (Phase ≈ 12) nur halb durchschlaegt.
// Ab Reihe 5 beginnt der Bonus, auf Reihe 7 ist der Bauer eine
// Umwandlungs- und Mattdrohung. Freibauern (kein schwarzer Bauer mehr
// davor oder daneben) werden mit PASSED_MULT vervielfacht: Schwarz muss
// sie mit einer FIGUR blockieren, und die fehlt ihm beim Jagen.
// ---------------------------------------------------------------------------
const ADVANCE_BONUS: [i32; 8] = [0, 0, 0, 0, 6, 14, 28, 0];
const PASSED_MULT: i32 = 2;

// ---------------------------------------------------------------------------
// Term 6: Sturm auf den schwarzen Koenig.
//
//   KING_ZONE_PAWN_BONUS: pro weissem Bauern hoechstens zwei Koenigsschritte
//   vom schwarzen Koenig entfernt, der NICHT schon an ihm vorbei ist
//   (Reihe des Bauern ≤ Reihe des Koenigs — ein Bauer kann nicht zurueck,
//   ein bereits vorbeigezogener Bauer bedroht den Koenig nie wieder).
//   KING_RING_ATTACK_BONUS: pro Feld der 3x3-Koenigszone, das ein weisser
//   Bauer angreift. Das sind die Fluchtfelder, die im Mattnetz fehlen.
//
// Beides zaehlt NUR fuer GEDECKTE Bauern (von Weiss angegriffen, i. d. R.
// per Bauernkette): ein ungedeckter Bauer neben dem schwarzen Koenig ist
// kein Mattnetz, sondern Beute — der Koenig nimmt ihn einfach. Ohne diese
// Bedingung wuerde der Term den schwarzen Koenig dafuer bestrafen, dass er
// als Jaeger in die Bauernmasse geht, und genau das ist in Horde richtig.
//
// Bewusst kein Gegenstueck fuer Weiss: die Horde hat keinen Koenig, es
// gibt fuer Schwarz kein Matt zu drohen.
// ---------------------------------------------------------------------------
const KING_ZONE_PAWN_BONUS: i32 = 12;
const KING_RING_ATTACK_BONUS: i32 = 10;
const KING_ZONE_RADIUS: i32 = 2;

/// Ergaenzt die generische Bewertung um die sechs Horde-Terme (s. Modul-
/// Doku). `p` und `phase` werden nicht gebraucht: die Konstanten leben hier
/// im Modul, und die einzige Phasenabhaengigkeit (Term 2) laeuft ueber die
/// schwarze Jaeger-Armee statt ueber die globale Spielphase.
#[inline]
pub fn adjust<B: EngineBoard>(board: &B, _p: &EvalParams, _phase: i32, base: i32) -> i32 {
    base + horde_terms(board)
}

/// Summe der Horde-Terme 1–6 aus Sicht von Weiss (ohne `base`).
fn horde_terms<B: EngineBoard>(board: &B) -> i32 {
    let white = *board.color_combined(Color::White);
    let black = *board.color_combined(Color::Black);
    let white_pawns = *board.pieces(Piece::Pawn) & white;
    let black_pawns = *board.pieces(Piece::Pawn) & black;
    let occ = *board.combined();

    // Term 1 + 2: Bauernzahl und Restarmee.
    let mut score = reserve_discount(white_pawns.popcnt() as i32);
    score -= low_army_penalty(board);

    // Angriffskarten beider Seiten mit der aktuellen Belegung. Fuer Weiss
    // sind das (fast immer) nur die Bauern-Schlagfelder; umgewandelte
    // Figuren kommen automatisch dazu. Fuer Schwarz ALLE Steine inklusive
    // Koenig — in Horde ist der Koenig ein legitimer Bauernjaeger, weil
    // Weiss ihm mit Bauern allein selten gefaehrlich wird.
    let white_pawn_attacks = pawn_attacks(white_pawns, Color::White);
    let white_attacks = white_pawn_attacks | piece_attacks(board, Color::White, occ);
    let black_attacks = pawn_attacks(black_pawns, Color::Black) | piece_attacks(board, Color::Black, occ);

    // Schwarzer Koenig fuer Term 6 (in regelkonformen Horde-Stellungen
    // immer vorhanden; der Guard ist Konvention aller Varianten-Module).
    let black_king = if board.has_king(Color::Black) {
        Some(board.king_square(Color::Black))
    } else {
        None
    };

    // Terme 3–6, je Bauer.
    for sq in white_pawns {
        let bb = BitBoard::from_square(sq);
        let rank = sq.get_rank().to_index();
        let defended = white_attacks & bb != EMPTY;

        // Term 3: diagonal von einem Bauern gedeckt?
        if white_pawn_attacks & bb != EMPTY {
            score += CHAIN_SUPPORT_BONUS;
        }

        // Term 4: angegriffen — haengend oder nur unter Druck?
        if black_attacks & bb != EMPTY {
            score -= if defended {
                PRESSED_PAWN_MALUS
            } else {
                HANGING_PAWN_MALUS
            };
        }

        // Term 5: Vormarsch, Freibauern doppelt.
        let mut advance = ADVANCE_BONUS[rank];
        if advance > 0 && is_passed(sq, Color::White, black_pawns) {
            advance *= PASSED_MULT;
        }
        score += advance;

        // Term 6a: gedeckter Bauer im Umkreis des schwarzen Koenigs, noch
        // vor ihm (nicht schon vorbeigezogen).
        if let (Some(ksq), true) = (black_king, defended) {
            if rank <= ksq.get_rank().to_index() && chebyshev(sq, ksq) <= KING_ZONE_RADIUS {
                score += KING_ZONE_PAWN_BONUS;
            }
        }
    }

    // Term 6b: Felder der Koenigszone (3x3 inkl. Koenigsfeld), die ein
    // GEDECKTER weisser Bauer angreift.
    if let Some(ksq) = black_king {
        let ring = get_king_moves(ksq) | BitBoard::from_square(ksq);
        let defended_pawns = white_pawns & white_attacks;
        score += (pawn_attacks(defended_pawns, Color::White) & ring).popcnt() as i32
            * KING_RING_ATTACK_BONUS;
    }

    score
}

/// Term 1: Abschlag fuer Bauern jenseits der Grundausstattung (negativ
/// oder 0).
#[inline]
fn reserve_discount(white_pawn_count: i32) -> i32 {
    -(white_pawn_count - FULL_VALUE_PAWNS).max(0) * RESERVE_PAWN_DISCOUNT
}

/// Term 2: Malus (positiv zurueckgegeben, wird abgezogen) fuer eine kleine
/// weisse Restarmee, skaliert mit der schwarzen Jaeger-Armee.
fn low_army_penalty<B: EngineBoard>(board: &B) -> i32 {
    let units = army_units(board, Color::White);
    if units >= LOW_ARMY_PENALTY.len() as i32 {
        return 0;
    }
    LOW_ARMY_PENALTY[units as usize] * hunter_phase(board) / HUNTER_PHASE_MAX
}

/// Armee-Groesse von `side` in Bauern-Einheiten: Bauer 1, Springer/Laeufer
/// 3, Turm 5, Dame 9 (Material / 100). Der Koenig zaehlt nicht — er kann
/// weder mattsetzen noch geschlagen werden.
fn army_units<B: EngineBoard>(board: &B, side: Color) -> i32 {
    let ours = *board.color_combined(side);
    let count = |piece: Piece| (*board.pieces(piece) & ours).popcnt() as i32;
    count(Piece::Pawn)
        + 3 * (count(Piece::Knight) + count(Piece::Bishop))
        + 5 * count(Piece::Rook)
        + 9 * count(Piece::Queen)
}

/// "Jaeger-Phase" der schwarzen Figuren in der Skala von `game_phase`
/// (0..=24), aber nur Schwarz gezaehlt: Springer/Laeufer 1, Turm 2, Dame 4
/// → volle Armee 12, verdoppelt 24. Bauern und Koenig zaehlen nicht — sie
/// holen keine Bauern ein, die ueber das ganze Brett verteilt sind.
fn hunter_phase<B: EngineBoard>(board: &B) -> i32 {
    let black = *board.color_combined(Color::Black);
    let count = |piece: Piece| (*board.pieces(piece) & black).popcnt() as i32;
    let army = count(Piece::Knight) + count(Piece::Bishop)
        + 2 * count(Piece::Rook)
        + 4 * count(Piece::Queen);
    army.min(HUNTER_PHASE_MAX / 2) * 2
}

/// Schlagfelder aller Bauern in `pawns` (Weiss schlaegt nach oben, Schwarz
/// nach unten; die Linienmasken verhindern den Ueberlauf a↔h).
#[inline]
fn pawn_attacks(pawns: BitBoard, side: Color) -> BitBoard {
    const NOT_A_FILE: u64 = 0xFEFE_FEFE_FEFE_FEFE;
    const NOT_H_FILE: u64 = 0x7F7F_7F7F_7F7F_7F7F;
    let bb = pawns.0;
    BitBoard(match side {
        Color::White => ((bb << 9) & NOT_A_FILE) | ((bb << 7) & NOT_H_FILE),
        Color::Black => ((bb >> 7) & NOT_A_FILE) | ((bb >> 9) & NOT_H_FILE),
    })
}

/// Angriffsfelder der NICHT-Bauern von `side` (Springer, Laeufer, Turm,
/// Dame, Koenig) mit der Belegung `occ`. Der Koenig wird ueber sein
/// Bitboard gelesen — fehlt er (Weiss in Horde), ist die Schleife leer,
/// `king_square` wird nie angefasst.
fn piece_attacks<B: EngineBoard>(board: &B, side: Color, occ: BitBoard) -> BitBoard {
    let ours = *board.color_combined(side);
    let mut attacks = EMPTY;
    for sq in *board.pieces(Piece::Knight) & ours {
        attacks |= get_knight_moves(sq);
    }
    for sq in (*board.pieces(Piece::Bishop) | *board.pieces(Piece::Queen)) & ours {
        attacks |= get_bishop_moves(sq, occ);
    }
    for sq in (*board.pieces(Piece::Rook) | *board.pieces(Piece::Queen)) & ours {
        attacks |= get_rook_moves(sq, occ);
    }
    for sq in *board.pieces(Piece::King) & ours {
        attacks |= get_king_moves(sq);
    }
    attacks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board_shak::{BoardHorde, ShakVariant};
    use crate::eval::{evaluate, evaluate_breakdown};
    use crate::polyglot::BookSet;
    use crate::search::{search, GoParams, SearchRequest, SearchResult};
    use crate::tt::TranspositionTable;
    use chess::{BoardStatus, ChessMove, Square};
    use shakmaty::fen::Fen;
    use shakmaty::CastlingMode;
    use std::path::Path;
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};

    /// Mate-Codierung der Suche: MATE = 100_000, Schwelle MATE - 1000
    /// (private Konstanten in `search.rs`, hier gespiegelt).
    const MATE_SCORE_MIN: i32 = 99_000;

    /// Lichess-Startstellung der Variante.
    const START_FEN: &str = "rnbqkbnr/pppppppp/8/1PP2PP1/PPPPPPPP/PPPPPPPP/PPPPPPPP/PPPPPPPP w kq - 0 1";

    fn board(fen: &str) -> BoardHorde {
        BoardHorde::from_fen(fen).unwrap_or_else(|e| panic!("FEN {}: {}", fen, e))
    }

    fn eval(fen: &str) -> i32 {
        evaluate(&board(fen), &EvalParams::default())
    }

    fn run_search(fen: &str, depth: Option<u32>, movetime: u64) -> Option<SearchResult> {
        let board = board(fen);
        let req = SearchRequest {
            history: vec![board.get_hash()],
            board,
            halfmove_clock: 0,
            params: GoParams {
                depth,
                movetime: Some(movetime),
                ..GoParams::default()
            },
            tt: Arc::new(Mutex::new(TranspositionTable::new(1))),
            book: Arc::new(BookSet::load(Path::new("."), &[])),
            eval: Arc::new(EvalParams::default()),
            stop: Arc::new(AtomicBool::new(false)),
            pondering: Arc::new(AtomicBool::new(false)),
            move_overhead: 0,
            syzygy: None,
        };
        search(req)
    }

    fn perft(b: &BoardHorde, depth: u32) -> u64 {
        if depth == 0 {
            return 1;
        }
        b.legal_gen()
            .map(|mv| perft(&b.make_move_new(mv), depth - 1))
            .sum()
    }

    // --- Regeln / Adapter --------------------------------------------------

    #[test]
    fn startpos_is_lichess_horde_fen() {
        let start = BoardHorde::startpos();
        assert_eq!(start.to_fen(), START_FEN);
        let parsed = board(START_FEN);
        assert_eq!(parsed.get_hash(), start.get_hash());
        assert_eq!((*start.pieces(Piece::Pawn) & *start.color_combined(Color::White)).popcnt(), 36);
        assert!(!start.has_king(Color::White));
        assert!(start.has_king(Color::Black));
        assert_eq!(start.king_square(Color::Black), Square::E8);
        assert_eq!(start.status(), BoardStatus::Ongoing);
        assert_eq!(shakmaty::variant::Horde::KIND, crate::backend::VariantKind::Horde);
    }

    #[test]
    fn perft_matches_shakmaty() {
        for fen in [
            START_FEN,
            // Mittelspiel: Weiss hat die e-Linie geoeffnet, Schwarz greift
            // mit Figuren in die Bauernmasse.
            "r1bq1rk1/pp1nbppp/2p1pn2/3pP3/1PPP1PP1/P1P1PPPP/PPPPPPPP/PPPPPPPP b - - 0 9",
        ] {
            let b = board(fen);
            let reference: shakmaty::variant::Horde = Fen::from_ascii(fen.as_bytes())
                .unwrap()
                .into_position(CastlingMode::Standard)
                .unwrap();
            for depth in 1..=3 {
                assert_eq!(
                    perft(&b, depth),
                    shakmaty::perft(&reference, depth),
                    "perft({}) fuer {}",
                    depth,
                    fen
                );
            }
        }
    }

    #[test]
    fn first_rank_pawn_may_double_step_without_en_passant() {
        // Bauer d1, d2/d3 frei: d1d2 und d1d3 sind beide legal.
        let b = board("4k3/8/8/8/8/8/8/3P4 w - - 0 1");
        assert!(b.parse_uci_move("d1d2").is_ok());
        let dbl = b.parse_uci_move("d1d3").unwrap();
        let after = b.make_move_new(dbl);
        assert_eq!(after.piece_on(Square::D3), Some(Piece::Pawn));
        // Lichess-Regel: nach dem Doppelschritt von Reihe 1 KEIN en passant.
        assert_eq!(after.en_passant(), None);
        // Zum Vergleich: Doppelschritt von Reihe 2 setzt das ep-Feld
        // (chess-Crate-Konvention: Feld des schlagbaren BAUERN).
        let b2 = board("4k3/8/8/8/4p3/8/3P4/8 w - - 0 1");
        let after2 = b2.make_move_new(b2.parse_uci_move("d2d4").unwrap());
        assert_eq!(after2.en_passant(), Some(Square::D4));
        assert!(after2.parse_uci_move("e4d3").is_ok());
        // Blockierter Reihe-1-Bauer in der Startstellung: kein Doppelschritt.
        assert!(BoardHorde::startpos().parse_uci_move("a1a3").is_err());
    }

    #[test]
    fn black_to_move_without_white_pieces_is_terminal_win() {
        let b = board("4k3/8/8/8/8/8/8/8 b - - 0 1");
        assert!(b.is_variant_win());
        assert!(!b.is_variant_loss());
        assert_eq!(b.legal_gen().count(), 0);
        assert_eq!(b.status(), BoardStatus::Checkmate);
        // Die Suche liefert an einer Endstellung keinen Zug.
        assert!(run_search("4k3/8/8/8/8/8/8/8 b - - 0 1", Some(2), 1_000).is_none());
        // Spiegelfall: Weiss am Zug ohne Steine → Niederlage (Adapter-Test
        // in board_shak.rs), hier nur der Score-Pfad: Schwarz schlaegt den
        // letzten Bauern und gewinnt sofort.
        let b = board("8/8/8/8/8/8/2kP4/8 b - - 0 1");
        let capture = b.parse_uci_move("c2d2").unwrap();
        let after = b.make_move_new(capture);
        assert!(after.is_variant_loss());
        assert_eq!(after.status(), BoardStatus::Checkmate);
    }

    // --- Bewertung ---------------------------------------------------------

    #[test]
    fn startpos_eval_is_moderately_positive_and_breakdown_consistent() {
        // Die Horde steht in der Grundstellung etwas besser (viel Material
        // in Bauern-Einheiten, Schwarz muss 36 Steine schlagen), aber nicht
        // "gewonnen": Reserve-Abschlag daempft die 36 Bauern auf eine
        // plausible Groessenordnung von unter einer Leichtfigur.
        let start = BoardHorde::startpos();
        let p = EvalParams::default();
        let s = evaluate(&start, &p);
        assert!(s > 0 && s < 300, "Startstellung {} cp", s);
        // Breakdown (UCI `eval`) und Hot-Path liefern dasselbe Total.
        let bd = evaluate_breakdown(&start, &p);
        assert_eq!(bd.total, s);
        assert_eq!(bd.variant_adjust, horde_terms(&start));
        // Term 1 in der Grundstellung: 20 Reserve-Bauern.
        assert_eq!(reserve_discount(36), -20 * RESERVE_PAWN_DISCOUNT);
        assert_eq!(reserve_discount(16), 0);
        assert_eq!(reserve_discount(3), 0);
        // Term 2 greift bei 36 Einheiten nicht.
        assert_eq!(low_army_penalty(&start), 0);
        assert_eq!(hunter_phase(&start), HUNTER_PHASE_MAX);
    }

    #[test]
    fn small_white_army_is_penalised_only_against_hunters() {
        // Zwei Bauern gegen das volle schwarze Lager: voller Malus.
        let b = board("rnbqkbnr/pppppppp/8/8/8/8/3PP3/8 w kq - 0 1");
        assert_eq!(army_units(&b, Color::White), 2);
        assert_eq!(low_army_penalty(&b), LOW_ARMY_PENALTY[2]);
        // Dieselben zwei Bauern gegen den nackten Koenig: kein Malus —
        // hier ist die Umwandlung der Plan, nicht die Niederlage.
        let bare = board("4k3/8/8/8/8/8/3PP3/8 w - - 0 1");
        assert_eq!(hunter_phase(&bare), 0);
        assert_eq!(low_army_penalty(&bare), 0);
        // Halbe Jaeger-Armee (Dame + Turm = 6 von 12) → halber Malus.
        let half = board("3qk2r/8/8/8/8/8/3PP3/8 w - - 0 1");
        assert_eq!(hunter_phase(&half), 12);
        assert_eq!(low_army_penalty(&half), LOW_ARMY_PENALTY[2] / 2);
        // Eine umgewandelte Dame zaehlt als neun Einheiten: Dame + Bauer
        // = 10 → kein Malus mehr.
        let queen = board("rnbqkbnr/pppppppp/8/8/8/8/3P4/3Q4 w kq - 0 1");
        assert_eq!(army_units(&queen, Color::White), 10);
        assert_eq!(low_army_penalty(&queen), 0);
        // Und in der Gesamtbewertung: zwei Bauern gegen volles Lager sind
        // klar verloren, deutlich unter dem reinen Materialsaldo.
        let s = eval("rnbqkbnr/pppppppp/8/8/8/8/3PP3/8 w kq - 0 1");
        assert!(s < -3600 - LOW_ARMY_PENALTY[2] / 2, "score {}", s);
    }

    #[test]
    fn chain_support_and_hanging_pawns() {
        // Weisser Bauer e4 vom schwarzen Laeufer h7 angegriffen (Diagonale
        // h7-g6-f5-e4, dahinter verstellt — d3 sieht er also NICHT). Ohne
        // Deckung haengt e4; mit Bauer d3 dahinter ist er gedeckt (Kette)
        // und nur "unter Druck". Nur die Horde-Terme vergleichen, damit
        // Material/PST des zusaetzlichen Bauern nicht hineinspielen: dazu
        // die Deckung einmal durch einen Bauern auf d3 (deckt e4) und
        // einmal durch einen Bauern auf a2 (deckt nichts) stellen.
        let hanging = board("4k3/7b/8/8/4P3/8/P7/8 w - - 0 1");
        let chained = board("4k3/7b/8/8/4P3/3P4/8/8 w - - 0 1");
        let h = horde_terms(&hanging);
        let c = horde_terms(&chained);
        // Unterschied: Kette (+5) und haengend (-18) → gedeckt (-4).
        assert_eq!(
            c - h,
            CHAIN_SUPPORT_BONUS + HANGING_PAWN_MALUS - PRESSED_PAWN_MALUS
        );
        // Ohne Angreifer: nur der Kettenbonus unterscheidet.
        let free = board("4k3/8/8/8/4P3/8/P7/8 w - - 0 1");
        let free_chained = board("4k3/8/8/8/4P3/3P4/8/8 w - - 0 1");
        assert_eq!(horde_terms(&free_chained) - horde_terms(&free), CHAIN_SUPPORT_BONUS);
        // Der schwarze Koenig zaehlt als Angreifer (Bauernjaeger).
        let king_attacks = board("8/8/8/8/4k3/4P3/8/P7 w - - 0 1");
        let king_far = board("8/8/8/8/8/4P3/8/P6k w - - 0 1");
        assert!(horde_terms(&king_attacks) < horde_terms(&king_far));
    }

    #[test]
    fn advanced_and_passed_pawns_score_higher() {
        // Gleicher Bauer auf Reihe 4 / 5 / 6 / 7 (nur Horde-Terme, ohne
        // schwarzen Bauern → Freibauer, doppelt). Der schwarze Koenig steht
        // weit weg (a8), damit Term 6 nicht greift.
        let r4 = horde_terms(&board("k7/8/8/8/4P3/8/8/8 w - - 0 1"));
        let r5 = horde_terms(&board("k7/8/8/4P3/8/8/8/8 w - - 0 1"));
        let r6 = horde_terms(&board("k7/8/4P3/8/8/8/8/8 w - - 0 1"));
        let r7 = horde_terms(&board("k7/4P3/8/8/8/8/8/8 w - - 0 1"));
        assert_eq!(r5 - r4, PASSED_MULT * ADVANCE_BONUS[4]);
        assert!(r6 > r5 && r7 > r6, "r5 {} r6 {} r7 {}", r5, r6, r7);
        // Nicht-Freibauer (schwarzer Bauer e7 davor): nur einfacher Bonus.
        let blocked = horde_terms(&board("k7/4p3/8/4P3/8/8/8/8 w - - 0 1"));
        // Der schwarze Bauer e7 greift d6/f6 an, nicht e5 → kein Druck-Malus.
        assert_eq!(blocked, ADVANCE_BONUS[4] - low_army_penalty(&board("k7/4p3/8/4P3/8/8/8/8 w - - 0 1")));
    }

    #[test]
    fn pawn_storm_near_black_king_is_rewarded() {
        // Sechs weisse Bauern als Kette (f6/g6/h6, gedeckt von f5/g5/h5)
        // nahe am schwarzen Koenig g8 gegen dieselbe Kette am anderen
        // Fluegel (a-c): Sturm zaehlt, Fern-Bauern nicht. Alles andere
        // (Kette, Vormarsch, Material) ist in beiden Stellungen gleich.
        let storm = board("6k1/8/5PPP/5PPP/8/8/8/8 w - - 0 1");
        let far = board("6k1/8/PPP5/PPP5/8/8/8/8 w - - 0 1");
        let s = horde_terms(&storm);
        let f = horde_terms(&far);
        // f6/g6/h6 liegen im 2er-Umkreis von g8 und sind gedeckt (3 Zonen-
        // Bauern); f5/g5/h5 sind drei Schritte weg. Ring-Angriffe: f6→g7,
        // g6→f7/h7, h6→g7 → drei verschiedene Felder (g7 zaehlt einmal).
        let ring = (pawn_attacks(*storm.pieces(Piece::Pawn), Color::White)
            & (get_king_moves(Square::G8) | BitBoard::from_square(Square::G8)))
        .popcnt() as i32;
        assert_eq!(ring, 3);
        assert_eq!(s - f, 3 * KING_ZONE_PAWN_BONUS + ring * KING_RING_ATTACK_BONUS);
        // UNGEDECKTE Bauern neben dem Koenig sind Beute, kein Sturm: die
        // drei Bauern ohne Kette dahinter bekommen weder Zonen- noch
        // Ring-Bonus (der Koenig auf g8 greift sie selbst nicht an, also
        // auch kein Druck-Malus) → nur der Vormarsch (Reihe 6, Freibauern)
        // bleibt uebrig.
        let loose = board("6k1/8/5PPP/8/8/8/8/8 w - - 0 1");
        assert_eq!(
            horde_terms(&loose),
            3 * PASSED_MULT * ADVANCE_BONUS[5] - low_army_penalty(&loose)
        );
        // Ein Bauer, der schon am Koenig vorbei ist (Koenig g5, gedeckter
        // Bauer g6 darueber): kein Zonen-Bonus, obwohl er im Umkreis steht.
        // Zum Vergleich derselbe gedeckte Bauer VOR dem Koenig (g4/f3).
        let passed_by = board("8/8/6P1/5Pk1/8/8/8/8 w - - 0 1");
        let not_yet = board("8/8/8/6k1/6P1/5P2/8/8 w - - 0 1");
        assert!(horde_terms(&not_yet) > horde_terms(&passed_by));
    }

    // --- Suche -------------------------------------------------------------

    #[test]
    fn search_finds_pawn_mate_in_one() {
        // Schwarz: Kh8, Bauer h7. Weiss: Bauern f6, f7, g6. g6-g7+ ist
        // Matt (g7 von f6 gedeckt, g8 von f7 gedeckt, h7 eigener Bauer);
        // f7-f8=D+ setzt ebenfalls matt. Die Suche muss einen Mattzug mit
        // Mate-Score finden.
        let fen = "7k/5P1p/5PP1/8/8/8/8/8 w - - 0 1";
        let result = run_search(fen, Some(3), 10_000).unwrap();
        assert!(result.score > MATE_SCORE_MIN, "score {}", result.score);
        let after = board(fen).make_move_new(result.best);
        assert_eq!(after.status(), BoardStatus::Checkmate, "bestmove {}", result.best);
        assert!(after.checkers().popcnt() > 0, "kein Schach nach {}", result.best);
        assert!(!after.is_variant_win() && !after.is_variant_loss());
    }

    #[test]
    fn search_captures_last_pawn_for_immediate_win() {
        // Schwarzer Koenig c2, letzter weisser Bauer d2 (greift c3/e3 an,
        // kein Schach): Kxd2 nimmt den letzten Stein → Schwarz gewinnt.
        let fen = "8/8/8/8/8/8/2kP4/8 b - - 0 1";
        let result = run_search(fen, Some(2), 10_000).unwrap();
        assert_eq!(result.best, ChessMove::new(Square::C2, Square::D2, None));
        assert!(result.score > MATE_SCORE_MIN, "score {}", result.score);
    }

    #[test]
    fn search_runs_from_startpos_with_movetime() {
        // Reine Zeitvorgabe (wie `go movetime`): legaler Zug, plausibler
        // Score (kein Mate-/Panikwert), keine Panik trotz 36 Bauern und
        // koenigslosem Weiss.
        let result = run_search(START_FEN, None, 300).expect("Suche liefert einen Zug");
        let legal: Vec<ChessMove> = BoardHorde::startpos().legal_gen().collect();
        assert!(legal.contains(&result.best), "illegaler bestmove {}", result.best);
        assert!(result.score.abs() < 1000, "score {}", result.score);
        // Und aus Sicht von Schwarz in einer Mittelspielstellung.
        let fen = "r1bq1rk1/pp1nbppp/2p1pn2/3pP3/1PPP1PP1/P1P1PPPP/PPPPPPPP/PPPPPPPP b - - 0 9";
        let result = run_search(fen, None, 300).expect("Suche liefert einen Zug");
        let legal: Vec<ChessMove> = board(fen).legal_gen().collect();
        assert!(legal.contains(&result.best), "illegaler bestmove {}", result.best);
        assert!(result.score.abs() < 1500, "score {}", result.score);
    }
}
