//! King of the Hill — varianten-spezifische Bewertung.
//!
//! Regeln: Standardschach plus ein zweiter Gewinnweg — wer seinen Koenig
//! auf eines der vier Huegelfelder d4/e4/d5/e5 stellt, hat sofort
//! gewonnen. Matt zaehlt weiterhin. Die Zuggenerierung (shakmaty) und die
//! Terminal-Erkennung (`is_variant_win`/`is_variant_loss`, leere Zugliste)
//! bilden das exakt ab; die Suche sieht den Huegelsieg damit wie ein Matt.
//!
//! Was die generische Bewertung (`base`) NICHT weiss: dass der Koenig hier
//! nicht nur ein zu schuetzendes, sondern auch ein ANGREIFENDES Stueck ist.
//! Ein Koenig, der bis auf ein Feld an den Huegel herangekommen ist, ist
//! eine Drohung wie eine Mattdrohung — der Gegner muss die Einstiegsfelder
//! decken, sonst ist die Partie vorbei. `base` bleibt vollstaendig erhalten
//! (Material, Bauernstruktur, Koenigssicherheit usw. gelten unveraendert),
//! und dieses Modul legt vier Terme obendrauf, jeweils fuer beide Seiten
//! berechnet und als Differenz Weiss − Schwarz addiert:
//!
//!   1. Naehe zum Huegel (Chebyshev-Distanz des Koenigs zum naechsten
//!      Huegelfeld), gewichtet nach der RESTARMEE DES GEGNERS — nicht nach
//!      der globalen Spielphase. Denn ob ein Koenigsmarsch gefaehrlich ist,
//!      haengt davon ab, wer ihn angreifen kann: hat der Gegner noch Dame
//!      und Tuerme, ist ein Koenig im Zentrum Kanonenfutter; hat er nur
//!      noch Bauern, entscheidet der Marsch die Partie.
//!   2. Ungedeckte Huegelfelder — Felder, die der Gegner nicht angreift.
//!      Je naeher der eigene Koenig steht, desto mehr sind sie wert.
//!      Spiegelbildlich ist das der "Malus, wenn der gegnerische Koenig
//!      einen freien Weg ins Zentrum hat": seine ungedeckten Felder
//!      zaehlen fuer ihn, also gegen uns.
//!   3. Einstiegsdrohung — der Koenig steht direkt neben einem Huegelfeld,
//!      das er betreten DARF (nicht angegriffen, nicht von eigener Figur
//!      belegt). Ist die Seite NICHT am Zug, muss der Gegner jetzt sofort
//!      parieren: fester Bonus, unabhaengig von der Phase.
//!   4. Sieg im naechsten Zug — dieselbe Einstiegsdrohung, aber die Seite
//!      IST am Zug: der Koenigszug auf den Huegel ist legal und gewinnt
//!      sofort. Die Hauptsuche findet das ohnehin (Tiefe 1), aber die
//!      Quiescence sucht nur Schlagzuege und wuerde den stillen
//!      Gewinnzug uebersehen; ihr Stand-Pat bekommt deshalb einen
//!      "entschieden"-Wert, der jede Materialdifferenz ueberragt, aber
//!      unter der Mate-Schwelle der Suche bleibt (kein Ply-Adjustment).
//!
//! Alle Werte in Centipawns (Skala wie die Standard-Eval: Bauer 100,
//! Leichtfigur 300, Turm 500, Dame 900), Rueckgabe aus Sicht von Weiss.
//! Signatur und Konvention siehe `crate::variants` (Modul-Doku).

use crate::backend::EngineBoard;
use crate::endgame::chebyshev;
use crate::eval::taper;
use crate::eval_config::EvalParams;
use chess::{
    get_bishop_moves, get_king_moves, get_knight_moves, get_pawn_attacks, get_rook_moves,
    BitBoard, Color, Piece, Square, EMPTY,
};

