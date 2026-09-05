//! Three-Check — varianten-spezifische Bewertung.
//!
//! Regeln: Standardschach plus ein zweiter Gewinnweg — wer dem Gegner das
//! DRITTE Schach bietet, hat sofort gewonnen. Matt zaehlt weiterhin. Die
//! Zuggenerierung (shakmaty) zaehlt die Schachs mit, nimmt sie in den
//! Zobrist-Hash auf und meldet das dritte Schach als Spielende; die Suche
//! sieht es ueber `is_variant_loss` (leere Zugliste) wie ein Matt.
//!
//! Was die generische Bewertung (`base`) NICHT weiss:
//!
//!   1. Wie viele Schachs jede Seite schon gegeben hat. Das ist in dieser
//!      Variante der zentrale "Materialwert" neben den Figuren — zwei
//!      gegebene Schachs bedeuten, dass JEDES weitere Schach die Partie
//!      beendet. Deshalb ein eskalierender Bonus je gegebenem Schach
//!      (das zweite ist deutlich mehr wert als das erste).
//!   2. Dass Koenigssicherheit hier nicht nur "Matt vermeiden" heisst,
//!      sondern "ueberhaupt keine Schachs zulassen". Ein Koenig, der auf
//!      offenen Linien und Diagonalen steht, ist eine Schach-Quelle fuer
//!      den Gegner — auch dann, wenn er nie mattgesetzt wuerde. Deshalb
//!      ein eigener Expositions-Term (offene Strahlen zum Koenig, wenige
//!      eigene Verteidiger drumherum).
//!   3. Dass die vorhandene Angriffs-Druck-Rechnung (`king_danger` in
//!      eval.rs: gegnerische Offiziere, die die 3x3-Koenigszone bestreichen)
//!      fuer Three-Check viel zu zaghaft gewichtet ist. Sie wird hier ein
//!      zweites Mal mit denselben Gewichten/Tabellen aus eval.toml
//!      berechnet und mit einem kraeftigen Faktor obendrauf gelegt — so
//!      bleibt eval.rs unangetastet (Standardpfad bit-exakt), und der
//!      Verstaerkungsfaktor ist an EINER Stelle als Konstante sichtbar.
//!
//! Terme 2 und 3 werden nach dem "Schach-Stand" des ANGREIFERS eskaliert:
//! braucht der Gegner nur noch ein Schach, ist jede Luecke in der
//! Koenigsstellung doppelt so gefaehrlich wie zu Partiebeginn.
//!
//! Alle Werte in Centipawns (Skala wie die Standard-Eval: Bauer 100,
//! Leichtfigur 300, Turm 500, Dame 900). `base` bleibt vollstaendig
//! erhalten (Material, PSTs, Bauernstruktur, Koenigssicherheit usw. gelten
//! unveraendert), dieses Modul addiert die Differenz Weiss − Schwarz der
//! drei Zusatzterme. Rueckgabe aus Sicht von Weiss; Signatur und Konvention
//! siehe `crate::variants` (Modul-Doku).

use crate::backend::EngineBoard;
use crate::eval_config::EvalParams;
use chess::{
    get_bishop_moves, get_king_moves, get_knight_moves, get_rook_moves, BitBoard, Color, Piece,
    EMPTY,
};

// ---------------------------------------------------------------------------
// Grundgroessen
// ---------------------------------------------------------------------------

/// Anzahl Schachs, die eine Seite zum Sieg geben muss (Lichess-Zaehlweise:
/// `checks_remaining` startet bei 3 und faellt auf 0 = gewonnen).
const CHECKS_TO_WIN: u8 = 3;

/// Bewertung einer bereits ENTSCHIEDENEN Stellung (eine Seite hat ihr
/// drittes Schach gegeben). Die Suche kommt hier nie hin — sie erkennt das
/// Ende ueber die leere Zugliste und vergibt einen echten Matt-Score — aber
/// das UCI-Kommando `eval` und statische Analysen sollen trotzdem einen
/// eindeutigen Wert sehen. Weit ueber jeder Materialdifferenz, aber unter
/// der Matt-Schwelle der Suche (MATE − 1000 = 99 000), damit kein
/// Ply-Adjustment fuer Mate-Scores darauf anspringt.
const DECIDED: i32 = 10_000;

