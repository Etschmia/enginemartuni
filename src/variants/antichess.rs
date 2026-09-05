//! Antichess (Raeuberschach) — varianten-spezifische Bewertung.
//!
//! Regeln: Schlagzwang, kein Schach, der Koenig ist eine normale Figur und
//! kann fehlen (`has_king` pruefen!). Wer alle eigenen Steine verliert oder
//! keinen legalen Zug mehr hat, GEWINNT.
//!
//! Die generische Martuni-Bewertung (`base`) ist hier nicht nur nutzlos,
//! sondern verkehrt herum: sie belohnt Material, Figurenaktivitaet,
//! Koenigssicherheit und Bauernvormarsch — alles Dinge, die im Raeuberschach
//! eine LAST sind. Deshalb wird `base` komplett verworfen und durch eine
//! kleine, eigene Bewertung ersetzt. Die Terme sind bewusst wenige und
//! einfach; die eigentliche Antichess-"Taktik" (Opfer, die den Gegner zum
//! Schlagen zwingen, und Schlagketten) loest die Suche, weil die
//! Zuggenerierung den Schlagzwang exakt abbildet.
//!
//! Alle Terme werden fuer beide Seiten berechnet und als Differenz
//! (Weiss − Schwarz) zurueckgegeben; positiv = gut fuer Weiss. Die Suche
//! dreht das Vorzeichen selbst auf die Seite am Zug.
//!
//! Signatur und Konvention siehe `crate::variants` (Modul-Doku).

use crate::backend::EngineBoard;
use crate::eval_config::EvalParams;
use chess::{
    get_bishop_moves, get_king_moves, get_knight_moves, get_pawn_attacks, get_rook_moves, Color,
    Piece, EMPTY,
};

// ---------------------------------------------------------------------------
// Term 1: Material — UMGEKEHRT.
//
// Ziel im Raeuberschach ist, alle eigenen Steine loszuwerden. Jeder Stein,
// den ich noch habe, ist Weg, den ich noch gehen muss; jeder Stein, den der
// Gegner noch hat, ist Weg fuer ihn. Also: eigenes Material ZAEHLT NEGATIV.
// Die Werte bleiben in der gewohnten Centipawn-Groessenordnung (100/300/
// 300/500/900), damit die Suche (Mate-Schwelle, Fenster, Delta-Vergleiche)
// mit denselben Skalen arbeitet wie im Standardschach.
//
// Der Koenig ist hier eine gewoehnliche Figur ohne Sonderrolle. Er zieht
// wie ein Koenig (8 Felder, keine Reichweite) — von der "Last" her also
// etwa ein Springer, daher 300. Damit hat auch die Umwandlung in einen
// Koenig (im Raeuberschach erlaubt) einen sinnvollen Preis: sie ist so
// billig wie die in einen Springer/Laeufer und viel billiger als eine Dame.
// ---------------------------------------------------------------------------
const VAL_PAWN: i32 = 100;
const VAL_KNIGHT: i32 = 300;
const VAL_BISHOP: i32 = 300;
const VAL_ROOK: i32 = 500;
const VAL_QUEEN: i32 = 900;
const VAL_KING: i32 = 300;

// ---------------------------------------------------------------------------
// Term 2: Angriffs-Reichweite als MALUS ("Mobilitaets-Malus").
//
// Warum ist Reichweite hier schlecht? Wegen des Schlagzwangs: jedes Feld,
// das eine meiner Figuren angreift, ist ein Feld, auf dem der Gegner mir
// einen Stein HINSTELLEN kann — und dann MUSS ich schlagen. Genauso: greift
// meine Figur ein eigenes Feld (Deckung) an, kann der Gegner dort schlagen
// und mich zum Rueckschlag zwingen. Je mehr Felder ich insgesamt angreife,
// desto leichter kann der Gegner mir Material aufzwingen und meine Zuege
// diktieren; eine "vergrabene" Figur, die nichts angreift, ist dagegen
// kaum erzwingbar. Wir zaehlen deshalb pro Figur die angegriffenen Felder
// (mit Mehrfachzaehlung, Gleiter mit aktueller Belegung, Bauern ihre zwei
// Schlagfelder) und ziehen pro Feld einen kleinen Betrag ab.
//
// Bewusst NICHT "legale Zuege" gezaehlt: legale Zuege sind unter Schlagzwang
// oft nur die Schlagzuege, das sagt nichts ueber die Erzwingbarkeit aus.
// Groessenordnung: eine offene Dame (~25 Felder) kostet ~75 cp zusaetzlich
// zu ihren 900 — die Dame ist die groesste Last, so wie es die Praxis
// des Raeuberschachs auch zeigt.
// ---------------------------------------------------------------------------
const ATTACK_SQUARE_MALUS: i32 = 3;