// ---------------------------------------------------------------------------
// Die vier Huegelfelder als Bitboard. Bit-Index = Feldindex der chess-Crate
// (a1 = 0, h8 = 63): d4 = 27, e4 = 28, d5 = 35, e5 = 36.
// ---------------------------------------------------------------------------
const HILL: BitBoard = BitBoard((1u64 << 27) | (1u64 << 28) | (1u64 << 35) | (1u64 << 36));
const HILL_SQUARES: [Square; 4] = [Square::D4, Square::E4, Square::D5, Square::E5];

// ---------------------------------------------------------------------------
// Term 1: Naehe zum Huegel.
//
// Index = Chebyshev-Distanz des Koenigs zum naechsten Huegelfeld. Auf dem
// 8x8-Brett ist sie hoechstens 3 (Ecke a1 → d4: drei Koenigsschritte),
// Distanz 0 = Koenig steht auf dem Huegel = Partie entschieden (kommt in
// der Suche nie zur Bewertung, weil die Zugliste dann leer ist; fuer das
// UCI-Kommando `eval` und statische Analysen steht trotzdem ein
// eindeutiger Wert da, siehe HILL_REACHED).
//
// Zwei Tabellen, zwischen denen nach der gegnerischen Restarmee
// interpoliert wird (siehe `enemy_army_phase`):
//   MG (Gegner hat volle Armee): der Koenig soll nicht auf Verdacht
//       losmarschieren — ein kleiner Anreiz, damit die Suche das Motiv
//       "Koenig zum Zentrum" nicht voellig ignoriert, aber deutlich
//       kleiner als ein Bauer. Die Koenigssicherheits-Terme in `base`
//       halten dagegen.
//   EG (Gegner hat keine Figuren mehr): der Marsch ist der Plan. Eine
//       Distanz-Stufe ist hier fast einen halben Bauern wert, Distanz 1
//       fast eine Leichtfigur — in Bauernendspielen entscheidet das.
// ---------------------------------------------------------------------------
const HILL_DIST_MG: [i32; 4] = [0, 20, 8, 0];
const HILL_DIST_EG: [i32; 4] = [0, 90, 45, 15];

/// Koenig steht bereits auf dem Huegel (nur ausserhalb der Suche
/// erreichbar, s. o.): klar entschieden, oberhalb jeder Materialsumme.
const HILL_REACHED: i32 = 10_000;

// ---------------------------------------------------------------------------
// Term 2: Ungedeckte Huegelfelder (Felder, die der Gegner NICHT angreift),
// pro Feld, skaliert mit der Koenigsdistanz (Index wie oben).
//
// Ein ungedecktes Huegelfeld ist nur dann eine Drohung, wenn der Koenig
// es auch bald erreichen kann. Bei Distanz 3 (Grundreihe) ist es fast
// egal, bei Distanz 1 fast schon Term 3. Vier ungedeckte Felder bei
// Distanz 2 = 48 cp: der Gegner hat das Zentrum aufgegeben, das ist etwa
// ein halber Bauer wert — und zwingt ihn, Figuren zur Deckung
// abzustellen statt anzugreifen.
// ---------------------------------------------------------------------------
const UNGUARDED_HILL_PER_SQUARE: [i32; 4] = [0, 30, 12, 4];

// ---------------------------------------------------------------------------
// Term 3: Einstiegsdrohung der Seite, die NICHT am Zug ist.
//
// Der Koenig steht neben einem betretbaren Huegelfeld; der Gegner ist am
// Zug und MUSS das parieren (Feld angreifen oder — selten — selbst sofort
// gewinnen/mattsetzen). Ein Bauer Bonus fuer das erste, ein halber fuer
// jedes weitere Einstiegsfeld: zwei Felder gleichzeitig zu decken ist
// deutlich schwerer als eines. Bewusst NICHT phasenabhaengig — die Drohung
// ist konkret, egal wie viel Material auf dem Brett steht.
// ---------------------------------------------------------------------------
const ENTRY_THREAT_FIRST: i32 = 100;
const ENTRY_THREAT_EXTRA: i32 = 50;