// ---------------------------------------------------------------------------
// Term 1: Bonus je bereits gegebenem Schach (Index = gegebene Schachs 0..=2).
//
// Warum eskalierend? Nach dem ersten Schach hat sich am Brett noch wenig
// geaendert — der Gegner muss lediglich etwas vorsichtiger stehen. Nach dem
// ZWEITEN Schach entscheidet jedes weitere: ab jetzt gewinnt jeder
// Abzug, jedes Opfer mit Schach, jede Springergabel mit Schach — der
// Gegner muss praktisch jede Schach-Moeglichkeit im Voraus verhindern und
// verliert dafuer laufend Tempi und Material. Das erste Schach wiegt daher
// etwas mehr als einen Bauern, das zweite kommt einem Turm nahe (+210 auf
// das erste, insgesamt +330).
// ---------------------------------------------------------------------------
const CHECK_GIVEN_BONUS: [i32; 3] = [0, 120, 330];

// ---------------------------------------------------------------------------
// Eskalation der Koenigs-Terme (2 und 3) nach dem Schach-Stand des
// ANGREIFERS, in Promille. Index = vom Angreifer schon gegebene Schachs.
//
//   0 gegeben: normaler Faktor (1,0) — Koenigsexposition ist ein Risiko,
//              aber noch kein akutes.
//   1 gegeben: 1,5 — jede Luecke ist jetzt der Weg zu Schach Nr. 2.
//   2 gegeben: 2,2 — ein einziges Schach beendet die Partie; die
//              Koenigsstellung ist damit wichtiger als jede Figur.
// ---------------------------------------------------------------------------
const ESCALATION_PERMILLE: [i32; 3] = [1000, 1500, 2200];

// ---------------------------------------------------------------------------
// Term 2: Koenigsexposition — "von wie vielen Feldern aus koennte der
// Gegner den Koenig SOFORT im Schach haben?"
//
// Wir gehen vom Koenigsfeld aus rueckwaerts: Ein Laeufer/eine Dame koennte
// von jedem Feld auf einer freien Diagonale des Koenigs Schach geben, ein
// Turm/eine Dame von jedem Feld auf einer freien Linie/Reihe, ein Springer
// von jedem Springer-Sprungfeld. Diese "Schachfelder" zaehlen wir —
// getrennt nach Figurenart und nur, wenn der Gegner die passende Figur
// ueberhaupt noch hat (ohne Laeufer/Dame nuetzt eine offene Diagonale
// niemandem).
//
// Zwei Feinheiten:
//   - Die acht Nachbarfelder des Koenigs zaehlen NICHT mit. Sie sind immer
//     "sichtbar" und wuerden jeden Koenig gleich bestrafen; ausserdem
//     deckt der Koenig sie selbst, ein Schach von dort ist meist ein
//     Figurenopfer. Es zaehlen also nur Strahlenfelder ab Distanz 2 —
//     genau die "offenen Linien und Diagonalen", die man beim Blick aufs
//     Brett meint. Ein Koenig hinter intaktem Bauernschild hat davon 0–2,
//     ein Koenig mitten auf dem leeren Brett 19.
//   - Felder mit eigenen Steinen zaehlen nicht (der Strahl endet dort;
//     ein Schach von diesem Feld braeuchte erst einen Schlag).
//
// Eigene Steine in der 3x3-Koenigszone (Bauern UND Figuren, ohne den
// Koenig selbst) bringen eine kleine Gutschrift: sie koennen Schachs durch
// Dazwischenziehen parieren oder den Schachgeber schlagen.
//
//   exposure = max(0, strahlfelder * RAY_SQUARE_WEIGHT
//                     + springerfelder * KNIGHT_SQUARE_WEIGHT
//                     − verteidiger * DEFENDER_WEIGHT)
//
// Beispiele (Gegner hat Dame, Turm und Springer):
//   - Ke1 mit offener e-Linie (e3–e7 frei), Bauern d2/f2, Dd1, Lf1:
//     5 Strahlfelder * 8 = 40, kein Springerfeld frei = 0, 4 Verteidiger
//     * 4 = −16 → 24 cp; braucht der Gegner nur noch ein Schach, ×2,2 =
//     53 cp — die Suche zieht dann eine Koenigs-Umgruppierung einem halben
//     Bauern Materialgewinn vor.
//   - Kg1 rochiert hinter f2/g2/h2: 0–2 Strahlfelder → praktisch 0.
//   - Ke4 auf leerem Brett gegen eine Dame: 19 * 8 = 152 cp, ×2,2 = 334 cp
//     — ein solcher Koenig ist in Three-Check faktisch verloren.
// ---------------------------------------------------------------------------
/// Centipawns je freiem Strahlfeld (Distanz ≥ 2), von dem ein gegnerischer
/// Langschrittler den Koenig im Schach haette.
const RAY_SQUARE_WEIGHT: i32 = 8;
/// Centipawns je freiem Springer-Sprungfeld um den Koenig, wenn der Gegner
/// noch einen Springer hat. Kleiner als der Strahlwert: der Springer muss
/// das Feld erst erreichen, ein Langschrittler wirkt aus der Ferne.
const KNIGHT_SQUARE_WEIGHT: i32 = 4;
/// Gutschrift je eigenem Stein in der 3x3-Koenigszone (ohne Koenig).
const DEFENDER_WEIGHT: i32 = 4;