// ---------------------------------------------------------------------------
// Term 3: Weit vorgerueckte Bauern als MALUS.
//
// Ein Bauer kann nicht zurueck. Je weiter er vorne steht, desto naeher ist
// die Umwandlung — und die erzeugt aus einem 100er-Stein eine neue Figur
// (mindestens 300, mit grosser Reichweite = Term 2), die ich dann wieder
// loswerden muss. Ausserdem verliert ein weit vorgerueckter Bauer seine
// Wartezuege (Tempo-Reserve), die im Raeuberschach oft entscheiden, wer
// in eine erzwungene Schlagfolge laufen muss. Der Malus ist klein und
// waechst mit der Reihe; auf der 8. Reihe steht kein Bauer mehr (dann ist
// er umgewandelt und wird ueber Term 1/2 bewertet). Index = relative
// Reihe (0 = eigene Grundreihe, 6 = eine Reihe vor der Umwandlung).
// ---------------------------------------------------------------------------
const PAWN_ADVANCE_MALUS: [i32; 8] = [0, 0, 0, 0, 4, 10, 20, 0];

/// Ersetzt die generische Bewertung komplett (siehe Modul-Doku). `base`,
/// `p` und `phase` werden bewusst ignoriert: kein Term der Standard-Eval
/// gilt hier, und eine Phasen-Interpolation gibt es nicht.
#[inline]
pub fn adjust<B: EngineBoard>(board: &B, _p: &EvalParams, _phase: i32, _base: i32) -> i32 {
    side_score(board, Color::White) - side_score(board, Color::Black)
}

/// Bewertung EINER Seite: negatives Material minus Reichweite minus
/// Bauernvormarsch. Rueckgabe aus Sicht dieser Seite (hoeher = besser fuer
/// `us`); die Differenz beider Seiten bildet `adjust`.
fn side_score<B: EngineBoard>(board: &B, us: Color) -> i32 {
    -material(board, us)
        - ATTACK_SQUARE_MALUS * attack_squares(board, us)
        - pawn_advance_malus(board, us)
}

/// Materialsumme von `us` (Koenig als normale Figur, siehe VAL_KING).
fn material<B: EngineBoard>(board: &B, us: Color) -> i32 {
    let ours = *board.color_combined(us);
    let count = |piece: Piece| (*board.pieces(piece) & ours).popcnt() as i32;
    count(Piece::Pawn) * VAL_PAWN
        + count(Piece::Knight) * VAL_KNIGHT
        + count(Piece::Bishop) * VAL_BISHOP
        + count(Piece::Rook) * VAL_ROOK
        + count(Piece::Queen) * VAL_QUEEN
        + count(Piece::King) * VAL_KING
}