// ---------------------------------------------------------------------------
// Term 4: Sieg im naechsten Zug (Seite am Zug hat ein betretbares
// Huegelfeld neben dem Koenig).
//
// Groessenordnung: klar oberhalb jeder realistischen Materialdifferenz
// (Dame + zwei Tuerme + Rest ≈ 4000), aber weit unter der Mate-Schwelle
// der Suche (99 000), damit die Suche den Wert wie eine normale Bewertung
// behandelt (kein Ply-Adjustment in der TT) und ein ECHTES Matt bzw. ein
// gefundener Huegelsieg (MATE − ply) immer noch hoeher ist.
// ---------------------------------------------------------------------------
const WIN_NEXT_MOVE: i32 = 5_000;

/// Ergaenzt die generische Bewertung um die vier KotH-Terme (s. Modul-Doku).
/// `p` wird nicht gebraucht (die Konstanten leben hier im Modul); `phase`
/// ebenfalls nicht — die Phasengewichtung laeuft ueber die gegnerische
/// Restarmee, siehe `enemy_army_phase`.
#[inline]
pub fn adjust<B: EngineBoard>(board: &B, _p: &EvalParams, _phase: i32, base: i32) -> i32 {
    base + side_score(board, Color::White) - side_score(board, Color::Black)
}

/// KotH-Zusatzbewertung EINER Seite aus deren Sicht (hoeher = besser fuer
/// `us`). Die Differenz beider Seiten bildet `adjust`.
fn side_score<B: EngineBoard>(board: &B, us: Color) -> i32 {
    // Regelkonforme KotH-Stellungen haben immer beide Koenige; der Guard
    // ist die Konvention aller Varianten-Module (Platzhalter-Feld nie
    // bewerten).
    if !board.has_king(us) {
        return 0;
    }
    let king = board.king_square(us);
    let dist = hill_distance(king);
    if dist == 0 {
        return HILL_REACHED;
    }

    // Angriffskarte des Gegners OHNE unseren Koenig in der Belegung: ein
    // Koenig kann nicht "in seinem eigenen Schatten" ziehen — steht er auf
    // c3 und ein Laeufer auf b2, ist d4 zwar aktuell "verstellt", nach Kd4
    // aber angegriffen. Ohne diesen Kniff waere Term 3/4 nicht exakt.
    let occ_without_king = *board.combined() & !BitBoard::from_square(king);
    let enemy_attacks = attack_map(board, !us, occ_without_king);

    // Term 1: Naehe, gewichtet nach der gegnerischen Restarmee.
    let mut score = taper(
        HILL_DIST_MG[dist],
        HILL_DIST_EG[dist],
        enemy_army_phase(board, !us),
    );

    // Term 2: ungedeckte Huegelfelder.
    let unguarded = (HILL & !enemy_attacks).popcnt() as i32;
    score += unguarded * UNGUARDED_HILL_PER_SQUARE[dist];

    // Terme 3/4: betretbare Huegelfelder direkt neben dem Koenig. Eigene
    // Figuren blockieren (muessten erst wegziehen), gegnerische nicht (der
    // Koenig schlaegt sie und steht dann auf dem Huegel — sofern das Feld
    // nicht gedeckt ist, was `enemy_attacks` schon abdeckt).
    let ours = *board.color_combined(us);
    let entries = (get_king_moves(king) & HILL & !enemy_attacks & !ours).popcnt() as i32;
    if entries > 0 {
        if board.side_to_move() == us {
            score += WIN_NEXT_MOVE;
        } else {
            score += ENTRY_THREAT_FIRST + (entries - 1) * ENTRY_THREAT_EXTRA;
        }
    }

    score
}

/// Chebyshev-Distanz (Koenigsschritte) von `sq` zum naechsten Huegelfeld,
/// 0..=3.
fn hill_distance(sq: Square) -> usize {
    HILL_SQUARES
        .iter()
        .map(|&h| chebyshev(sq, h))
        .min()
        .expect("HILL_SQUARES ist nie leer") as usize
}