// ---------------------------------------------------------------------------
// Term 3: Verstaerkung der vorhandenen Angriffsdruck-Rechnung.
//
// `king_danger` in eval.rs summiert die Gewichte gegnerischer Offiziere,
// die die 3x3-Koenigszone bestreichen (ks_*_weight aus eval.toml), und
// schlaegt mit `n_attackers * weight_sum` in der `safety_table` nach. Fuer
// Standardschach ist das bewusst mild getunt (ein Angreifer kostet dort
// einstellige Centipawns). In Three-Check bedeutet "eine Figur bestreicht
// die Koenigszone" aber: sie ist ein Zug von einem Schach entfernt. Wir
// rechnen denselben Wert hier noch einmal aus und legen ihn mit
// KS_EXTRA_PERMILLE obendrauf — 1000 = der Druck zaehlt DOPPELT, mit
// Eskalation bis zu 3,2-fach.
//
// Warum eine Kopie statt eines Aufrufs? `king_danger` ist eine private
// Funktion der Standard-Eval; sie oeffentlich zu machen oder ihr einen
// Varianten-Schalter zu geben, wuerde eval.rs anfassen. Die Rechnung ist
// kurz, und so ist an dieser einen Stelle nachvollziehbar, was Three-Check
// zusaetzlich bestraft.
// ---------------------------------------------------------------------------
const KS_EXTRA_PERMILLE: i32 = 1000;

/// Eigene Steine in der 3x3-Zone um den Koenig (ohne den Koenig selbst).
#[inline]
fn zone_defenders<B: EngineBoard>(board: &B, us: Color) -> i32 {
    let king_sq = board.king_square(us);
    (get_king_moves(king_sq) & *board.color_combined(us)).popcnt() as i32
}

/// Term 2 (siehe oben), roh und noch ohne Eskalation. Sicht: positiver
/// Wert = so viel steht der Koenig von `us` offen (Malus fuer `us`).
fn king_exposure<B: EngineBoard>(board: &B, us: Color) -> i32 {
    let king_sq = board.king_square(us);
    let occ = *board.combined();
    let own = *board.color_combined(us);
    let enemy = *board.color_combined(!us);
    // Nachbarfelder + Koenigsfeld: zaehlen nie als Schachfeld.
    let near = get_king_moves(king_sq) | BitBoard::from_square(king_sq);
    let queens = *board.pieces(Piece::Queen) & enemy;

    let mut ray_squares = 0;
    if (*board.pieces(Piece::Bishop) & enemy) | queens != EMPTY {
        ray_squares += (get_bishop_moves(king_sq, occ) & !near & !own).popcnt() as i32;
    }
    if (*board.pieces(Piece::Rook) & enemy) | queens != EMPTY {
        ray_squares += (get_rook_moves(king_sq, occ) & !near & !own).popcnt() as i32;
    }
    let knight_squares = if *board.pieces(Piece::Knight) & enemy != EMPTY {
        (get_knight_moves(king_sq) & !own).popcnt() as i32
    } else {
        0
    };

    (ray_squares * RAY_SQUARE_WEIGHT + knight_squares * KNIGHT_SQUARE_WEIGHT
        - zone_defenders(board, us) * DEFENDER_WEIGHT)
        .max(0)
}