/// Anzahl der von `us` angegriffenen Felder, ueber alle Figuren summiert
/// (Mehrfachzaehlung gewollt: zwei Figuren auf ein Feld = zwei Wege, dort
/// zum Schlagen gezwungen zu werden). Gleiter sehen die aktuelle Belegung
/// (der erste Stein auf der Linie ist noch angreifbar, dahinter nicht).
/// Nutzt bewusst `pieces(Piece::King)` statt `king_square`: fehlt der
/// Koenig, ist das Bitboard leer und die Schleife laeuft einfach nicht.
fn attack_squares<B: EngineBoard>(board: &B, us: Color) -> i32 {
    let ours = *board.color_combined(us);
    let occ = *board.combined();
    let mut n = 0;
    for sq in *board.pieces(Piece::Pawn) & ours {
        n += get_pawn_attacks(sq, us, !EMPTY).popcnt();
    }
    for sq in *board.pieces(Piece::Knight) & ours {
        n += get_knight_moves(sq).popcnt();
    }
    for sq in *board.pieces(Piece::Bishop) & ours {
        n += get_bishop_moves(sq, occ).popcnt();
    }
    for sq in *board.pieces(Piece::Rook) & ours {
        n += get_rook_moves(sq, occ).popcnt();
    }
    for sq in *board.pieces(Piece::Queen) & ours {
        n += (get_rook_moves(sq, occ) | get_bishop_moves(sq, occ)).popcnt();
    }
    for sq in *board.pieces(Piece::King) & ours {
        n += get_king_moves(sq).popcnt();
    }
    n as i32
}