/// "Phase" der Restarmee von `side` in derselben Skala wie `game_phase`
/// (0..=24), aber nur EINE Seite gezaehlt: Springer/Laeufer 1, Turm 2,
/// Dame 4 → eine volle Armee ergibt 12, verdoppelt 24. Bauern zaehlen
/// nicht — sie koennen einen marschierenden Koenig kaum bedrohen.
/// 24 = Gegner hat alles (MG-Tabelle), 0 = Gegner hat keine Figur mehr
/// (EG-Tabelle); dazwischen linear (`taper`).
fn enemy_army_phase<B: EngineBoard>(board: &B, side: Color) -> i32 {
    let theirs = *board.color_combined(side);
    let count = |piece: Piece| (*board.pieces(piece) & theirs).popcnt() as i32;
    let army = count(Piece::Knight) + count(Piece::Bishop)
        + 2 * count(Piece::Rook)
        + 4 * count(Piece::Queen);
    (army.min(12)) * 2
}

/// Alle Felder, die `side` mit der Belegung `occ` angreift (Bauern beide
/// Schlagfelder, Gleiter bis zum ersten Stein in `occ`, Koenig seine acht
/// Nachbarfelder). Der Koenig wird ueber das Bitboard gelesen — fehlt er
/// (in KotH nie, aber die Konvention gilt), ist die Schleife einfach leer.
fn attack_map<B: EngineBoard>(board: &B, side: Color, occ: BitBoard) -> BitBoard {
    let theirs = *board.color_combined(side);
    let mut attacks = EMPTY;
    for sq in *board.pieces(Piece::Pawn) & theirs {
        attacks |= get_pawn_attacks(sq, side, !EMPTY);
    }
    for sq in *board.pieces(Piece::Knight) & theirs {
        attacks |= get_knight_moves(sq);
    }
    for sq in (*board.pieces(Piece::Bishop) | *board.pieces(Piece::Queen)) & theirs {
        attacks |= get_bishop_moves(sq, occ);
    }
    for sq in (*board.pieces(Piece::Rook) | *board.pieces(Piece::Queen)) & theirs {
        attacks |= get_rook_moves(sq, occ);
    }
    for sq in *board.pieces(Piece::King) & theirs {
        attacks |= get_king_moves(sq);
    }
    attacks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board_shak::{BoardKingOfTheHill, ShakVariant};
    use crate::eval::evaluate;
    use crate::polyglot::BookSet;
    use crate::search::{search, GoParams, SearchRequest, SearchResult};
    use crate::tt::TranspositionTable;
    use chess::{BoardStatus, ChessMove};
    use shakmaty::fen::Fen;
    use shakmaty::CastlingMode;
    use std::path::Path;
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};

    /// Mate-Codierung der Suche: MATE = 100_000, Schwelle MATE - 1000
    /// (private Konstanten in `search.rs`, hier gespiegelt).
    const MATE_SCORE_MIN: i32 = 99_000;

    fn board(fen: &str) -> BoardKingOfTheHill {
        BoardKingOfTheHill::from_fen(fen).unwrap_or_else(|e| panic!("FEN {}: {}", fen, e))
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

    fn perft(b: &BoardKingOfTheHill, depth: u32) -> u64 {
        if depth == 0 {
            return 1;
        }
        b.legal_gen()
            .map(|mv| perft(&b.make_move_new(mv), depth - 1))
            .sum()
    }

    // --- Regeln / Adapter --------------------------------------------------

    #[test]
    fn king_on_hill_is_terminal_win_or_loss() {
        // Weisser Koenig auf d4, Weiss am Zug: Weiss hat bereits gewonnen.
        let won = board("4k3/8/8/8/3K4/8/8/8 w - - 0 1");
        assert!(won.is_variant_win());
        assert!(!won.is_variant_loss());
        assert_eq!(won.legal_gen().count(), 0);
        assert_eq!(won.status(), BoardStatus::Checkmate);
        // Dieselbe Stellung mit Schwarz am Zug: Schwarz hat verloren.
        let lost = board("4k3/8/8/8/3K4/8/8/8 b - - 0 1");
        assert!(lost.is_variant_loss());
        assert!(!lost.is_variant_win());
        assert_eq!(lost.legal_gen().count(), 0);
        assert_eq!(lost.status(), BoardStatus::Checkmate);
        // Alle vier Huegelfelder zaehlen, auch fuer Schwarz.
        for sq in ["d4", "e4", "d5", "e5"] {
            let fen = match sq {
                "d4" => "4K3/8/8/8/3k4/8/8/8 w - - 0 1",
                "e4" => "4K3/8/8/8/4k3/8/8/8 w - - 0 1",
                "d5" => "4K3/8/8/3k4/8/8/8/8 w - - 0 1",
                _ => "4K3/8/8/4k3/8/8/8/8 w - - 0 1",
            };
            assert!(board(fen).is_variant_loss(), "schwarzer Koenig auf {}", sq);
        }
        // Und der Zug dorthin macht die Stellung terminal.
        let before = board("4k3/8/8/8/8/3K4/8/8 w - - 0 1");
        let after = before.make_move_new(before.parse_uci_move("d3d4").unwrap());
        assert!(after.is_variant_loss());
        assert_eq!(after.status(), BoardStatus::Checkmate);
    }

    #[test]
    fn perft_with_king_next_to_hill_matches_shakmaty() {
        // Weisser Koenig auf d3: in der Tiefe entstehen Huegelsiege (Kd4/Ke4),
        // die als Blaetter mit leerer Zugliste zaehlen — genau wie bei
        // shakmatys Referenz-Perft. Schwarzer Turm deckt d4/e4 nicht.
        let fen = "r3k3/8/8/8/8/3K4/8/8 w - - 0 1";
        let b = board(fen);
        let reference: shakmaty::variant::KingOfTheHill = Fen::from_ascii(fen.as_bytes())
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
        // Koenig bereits auf d4 → Partie vorbei, kein einziger Zug mehr.
        assert_eq!(perft(&board("r3k3/8/8/8/3K4/8/8/8 b - - 0 1"), 1), 0);
        assert_eq!(
            shakmaty::variant::KingOfTheHill::KIND,
            crate::backend::VariantKind::KingOfTheHill
        );
    }

    // --- Bewertung ---------------------------------------------------------

    #[test]
    fn hill_distance_geometry() {
        assert_eq!(hill_distance(Square::D4), 0);
        assert_eq!(hill_distance(Square::E5), 0);
        assert_eq!(hill_distance(Square::D3), 1);
        assert_eq!(hill_distance(Square::F6), 1);
        assert_eq!(hill_distance(Square::E2), 2);
        assert_eq!(hill_distance(Square::E1), 3);
        assert_eq!(hill_distance(Square::A1), 3);
        assert_eq!(hill_distance(Square::H8), 3);
    }

    #[test]
    fn startpos_is_symmetric_and_unchanged_in_sign() {
        // Beide Koenige auf der Grundreihe, beide Lager komplett: die
        // KotH-Terme sind spiegelgleich und heben sich auf. Die generische
        // Eval der Startstellung ist ebenfalls symmetrisch → 0.
        let start = BoardKingOfTheHill::startpos();
        assert_eq!(side_score(&start, Color::White), side_score(&start, Color::Black));
        assert_eq!(evaluate(&start, &EvalParams::default()), 0);
        // Restarmee-Phase: volle Armee = 24 (wie game_phase-Skala).
        assert_eq!(enemy_army_phase(&start, Color::Black), 24);
        assert_eq!(enemy_army_phase(&board("4k3/8/8/8/8/8/8/4K3 w - - 0 1"), Color::Black), 0);
    }

    #[test]
    fn closer_king_scores_better_all_else_equal() {
        // Reines Koenigsendspiel, Schwarz am Zug (damit Term 4 nicht
        // greift): weisser Koenig e1 (Distanz 3) < e2 (Distanz 2) < e3
        // (Distanz 1, Einstiegsdrohung). Schwarzer Koenig jeweils e8.
        let d3 = eval("4k3/8/8/8/8/8/8/4K3 b - - 0 1");
        let d2 = eval("4k3/8/8/8/8/8/4K3/8 b - - 0 1");
        let d1 = eval("4k3/8/8/8/8/4K3/8/8 b - - 0 1");
        assert!(d2 > d3, "e2 {} sollte besser sein als e1 {}", d2, d3);
        assert!(d1 > d2, "e3 {} sollte besser sein als e2 {}", d1, d2);
        // Die Einstiegsdrohung (Koenig e3, d4 UND e4 betretbar, Schwarz am
        // Zug) ist mindestens einen Bauern plus Zuschlag wert.
        assert!(d1 - d2 >= ENTRY_THREAT_FIRST + ENTRY_THREAT_EXTRA, "d1 {} d2 {}", d1, d2);
        // Farbspiegelbild der e2-Stellung (schwarzer Koenig e7, weisser
        // e1, Weiss am Zug): gleicher Betrag, anderes Vorzeichen.
        let mirror = eval("8/4k3/8/8/8/8/8/4K3 w - - 0 1");
        assert_eq!(mirror, -d2);
    }

    #[test]
    fn side_to_move_with_free_entry_is_decided() {
        // Weiss am Zug, Koenig e3, d4/e4 frei und ungedeckt: Term 4.
        let s = eval("4k3/8/8/8/8/4K3/8/8 w - - 0 1");
        assert!(s >= WIN_NEXT_MOVE, "score {}", s);
        // Deckt Schwarz beide Felder (Turm d8 → d4, Turm e8 → e4... e8 ist
        // der Koenig, also Laeufer h7 → e4), bleibt nur die Naehe uebrig.
        let covered = eval("3rk3/7b/8/8/8/4K3/8/8 w - - 0 1");
        assert!(covered < WIN_NEXT_MOVE, "score {}", covered);
        // X-Ray-Fall: Laeufer b2 greift c3 (Koenig) an; d4 liegt im
        // Schatten des Koenigs. Kd4 waere illegal (Koenig zieht auf der
        // Angriffslinie) → kein Sieg-im-naechsten-Zug.
        let xray = board("4k3/8/8/8/8/2K5/1b6/8 w - - 0 1");
        assert!(xray.parse_uci_move("c3d4").is_err());
        assert!(side_score(&xray, Color::White) < WIN_NEXT_MOVE);
        // Gegnerische Figur AUF dem Huegel, ungedeckt: schlagen gewinnt.
        let capture = board("4k3/8/8/8/3n4/4K3/8/8 w - - 0 1");
        assert!(capture.parse_uci_move("e3d4").is_ok());
        assert!(side_score(&capture, Color::White) >= WIN_NEXT_MOVE);
    }

    #[test]
    fn unguarded_hill_squares_count_against_opponent() {
        // Weiss am Zug, Koenig e2 (Distanz 2, kein Einstieg). Schwarz hat
        // in beiden Varianten Turm + Laeufer (gleiche Restarmee → Term 1
        // identisch). Variante A: Turm a8 / Laeufer h3 decken kein
        // Huegelfeld → 4 ungedeckte Felder fuer Weiss. Variante B: Turm d8
        // deckt d4/d5, Laeufer h7 deckt e4 → nur e5 ungedeckt. Der
        // Unterschied muss genau 3 * UNGUARDED[2] betragen; verglichen wird
        // nur `side_score`, damit die generische Eval (PST der Figuren)
        // nicht hineinspielt.
        let open = board("r6k/8/8/8/8/7b/4K3/8 w - - 0 1");
        let closed = board("3r3k/7b/8/8/8/8/4K3/8 w - - 0 1");
        let a = side_score(&open, Color::White);
        let b = side_score(&closed, Color::White);
        assert_eq!(a - b, 3 * UNGUARDED_HILL_PER_SQUARE[2]);
    }

    #[test]
    fn enemy_army_dampens_king_march() {
        // Gleicher weisser Koenig auf e2 (Distanz 2). Gegen einen Gegner
        // mit voller Figurenarmee ist der Naehe-Bonus kleiner als gegen
        // einen Gegner ohne Figuren. Nur Term 1 vergleichen: die
        // gegnerischen Figuren stehen so, dass sie keinen Huegel decken
        // (a8/b8/... und Tuerme/Dame hinter eigenen Bauern).
        let bare = board("4k3/8/8/8/8/8/4K3/8 w - - 0 1");
        let full = board("rnbqkbnr/pppppppp/8/8/8/8/4K3/8 w - - 0 1");
        let bare_near = taper(HILL_DIST_MG[2], HILL_DIST_EG[2], enemy_army_phase(&bare, Color::Black));
        let full_near = taper(HILL_DIST_MG[2], HILL_DIST_EG[2], enemy_army_phase(&full, Color::Black));
        assert_eq!(bare_near, HILL_DIST_EG[2]);
        assert_eq!(full_near, HILL_DIST_MG[2]);
        assert!(bare_near > full_near);
    }

    // --- Suche -------------------------------------------------------------

    #[test]
    fn search_plays_king_onto_hill_for_immediate_win() {
        // Weisser Koenig e3, schwarzer Turm d8 deckt d4 → nur Ke4 gewinnt
        // sofort; Kd4 waere illegal. Die Suche muss e3e4 mit Mate-Score
        // liefern.
        let result = run_search("3r3k/8/8/8/8/4K3/8/8 w - - 0 1", Some(2), 10_000).unwrap();
        assert_eq!(result.best, ChessMove::new(Square::E3, Square::E4, None));
        assert!(result.score > MATE_SCORE_MIN, "score {}", result.score);
    }

    #[test]
    fn search_defends_against_entry_threat() {
        // Schwarz am Zug, weisser Koenig e3. Die schwarze Dame d8 deckt
        // d4 (d-Linie), e4 ist aber frei → Weiss droht Ke4. Schwarz muss
        // so ziehen, dass BEIDE Einstiegsfelder gedeckt bleiben (z. B.
        // Qd5, Qa4 oder Qh4 — jeweils Reihe/Diagonale ueber d4 und e4).
        // Wir pruefen nicht den konkreten Zug, sondern robust die Folge:
        // nach dem gefundenen Zug darf Weiss keinen sofortigen Huegelsieg
        // mehr haben.
        let fen = "3q3k/8/8/8/8/4K3/8/8 b - - 0 1";
        let result = run_search(fen, Some(4), 10_000).unwrap();
        let b = board(fen);
        let after = b.make_move_new(result.best);
        // Kein Koenigszug von Weiss fuehrt direkt auf den Huegel.
        let wins: Vec<ChessMove> = after
            .legal_gen()
            .filter(|mv| after.make_move_new(*mv).is_variant_loss())
            .collect();
        assert!(wins.is_empty(), "nach {} gewinnt Weiss sofort mit {:?}", result.best, wins);
        // Und die Bewertung aus Sicht von Schwarz ist nicht "verloren".
        assert!(result.score > -MATE_SCORE_MIN, "score {}", result.score);
    }

    #[test]
    fn search_runs_on_middlegame_with_movetime() {
        // Typische KotH-Mittelspielstellung (Italienisch), reine
        // Zeitvorgabe: legaler Zug, kein Panik-Score.
        let fen = "r1bqk2r/pppp1ppp/2n2n2/2b1p3/2B1P3/3P1N2/PPP2PPP/RNBQK2R w KQkq - 0 5";
        let result = run_search(fen, None, 300).expect("Suche liefert einen Zug");
        let legal: Vec<ChessMove> = board(fen).legal_gen().collect();
        assert!(legal.contains(&result.best), "illegaler bestmove {}", result.best);
        assert!(result.score.abs() < 1000, "score {}", result.score);
    }
}