/// Term 3, roh: Angriffsdruck gegnerischer Offiziere auf die 3x3-Zone um
/// den Koenig von `us` — dieselbe Rechnung wie `king_danger` in eval.rs
/// (gleiche Gewichte und `safety_table` aus eval.toml). Positiver Wert =
/// Malus fuer `us`.
fn zone_pressure<B: EngineBoard>(board: &B, us: Color, p: &EvalParams) -> i32 {
    let king_sq = board.king_square(us);
    let zone = get_king_moves(king_sq) | BitBoard::from_square(king_sq);
    let enemy = *board.color_combined(!us);
    let occ = *board.combined();

    let mut n_attackers = 0;
    let mut weight_sum = 0;
    for sq in *board.pieces(Piece::Knight) & enemy {
        if get_knight_moves(sq) & zone != EMPTY {
            n_attackers += 1;
            weight_sum += p.ks_knight_weight;
        }
    }
    for sq in *board.pieces(Piece::Bishop) & enemy {
        if get_bishop_moves(sq, occ) & zone != EMPTY {
            n_attackers += 1;
            weight_sum += p.ks_bishop_weight;
        }
    }
    for sq in *board.pieces(Piece::Rook) & enemy {
        if get_rook_moves(sq, occ) & zone != EMPTY {
            n_attackers += 1;
            weight_sum += p.ks_rook_weight;
        }
    }
    for sq in *board.pieces(Piece::Queen) & enemy {
        if (get_rook_moves(sq, occ) | get_bishop_moves(sq, occ)) & zone != EMPTY {
            n_attackers += 1;
            weight_sum += p.ks_queen_weight;
        }
    }
    if n_attackers == 0 {
        return 0;
    }
    let max_idx = p.safety_table.len() as i32 - 1;
    if max_idx < 0 {
        return 0;
    }
    let idx = (n_attackers * weight_sum).clamp(0, max_idx) as usize;
    p.safety_table[idx]
}

/// Gesamtbeitrag einer Seite (Sicht dieser Seite, positiv = gut fuer sie):
/// Term 1 fuer die eigenen Schachs minus die eskalierten Koenigs-Terme 2
/// und 3, die der Gegner mit seinem Schach-Stand gegen uns ausspielt.
fn side_score<B: EngineBoard>(
    board: &B,
    p: &EvalParams,
    us: Color,
    our_remaining: u8,
    their_remaining: u8,
) -> i32 {
    // Schon gegebene Schachs (Index 0..=2; 3 = entschieden, s. `adjust`).
    let our_given = CHECKS_TO_WIN.saturating_sub(our_remaining).min(2) as usize;
    let their_given = CHECKS_TO_WIN.saturating_sub(their_remaining).min(2) as usize;

    let check_bonus = CHECK_GIVEN_BONUS[our_given];

    // Koenigslos kommt in Three-Check regulaer nicht vor (nur ueber
    // kuriose FENs) — dann gibt es auch keinen Koenig, den man exponieren
    // koennte.
    if !board.has_king(us) {
        return check_bonus;
    }

    let exposure = king_exposure(board, us);
    let extra_pressure = zone_pressure(board, us, p) * KS_EXTRA_PERMILLE / 1000;
    let king_terms = (exposure + extra_pressure) * ESCALATION_PERMILLE[their_given] / 1000;

    check_bonus - king_terms
}