/// Summe der Vormarsch-Mali aller Bauern von `us` (relative Reihe, so dass
/// Weiss und Schwarz spiegelgleich behandelt werden).
fn pawn_advance_malus<B: EngineBoard>(board: &B, us: Color) -> i32 {
    let ours = *board.color_combined(us);
    let mut malus = 0;
    for sq in *board.pieces(Piece::Pawn) & ours {
        let rank = sq.get_rank().to_index();
        let relative = match us {
            Color::White => rank,
            Color::Black => 7 - rank,
        };
        malus += PAWN_ADVANCE_MALUS[relative];
    }
    malus
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board_shak::BoardAntichess;
    use crate::eval::evaluate;
    use crate::polyglot::BookSet;
    use crate::search::{search, GoParams, SearchRequest, SearchResult};
    use crate::tt::TranspositionTable;
    use chess::{ChessMove, Square};
    use std::path::Path;
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};

    /// Mate-Codierung der Suche: MATE = 100_000, Schwelle MATE - 1000
    /// (private Konstanten in `search.rs`, hier gespiegelt).
    const MATE_SCORE_MIN: i32 = 99_000;

    fn eval(fen: &str) -> i32 {
        let board = BoardAntichess::from_fen(fen).unwrap();
        evaluate(&board, &EvalParams::default())
    }

    fn run_search(fen: &str, depth: Option<u32>, movetime: u64) -> Option<SearchResult> {
        let board = BoardAntichess::from_fen(fen).unwrap();
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

    #[test]
    fn startpos_is_symmetric() {
        // Spiegelgleiche Stellung → exakt 0 (Material, Reichweite und
        // Bauernreihen sind fuer beide Seiten identisch).
        assert_eq!(eval("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w - - 0 1"), 0);
        let start = BoardAntichess::startpos();
        assert_eq!(evaluate(&start, &EvalParams::default()), 0);
    }

    #[test]
    fn less_material_is_better() {
        // Weiss hat nur noch einen Bauern, Schwarz fast alles → Weiss steht
        // (aus Sicht des Raeuberschachs) klar besser: positiv.
        let s = eval("rnbqkbnr/pppppppp/8/8/8/8/4P3/8 w - - 0 1");
        assert!(s > 3000, "Weiss mit 1 Bauer vs. volles Lager: {}", s);
        // Spiegelbild → gleicher Betrag, anderes Vorzeichen.
        let m = eval("8/4p3/8/8/8/8/PPPPPPPP/RNBQKBNR w - - 0 1");
        assert_eq!(m, -s);
        // Eine Dame loszuwerden ist besser als einen Bauern loszuwerden:
        // Weiss ohne Dame vs. Weiss ohne einen Bauern.
        let no_queen = eval("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNB1KBNR w - - 0 1");
        let no_pawn = eval("rnbqkbnr/pppppppp/8/8/8/8/PPPP1PPP/RNBQKBNR w - - 0 1");
        assert!(no_queen > no_pawn, "ohne Dame {} vs. ohne Bauer {}", no_queen, no_pawn);
        // Der Koenig zaehlt als normale Figur: ohne Koenig besser als mit.
        let no_king = eval("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQ1BNR w - - 0 1");
        assert!(no_king > 0 && no_king < no_queen);
    }

    #[test]
    fn material_values_are_centipawn_scaled() {
        // Reine Materialdifferenz ohne Reichweite/Vormarsch: zwei Steine
        // ohne Angriffsfelder gibt es nicht, also ueber die Hilfsfunktion.
        let board = BoardAntichess::from_fen("q6k/8/8/8/8/8/8/R6N w - - 0 1").unwrap();
        assert_eq!(material(&board, Color::White), VAL_ROOK + VAL_KNIGHT);
        assert_eq!(material(&board, Color::Black), VAL_QUEEN + VAL_KING);
    }

    #[test]
    fn wide_attack_range_is_a_liability() {
        // Zwei Tuerme allein auf dem Brett: beide sehen 14 Felder → die
        // Reichweite ist symmetrisch, die Bewertung exakt 0.
        let board = BoardAntichess::from_fen("7r/8/8/8/8/8/8/R7 w - - 0 1").unwrap();
        assert_eq!(attack_squares(&board, Color::White), 14);
        assert_eq!(attack_squares(&board, Color::Black), 14);
        assert_eq!(evaluate(&board, &EvalParams::default()), 0);

        // Ein schwarzer Bauer auf h7 (relative Reihe 1 → kein Vormarsch-
        // Malus) nimmt dem Turm h8 die h-Linie: Turm sieht g8..a8 (7) plus
        // h7 (1), der Bauer greift g6 an (1) → 9 statt 14 Felder.
        let blocked = BoardAntichess::from_fen("7r/7p/8/8/8/8/8/R7 w - - 0 1").unwrap();
        assert_eq!(attack_squares(&blocked, Color::White), 14);
        assert_eq!(attack_squares(&blocked, Color::Black), 9);
        // Schwarz traegt jetzt einen Bauern MEHR (Last +100) und hat 5 Felder
        // WENIGER Reichweite (Entlastung 5 * 3 = 15): netto steht Weiss um
        // 85 cp besser — die Reichweite mildert, kippt aber nicht das
        // Material. Genau diese Groessenordnung ist gewollt.
        let s = evaluate(&blocked, &EvalParams::default());
        assert_eq!(s, VAL_PAWN - ATTACK_SQUARE_MALUS * 5);
    }

    #[test]
    fn queen_costs_more_than_its_material() {
        // Gleiche Reichweite-Logik: eine freie Dame im Zentrum greift 27
        // Felder an; eine in der Ecke eingeklemmte deutlich weniger. Die
        // zentrale Dame ist die groessere Last.
        let central = BoardAntichess::from_fen("8/8/8/3Q4/8/8/8/7k w - - 0 1").unwrap();
        let corner = BoardAntichess::from_fen("8/8/8/8/8/8/8/Q6k w - - 0 1").unwrap();
        let p = EvalParams::default();
        assert!(evaluate(&corner, &p) > evaluate(&central, &p));
        assert_eq!(attack_squares(&central, Color::White), 27);
    }

    #[test]
    fn advanced_pawn_is_worse_than_home_pawn() {
        // Gleiches Material, gleiche Reichweite (je 1 Randbauer = 1
        // Schlagfeld): nur der Vormarsch unterscheidet. Weisser Bauer auf
        // a7 (relative Reihe 6) gegen schwarzen Bauern auf h7 (relative
        // Reihe 1) → Weiss schlechter, genau um den Malus.
        let s = eval("8/P6p/8/8/8/8/8/8 w - - 0 1");
        assert_eq!(s, -PAWN_ADVANCE_MALUS[6]);
        // Umgekehrt: Bauer auf a2 gegen Bauer auf h2 → Schwarz traegt den
        // Malus.
        let t = eval("8/8/8/8/8/8/P6p/8 w - - 0 1");
        assert_eq!(t, PAWN_ADVANCE_MALUS[6]);
        // Auf den ersten Reihen kein Malus.
        let board = BoardAntichess::from_fen("8/8/8/8/8/P7/8/8 w - - 0 1").unwrap();
        assert_eq!(pawn_advance_malus(&board, Color::White), 0);
    }

    #[test]
    fn kingless_position_evaluates_without_panic() {
        // Beide Koenige sind weg — jeder Term muss ohne `king_square`
        // auskommen (Platzhalter A1/A8 duerfen nicht in die Bewertung).
        let board = BoardAntichess::from_fen("r1b3n1/pp3ppp/2p5/8/3P4/2N5/PP3PPP/R1B5 w - - 0 1")
            .unwrap();
        assert!(!board.has_king(Color::White));
        assert!(!board.has_king(Color::Black));
        let s = evaluate(&board, &EvalParams::default());
        // Weiss: R+B+N+6P = 1700, Schwarz: R+B+N+6P = 1700 → nur Reichweite
        // und Vormarsch (d4 = relative Reihe 3, kein Malus) unterscheiden.
        assert!(s.abs() < 200, "kingless eval {}", s);
    }

    // --- Suche -----------------------------------------------------------

    #[test]
    fn search_gives_away_last_piece_for_immediate_win() {
        // Weiss hat nur den Laeufer c1, Schwarz nur den Turm h8. Bh6 stellt
        // den Laeufer in die h-Linie: Schwarz MUSS Rxh6 schlagen, Weiss hat
        // keine Steine mehr und gewinnt. Die sechs anderen Laeuferzuege
        // gewinnen nicht sofort — die Suche muss Bh6 mit Mate-Score finden.
        let result = run_search("7r/8/8/8/8/8/8/2B5 w - - 0 1", Some(3), 10_000).unwrap();
        assert_eq!(result.best, ChessMove::new(Square::C1, Square::H6, None));
        assert!(result.score > MATE_SCORE_MIN, "score {}", result.score);
    }

    #[test]
    fn search_recognises_own_stalemate_as_win() {
        // Weiss: Bauern a2/h2, Schwarz: Bauern a4/h4 + Koenig e8. Nach a3
        // und h3 sind beide weissen Bauern blockiert, Weiss hat keinen
        // legalen Zug mehr → Weiss GEWINNT (Patt = Sieg). Schwarz kann das
        // nicht verhindern (der Koenig erreicht die Bauern nicht rechtzeitig,
        // die schwarzen Bauern sind blockiert). Zwei echte Kandidaten an der
        // Wurzel (a3/h3), beide gewinnen in 4 Halbzuegen.
        let result = run_search("4k3/8/8/8/p6p/8/P6P/8 w - - 0 1", Some(6), 10_000).unwrap();
        let a3 = ChessMove::new(Square::A2, Square::A3, None);
        let h3 = ChessMove::new(Square::H2, Square::H3, None);
        assert!(result.best == a3 || result.best == h3, "best {}", result.best);
        assert!(result.score > MATE_SCORE_MIN, "score {}", result.score);
        // Und direkt am Brett: die blockierte Stellung ist ein Sieg der
        // Seite am Zug.
        let stuck = BoardAntichess::from_fen("3k4/8/8/8/p6p/P6P/8/8 w - - 0 1").unwrap();
        assert!(stuck.is_variant_win());
        assert_eq!(stuck.legal_gen().count(), 0);
    }

    #[test]
    fn search_runs_on_kingless_middlegame_with_movetime() {
        // Typische Raeuberschach-Mittelspielstellung ohne Koenige, reine
        // Zeitvorgabe (wie `go movetime`): muss ohne Panik einen legalen
        // Zug liefern.
        let fen = "r1b3n1/pp3ppp/2p5/8/3P4/2N5/PP3PPP/R1B5 w - - 0 1";
        let result = run_search(fen, None, 300).expect("Suche liefert einen Zug");
        let board = BoardAntichess::from_fen(fen).unwrap();
        let legal: Vec<ChessMove> = board.legal_gen().collect();
        assert!(legal.contains(&result.best), "illegaler bestmove {}", result.best);
    }
}