/// Three-Check-Bewertung: `base` plus Differenz Weiss − Schwarz der drei
/// Zusatzterme (siehe Modul-Doku). `phase` wird bewusst nicht genutzt:
/// Schachs sind in dieser Variante im Endspiel genauso entscheidend wie
/// im Mittelspiel (K+T gegen K ist mit zwei gegebenen Schachs sofort aus),
/// die Terme skalieren deshalb ueber die gegnerischen Angreifer selbst
/// (Strahl-/Springerfelder zaehlen nur, wenn der Gegner die passende
/// Figur noch hat; `zone_pressure` zaehlt konkrete Angreifer) statt ueber
/// die Spielphase.
#[inline]
pub fn adjust<B: EngineBoard>(board: &B, p: &EvalParams, _phase: i32, base: i32) -> i32 {
    // Ohne Schachzaehlung (sollte fuer dieses Backend nie passieren) bleibt
    // die generische Bewertung stehen.
    let (Some(w_rem), Some(b_rem)) = (
        board.checks_remaining(Color::White),
        board.checks_remaining(Color::Black),
    ) else {
        return base;
    };

    // Drittes Schach ist gefallen: Partie entschieden. Beide Zaehler
    // gleichzeitig auf 0 ist regelwidrig; Weiss wird dann bevorzugt
    // geprueft — irrelevant fuer die Suche, die diese Stellungen nie
    // bewertet (leere Zugliste → Terminal-Score).
    if w_rem == 0 {
        return DECIDED;
    }
    if b_rem == 0 {
        return -DECIDED;
    }

    let white = side_score(board, p, Color::White, w_rem, b_rem);
    let black = side_score(board, p, Color::Black, b_rem, w_rem);
    base + white - black
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board_shak::BoardThreeCheck;
    use crate::eval::{evaluate, game_phase};
    use crate::polyglot::BookSet;
    use crate::search::{search, GoParams, SearchRequest};
    use crate::tt::TranspositionTable;
    use std::path::Path;
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};

    const START_3CHECK: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 3+3 0 1";

    fn board(fen: &str) -> BoardThreeCheck {
        BoardThreeCheck::from_fen(fen).unwrap_or_else(|e| panic!("FEN {} ungueltig: {}", fen, e))
    }

    /// Bewertung aus Sicht von Weiss mit den Default-Parametern.
    fn eval_white(fen: &str) -> i32 {
        evaluate(&board(fen), &EvalParams::default())
    }

    /// Nur der Varianten-Anteil (Total minus generische Bewertung).
    fn variant_part(fen: &str) -> i32 {
        let b = board(fen);
        let p = EvalParams::default();
        adjust(&b, &p, game_phase(&b), 0)
    }

    /// Dieselbe Stellung mit anderem Schach-Stand (Lichess-Feld "w+b" =
    /// verbleibende Schachs Weiss+Schwarz).
    fn with_checks(fen: &str, remaining: &str) -> String {
        let mut parts: Vec<&str> = fen.split_whitespace().collect();
        assert_eq!(parts.len(), 7, "FEN mit Three-Check-Feld erwartet: {}", fen);
        parts[4] = remaining;
        parts.join(" ")
    }

    #[test]
    fn startpos_fen_parses_with_three_remaining_checks() {
        let b = board(START_3CHECK);
        assert_eq!(b.checks_remaining(Color::White), Some(3));
        assert_eq!(b.checks_remaining(Color::Black), Some(3));
        // Ausgeglichene Startstellung: der Varianten-Anteil ist symmetrisch,
        // also exakt 0 — die Zusatzterme aendern die Startbewertung nicht.
        assert_eq!(variant_part(START_3CHECK), 0);
        assert_eq!(eval_white(START_3CHECK), eval_white(&with_checks(START_3CHECK, "+0+0")));
    }

    #[test]
    fn giving_a_check_lowers_remaining_count() {
        // 1.e4 e5 2.Dh5 Sc6?? 3.Dxf7+ — erstes Schach von Weiss.
        let b = board("r1bqkbnr/pppp1ppp/2n5/4p2Q/4P3/8/PPPP1PPP/RNB1KBNR w KQkq - 3+3 2 3");
        let after = b.make_move_new(b.parse_uci_move("h5f7").unwrap());
        assert_eq!(after.checks_remaining(Color::White), Some(2));
        assert_eq!(after.checks_remaining(Color::Black), Some(3));
        assert!(after.checkers() != &EMPTY);
        assert!(!after.is_variant_loss(), "erstes Schach beendet die Partie nicht");
    }

    #[test]
    fn eval_prefers_fewer_remaining_checks_for_the_opponent_and_escalates() {
        // Mittelspielstellung, gleiche Figuren — nur der Schach-Stand
        // variiert. Sicht von Weiss: je weniger Schachs WEISS noch geben
        // muss, desto besser; je weniger SCHWARZ noch geben muss, desto
        // schlechter.
        let fen = "r1bq1rk1/pppp1ppp/2n2n2/2b1p3/2B1P3/2N2N2/PPPP1PPP/R1BQ1RK1 w - - 3+3 6 6";
        let e33 = eval_white(fen);
        let e23 = eval_white(&with_checks(fen, "2+3")); // Weiss hat 1 Schach gegeben
        let e13 = eval_white(&with_checks(fen, "1+3")); // Weiss hat 2 Schachs gegeben
        let e32 = eval_white(&with_checks(fen, "3+2")); // Schwarz hat 1 Schach gegeben
        let e31 = eval_white(&with_checks(fen, "3+1")); // Schwarz hat 2 Schachs gegeben

        assert!(e23 > e33, "ein gegebenes Schach muss Weiss besser stellen");
        assert!(e13 > e23, "zwei gegebene Schachs muessen besser sein als eins");
        assert!(e32 < e33, "ein gegnerisches Schach muss Weiss schlechter stellen");
        assert!(e31 < e32, "zwei gegnerische Schachs muessen schlechter sein als eins");

        // Eskalation: der Sprung 1→2 gegebene Schachs ist groesser als 0→1.
        assert!(e13 - e23 > e23 - e33, "zweites Schach muss mehr wert sein als das erste");
        assert!(e32 - e31 > e33 - e32, "zweites gegnerisches Schach muss schwerer wiegen");

        // Groessenordnung: ein Schach ~ ein guter Bauer, zwei ~ ein Turm,
        // aber nie in Matt-Naehe.
        let one = e23 - e33;
        let two = e13 - e33;
        assert!((80..=250).contains(&one), "erstes Schach = {} cp", one);
        assert!((250..=700).contains(&two), "zwei Schachs = {} cp", two);
    }

    #[test]
    fn eval_is_color_symmetric() {
        // Gespiegelte Stellung mit vertauschten Zaehlern muss das
        // Vorzeichen wechseln (kein Weiss-Bias in den Zusatztermen).
        let white_view = "r1bq1rk1/pppp1ppp/2n2n2/2b1p3/2B1P3/2N2N2/PPPP1PPP/R1BQ1RK1 w - - 1+3 6 6";
        let black_view = "r1bq1rk1/pppp1ppp/2n2n2/2b1p3/2B1P3/2N2N2/PPPP1PPP/R1BQ1RK1 b - - 3+1 6 6";
        assert_eq!(variant_part(white_view), -variant_part(black_view));
    }

    #[test]
    fn exposed_king_costs_more_than_castled_king_and_escalates() {
        let p = EvalParams::default();
        // Rochierter weisser Koenig hinter intaktem Bauernschild ...
        let safe = board("4k3/8/8/8/8/8/5PPP/3q2K1 w - - 3+3 0 1");
        // ... gegen einen Koenig mitten auf dem Brett ohne Deckung.
        let open = board("4k3/8/8/8/4K3/8/8/3q4 w - - 3+3 0 1");
        assert!(king_exposure(&open, Color::White) > king_exposure(&safe, Color::White));
        // 19 Strahlfelder ab Distanz 2 (27 Damenfelder minus 8 Nachbarn).
        assert_eq!(king_exposure(&open, Color::White), 19 * RAY_SQUARE_WEIGHT);
        // Rochiert: nur e1/d1 auf der Grundreihe sind Strahlfelder, drei
        // Schildbauern als Verteidiger → fast nichts.
        assert!(king_exposure(&safe, Color::White) <= 2 * RAY_SQUARE_WEIGHT);

        // Eskalation: derselbe offene Koenig ist teurer, wenn Schwarz nur
        // noch ein Schach braucht (Sicht von Weiss, nur Weiss' Anteil —
        // der schwarze Koenig steht auf dem leeren Brett ja auch offen).
        let calm = side_score(&open, &p, Color::White, 3, 3);
        let urgent = side_score(&open, &p, Color::White, 3, 1);
        assert!(calm < 0);
        assert!(urgent < calm, "{} vs {}", urgent, calm);
        let raw = -calm; // Exposition + Zusatzdruck, unskaliert
        assert_eq!(urgent, -(raw * ESCALATION_PERMILLE[2] / 1000), "Faktor 2,2 erwartet");
        // Der Schach-Bonus haengt nur am eigenen Zaehler, nicht am
        // gegnerischen.
        assert_eq!(
            side_score(&open, &p, Color::White, 1, 3) - calm,
            CHECK_GIVEN_BONUS[2]
        );

        // Ohne gegnerische Langschrittler zaehlen offene Strahlen nichts —
        // nur die acht Springer-Sprungfelder, weil Schwarz einen Springer hat.
        let knight_only = board("4k3/8/8/8/4K3/8/8/3n4 w - - 3+3 0 1");
        assert_eq!(king_exposure(&knight_only, Color::White), 8 * KNIGHT_SQUARE_WEIGHT);
        // Nur ein Turm: Diagonalen zaehlen nicht, Linien/Reihen schon
        // (e-Linie 5 + 4. Reihe 5 = 10 Felder ab Distanz 2; d1-Turm steht
        // nicht auf der e-Linie).
        let rook_only = board("4k3/8/8/8/4K3/8/8/3r4 w - - 3+3 0 1");
        assert_eq!(king_exposure(&rook_only, Color::White), 10 * RAY_SQUARE_WEIGHT);
        // Offene e-Linie vor dem Koenig e1 gegen Dame: 5 Felder e3–e7,
        // vier Verteidiger (Dd1, d2, f2, Lf1) → 40 − 16 = 24 cp.
        let e_file = board("q3k3/4p3/8/8/8/8/3P1P2/3QKB2 w - - 3+3 0 1");
        assert_eq!(
            king_exposure(&e_file, Color::White),
            5 * RAY_SQUARE_WEIGHT - 4 * DEFENDER_WEIGHT
        );
    }

    #[test]
    fn extra_pressure_uses_eval_toml_weights() {
        let p = EvalParams::default();
        // Schwarze Dame bestreicht die Koenigszone (g1): ein Angreifer mit
        // Gewicht ks_queen_weight → safety_table[1 * 5].
        let b = board("4k3/8/8/8/8/8/5PPP/3q2K1 w - - 3+3 0 1");
        let expected = p.safety_table[(p.ks_queen_weight as usize).min(p.safety_table.len() - 1)];
        assert_eq!(zone_pressure(&b, Color::White, &p), expected);
        // Keine Angreifer → 0.
        let quiet = board("4k3/8/8/8/8/8/5PPP/6K1 w - - 3+3 0 1");
        assert_eq!(zone_pressure(&quiet, Color::White, &p), 0);
    }

    #[test]
    fn decided_position_gets_decisive_static_score() {
        // Weiss hat sein drittes Schach gegeben (Zaehler 0): entschieden,
        // aber unterhalb der Matt-Schwelle der Suche.
        let won = board("rnb1kbnr/pppp1Qpp/8/4p3/4P3/8/PPPP1PPP/RNB1KBNR b KQkq - 0+3 0 3");
        assert!(won.is_variant_loss());
        assert_eq!(eval_white("rnb1kbnr/pppp1Qpp/8/4p3/4P3/8/PPPP1PPP/RNB1KBNR b KQkq - 0+3 0 3"), DECIDED);
        const { assert!(DECIDED < 99_000) };
        // Spiegelbild fuer Schwarz.
        assert_eq!(
            eval_white("rnb1kbnr/pppp1Qpp/8/4p3/4P3/8/PPPP1PPP/RNB1KBNR b KQkq - 3+0 0 3"),
            -DECIDED
        );
    }

    #[test]
    fn hash_differs_for_different_remaining_checks() {
        let a = board(START_3CHECK);
        let b = board(&with_checks(START_3CHECK, "2+3"));
        let c = board(&with_checks(START_3CHECK, "3+2"));
        assert_ne!(a.get_hash(), b.get_hash());
        assert_ne!(a.get_hash(), c.get_hash());
        assert_ne!(b.get_hash(), c.get_hash());
        // Und der Zobrist-Hash aendert sich auch durch ein tatsaechlich
        // gegebenes Schach — nicht nur durch das FEN-Feld.
        let pre = board("r1bqkbnr/pppp1ppp/2n5/4p2Q/4P3/8/PPPP1PPP/RNB1KBNR w KQkq - 3+3 2 3");
        let after = pre.make_move_new(pre.parse_uci_move("h5f7").unwrap());
        let same_squares =
            board("r1bqkbnr/pppp1Qpp/2n5/4p3/4P3/8/PPPP1PPP/RNB1KBNR b KQkq - 3+3 0 3");
        assert_ne!(after.get_hash(), same_squares.get_hash());
        assert_eq!(after.to_fen(), "r1bqkbnr/pppp1Qpp/2n5/4p3/4P3/8/PPPP1PPP/RNB1KBNR b KQkq - 2+3 0 3");
    }

    fn run_search(b: BoardThreeCheck, depth: u32) -> crate::search::SearchResult {
        let req = SearchRequest {
            history: vec![b.get_hash()],
            board: b,
            halfmove_clock: 0,
            params: GoParams {
                depth: Some(depth),
                movetime: Some(10_000),
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
        search(req).expect("Three-Check-Suche liefert einen Zug")
    }

    #[test]
    fn search_plays_the_third_check_to_win() {
        // Weiss braucht noch EIN Schach: Dxf7+ oder Dxe5+ gewinnen sofort
        // (Gewinn in 1) — obwohl die Dame auf f7 danach vom Koenig
        // geschlagen werden koennte; das zaehlt hier nicht mehr.
        let b = board("rnb1kbnr/pppp1ppp/8/4p2Q/4P3/8/PPPP1PPP/RNB1KBNR w KQkq - 1+3 0 3");
        let result = run_search(b.clone(), 2);
        let after = b.make_move_new(result.best);
        assert!(
            after.is_variant_loss(),
            "Zug {} ist kein sofort gewinnendes Schach",
            crate::position::move_to_uci(result.best)
        );
        assert_eq!(after.checks_remaining(Color::White), Some(0));
        assert!(result.score > 99_000, "Matt-Score erwartet, war {}", result.score);
        // Und die Wurzel meldet Matt in 1 — ein reiner Materialzug wie
        // Dxe5 ohne Schach waere hier keine Alternative.
        let alternatives = ["h5f7", "h5e5"];
        assert!(alternatives.contains(&crate::position::move_to_uci(result.best).as_str()));
    }

    #[test]
    fn search_avoids_allowing_the_third_check() {
        // Schwarz braucht noch EIN Schach und droht ...Dxf2+ (f2 ist nur
        // vom Koenig gedeckt — fuer Three-Check egal, das Schach zaehlt).
        // Einzige Verteidigung ist g2-g3: sie schliesst die Diagonale
        // h4-e1. Sf3 greift die Dame zwar an, laesst Dxf2+ aber zu; jeder
        // andere Zug ebenso. Die Suche muss also g2g3 waehlen — bzw. in
        // jedem Fall einen Zug, nach dem kein schwarzer Zug die Partie
        // sofort beendet.
        let b = board("rnb1kbnr/pppp1ppp/8/4p3/7q/8/PPPPPPPP/RNBQKBNR w KQkq - 3+1 0 3");
        let result = run_search(b.clone(), 3);
        let after = b.make_move_new(result.best);
        let loses_immediately = after
            .legal_gen()
            .any(|reply| after.make_move_new(reply).is_variant_loss());
        assert!(
            !loses_immediately,
            "Zug {} laesst das dritte Schach zu",
            crate::position::move_to_uci(result.best)
        );
        assert_eq!(result.best, b.parse_uci_move("g2g3").unwrap());
    }
}
