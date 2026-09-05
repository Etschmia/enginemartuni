//! Racing Kings — varianten-spezifische Bewertung.
//!
//! Regeln: beide Lager starten auf Reihe 1/2 (Weiss rechts, Schwarz links),
//! es gibt keine Bauern und keine Rochade, Schachgebote sind VERBOTEN
//! (die Zuggenerierung laesst keinen Zug zu, der den gegnerischen Koenig
//! angreift). Ziel ist es, den eigenen Koenig als Erster auf die 8. Reihe
//! zu bringen — fuer BEIDE Seiten dieselbe Reihe 8. Erreicht Weiss die
//! Reihe zuerst, darf Schwarz noch genau einen Zug nachziehen; schafft es
//! sein Koenig damit ebenfalls auf Reihe 8, ist es Remis (Ausgleich fuer
//! den Anzugsvorteil). shakmaty bildet Zuggenerierung und Spielende exakt
//! ab (`is_variant_win`/`is_variant_draw`/`is_variant_loss`, leere
//! Zugliste), die Suche sieht den Zieleinlauf damit wie ein Matt.
//!
//! Warum die generische Bewertung (`base`) hier NICHT taugt: Material,
//! PSTs, Koenigssicherheit, Mobilitaet — alles ist auf "Matt setzen"
//! geeicht. In Racing Kings ist der Koenig kein Schutzobjekt, sondern der
//! Laeufer im Wettlauf, und Figuren sind nur insofern wertvoll, wie sie
//! Felder KONTROLLIEREN: da Schach verboten ist, darf ein Koenig kein
//! angegriffenes Feld betreten — ein kontrolliertes Feld ist also eine
//! Sperre im Weg des gegnerischen Koenigs. `base` wird deshalb komplett
//! verworfen und durch fuenf eigene Terme ersetzt (alle in Centipawns,
//! Rueckgabe aus Sicht von Weiss):
//!
//!   1. Reihe des eigenen Koenigs — stark konvexe Tabelle (`RANK_SCORE`):
//!      von Reihe 1 auf 2 zu ziehen bringt fast nichts, von Reihe 6 auf 7
//!      sehr viel. Der Term ist reine Geometrie und liefert auch dann
//!      einen Anreiz zum Vorruecken, wenn der Weg (Term 2) gerade
//!      versperrt ist.
//!   2. Kuerzester freier Koenigsweg zur 8. Reihe (`PATH_SCORE`): eine
//!      Breitensuche ueber Koenigsschritte, die nur Felder betritt, die
//!      der Koenig auch legal betreten DARF — nicht von eigenen Figuren
//!      belegt, nicht vom Gegner angegriffen (Schachverbot!), nicht neben
//!      dem gegnerischen Koenig. Gegnerische Figuren sind passierbar,
//!      wenn sie ungedeckt sind (der Koenig schlaegt sie einfach). Die
//!      Zahl der Schritte ist die "wahre" Distanz; sie beruecksichtigt
//!      Blockaden, die Reihe allein nicht sieht. Kein Weg innerhalb von
//!      `MAX_PATH` Schritten = blockiert (Index `NO_PATH`).
//!   3. Sperre vor dem gegnerischen Koenig (`FORWARD_BLOCK_PER_SQUARE`,
//!      `KING_NO_FORWARD`): wie viele seiner bis zu drei Vorwaertsfelder
//!      (eine Reihe hoeher) kann er NICHT betreten, weil wir sie angreifen
//!      oder seine eigenen Figuren sie belegen? Das ist die "Sperre"
//!      im engeren Sinn — kleiner, direkter Anreiz, Figuren so zu
//!      stellen, dass sie dem gegnerischen Koenig den naechsten Schritt
//!      verbieten (Term 2 misst nur die Folge davon, die Wegverlaengerung).
//!   4. Material (`PIECE_VALUE`): deutlich unter den orthodoxen Werten
//!      (Bauer 100 … Dame 900), weil eine Figur hier nur als Blockierer
//!      zaehlt und ein Schlagzug oft ein Tempo im Wettlauf kostet. Die
//!      Dame bleibt die wertvollste Sperrfigur (sie kontrolliert die
//!      meisten Felder), ein Springer die schwaechste.
//!   5. Wettlauf-Prognose (`race_projection`): der einzige Term, der beide
//!      Koenige UND das Zugrecht zusammen betrachtet. Aus den Wegen aus
//!      Term 2 wird ausgerechnet, wer bei ungestoertem Rennen zuerst
//!      ankommt — in HALBZUEGEN, damit das Zugrecht und die Remis-Regel
//!      exakt eingehen (Herleitung bei `race_lead`). Ergebnis: Sieg fuer
//!      Weiss, Remis oder Sieg fuer Schwarz, gewichtet damit, wie nah der
//!      Fuehrende schon am Ziel ist (`RACE_LEAD`; je kuerzer der Restweg,
//!      desto sicherer die Prognose) plus ein kleiner Zuschlag je
//!      Halbzug Vorsprung. Kann die Seite am Zug im NAECHSTEN Zug legal
//!      einlaufen und die Prognose sagt "Sieg", ist die Partie
//!      entschieden — dafuer gibt es wie in King of the Hill einen
//!      "entschieden"-Wert (`WIN_NEXT_MOVE`) oberhalb jeder
//!      Materialsumme, damit auch der Quiescence-Stand-Pat (der stille
//!      Koenigszuege nicht sucht) den Gewinn sieht.
//!
//! Signatur und Konvention siehe `crate::variants` (Modul-Doku).

use crate::backend::EngineBoard;
use crate::eval_config::EvalParams;
use chess::{
    get_bishop_moves, get_king_moves, get_knight_moves, get_rank, get_rook_moves, BitBoard, Color,
    Piece, Rank, Square, EMPTY,
};

// ---------------------------------------------------------------------------
// Term 1: Reihe des Koenigs, Index = Reihenindex (0 = Reihe 1, 7 = Reihe 8).
//
// Stark konvex: die ersten Schritte sind fast wertlos (jeder Koenig kommt
// von Reihe 1/2 weg), die letzten entscheiden. Reihe 8 selbst ist in der
// Suche fast nie zu bewerten (Stellung dann terminal, Zugliste leer) —
// einzige Ausnahme: Weiss steht auf Reihe 8, Schwarz ist am Zug und kann
// nachziehen (Remis-Regel, Partie laeuft noch). Dafuer und fuer das
// UCI-Kommando `eval` steht ein eindeutiger Wert in Slot 7.
// ---------------------------------------------------------------------------
const RANK_SCORE: [i32; 8] = [0, 5, 12, 25, 45, 80, 130, 200];

// ---------------------------------------------------------------------------
// Term 2: kuerzester freier Koenigsweg, Index = Anzahl Koenigsschritte bis
// Reihe 8 (0 = steht schon dort). Laengster sinnvoller Weg: 7 Schritte
// (Reihe 1 → 8 ohne Umweg); Umwege darueber hinaus behandeln wir wie
// "blockiert" (Index NO_PATH) — bis dahin hat sich das Brett ohnehin
// mehrfach geaendert.
//
// Ebenfalls konvex, aber staerker gewichtet als Term 1, weil dieser Term
// die Blockaden kennt: ein Koenig auf Reihe 6 mit versperrtem Weg ist
// weniger wert als einer auf Reihe 5 mit freier Bahn.
// ---------------------------------------------------------------------------
const MAX_PATH: usize = 7;
const NO_PATH: usize = MAX_PATH + 1;
const PATH_SCORE: [i32; 9] = [300, 220, 150, 100, 65, 40, 22, 10, 0];

// ---------------------------------------------------------------------------
// Term 3: Sperre vor dem gegnerischen Koenig.
//
// Pro Vorwaertsfeld (Reihe +1, Linie −1/0/+1), das er nicht betreten
// darf, ein kleiner Bonus; am Brettrand zaehlt das fehlende dritte Feld
// mit (ein Randkoenig IST eingeschraenkter). Kann er GAR NICHT vorwaerts,
// muss er Zeit mit Seitwaerts-/Rueckwaertszuegen verlieren: Zuschlag.
// ---------------------------------------------------------------------------
const FORWARD_BLOCK_PER_SQUARE: i32 = 20;
const KING_NO_FORWARD: i32 = 40;

// ---------------------------------------------------------------------------
// Term 4: Material, Index = `Piece::to_index()` (Bauer, Springer, Laeufer,
// Turm, Dame, Koenig). Bauern und Koenige gibt es nicht bzw. sie werden
// nie geschlagen → 0. Relationen wie im orthodoxen Schach (Dame > Turm >
// Laeufer ≥ Springer), aber nur etwa die HAELFTE der Hoehe: Figuren
// zaehlen hier nur als Blockierer/Sperrsteine, ein Schlagzug kostet meist
// ein Tempo im Rennen. Warum nicht noch niedriger (ein Viertel wurde
// probiert): die Quiescence rechnet ihr Delta-Pruning mit den orthodoxen
// SEE-Figurenwerten der Suche. Liegt die Stand-Pat-Skala der Eval weit
// darunter, greift das Pruning nicht mehr und der Schlagbaum explodiert
// (gemessen 05.09.2026, Tiefe 4 ab Startstellung: Viertel-Werte 1,08 M
// Knoten, halbe Werte 0,70 M). Halbe Hoehe ist der Kompromiss zwischen
// "Material ist fast egal" und einer zur Suche passenden Skala.
// ---------------------------------------------------------------------------
const PIECE_VALUE: [i32; 6] = [0, 150, 160, 250, 450, 0];

// ---------------------------------------------------------------------------
// Term 5: Wettlauf-Prognose.
//
// RACE_LEAD: Bonus fuer den prognostizierten Sieger, Index = sein Restweg
// in Koenigsschritten (Term-2-Distanz). Index 0 (steht schon auf Reihe 8,
// Partie laeuft nur wegen der Remis-Regel noch) und Index 1 sind fast
// sicher; je laenger der Restweg, desto mehr kann der Gegner mit seinen
// Figuren noch dazwischenfunken. Groessenordnung: bei kurzem Restweg
// deutlich mehr als eine Dame (PIECE_VALUE), bei langem etwa ein Laeufer.
//
// LEAD_EXTRA_PER_PLY: je Halbzug Vorsprung ueber das fuer den Sieg noetige
// Minimum hinaus ein kleiner Zuschlag — ein Rennen "mit Reserve" ist
// robuster gegen Stoerungen. Gedeckelt ueber LEAD_EXTRA_MAX_PLIES.
//
// WIN_NEXT_MOVE: Seite am Zug kann legal einlaufen und gewinnt damit
// (Prognose "Sieg", d. h. der Gegner kann NICHT nachziehen). Oberhalb
// jeder Materialsumme (2 × 450 + 2 × 250 + 2 × 160 + 2 × 150 = 2020 je
// Seite, plus Wettlauf-Boni bis 3000 — Summe deutlich unter 5000 bei
// realistischen Stellungen), aber weit unter
// der Mate-Schwelle der Suche (99 000), damit die Suche den Wert wie
// eine normale Bewertung behandelt und ein gefundenes Matt/Zieleinlauf
// (MATE − ply) immer noch hoeher liegt.
// ---------------------------------------------------------------------------
const RACE_LEAD: [i32; 9] = [3000, 800, 500, 340, 240, 170, 120, 90, 0];
const LEAD_EXTRA_PER_PLY: i32 = 25;
const LEAD_EXTRA_MAX_PLIES: i32 = 6;
const WIN_NEXT_MOVE: i32 = 5_000;

/// Ergebnis der Wettlauf-Prognose aus `race_lead` (Sicht von Weiss).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RaceForecast {
    /// Weiss laeuft zuerst ein, Schwarz kann nicht nachziehen.
    WhiteWins { surplus_plies: i32 },
    /// Weiss laeuft zuerst ein, Schwarz zieht sofort nach (Remis-Regel).
    Draw,
    /// Schwarz laeuft zuerst ein (Weiss darf nie nachziehen).
    BlackWins { surplus_plies: i32 },
}

/// Alles, was beide Seiten fuer die Bewertung brauchen, einmal berechnet:
/// Angriffskarten (je OHNE den gegnerischen Koenig in der Belegung, siehe
/// `attack_map`-Aufruf) und die Term-2-Distanzen beider Koenige.
struct RaceContext {
    /// Felder, die Weiss bzw. Schwarz angreift; Index = `Color::to_index()`.
    attacks: [BitBoard; 2],
    /// Koenigsschritte bis Reihe 8 (0..=MAX_PATH) oder NO_PATH.
    dist: [usize; 2],
}

/// Ersetzt die generische Bewertung komplett durch die Racing-Kings-Terme
/// (s. Modul-Doku). `p` und `phase` werden nicht gebraucht — die
/// Konstanten leben hier im Modul, und eine Spielphase im orthodoxen Sinn
/// gibt es nicht (das Rennen ist von Zug 1 an "Endspiel").
#[inline]
pub fn adjust<B: EngineBoard>(board: &B, _p: &EvalParams, _phase: i32, _base: i32) -> i32 {
    // Regelkonforme Racing-Kings-Stellungen haben immer beide Koenige; der
    // Guard ist die Konvention aller Varianten-Module (Platzhalter-Feld nie
    // bewerten). Ohne Koenig bleibt nur das Material.
    if !board.has_king(Color::White) || !board.has_king(Color::Black) {
        return material(board, Color::White) - material(board, Color::Black);
    }

    let ctx = RaceContext::new(board);
    let white = side_score(board, &ctx, Color::White);
    let black = side_score(board, &ctx, Color::Black);
    white - black + race_projection(board, &ctx)
}

impl RaceContext {
    fn new<B: EngineBoard>(board: &B) -> Self {
        let occ = *board.combined();
        let wk = board.king_square(Color::White);
        let bk = board.king_square(Color::Black);
        // Angriffskarte einer Seite OHNE den GEGNERISCHEN Koenig in der
        // Belegung (ein Koenig kann nicht "in seinem eigenen Schatten"
        // ziehen — Standard-Kniff aus King of the Hill). In Racing Kings
        // ist er streng genommen nie noetig, weil ein Koenig hier nie auf
        // einer freien Gleiterlinie steht (das waere Schach, und Schach
        // ist verboten); er kostet nichts und haelt die Wegsuche (Term 2)
        // und den Sperr-Term (Term 3) auch fuer krumme Test-FENs
        // regelkonform.
        let attacks = [
            attack_map(board, Color::White, occ & !BitBoard::from_square(bk)),
            attack_map(board, Color::Black, occ & !BitBoard::from_square(wk)),
        ];
        let dist = [
            king_path_length(wk, forbidden_squares(board, Color::White, attacks[1], bk)),
            king_path_length(bk, forbidden_squares(board, Color::Black, attacks[0], wk)),
        ];
        Self { attacks, dist }
    }
}

/// Felder, die der Koenig von `us` NIE betreten darf: eigene Figuren
/// (muessten erst wegziehen — bewusste Vereinfachung: gilt als versperrt),
/// vom Gegner angegriffene Felder (Schachverbot; deckt auch gedeckte
/// gegnerische Figuren ab) und das Feld des gegnerischen Koenigs selbst
/// (seine Nachbarfelder stecken schon in seiner Angriffskarte).
fn forbidden_squares<B: EngineBoard>(
    board: &B,
    us: Color,
    enemy_attacks: BitBoard,
    enemy_king: Square,
) -> BitBoard {
    *board.color_combined(us) | enemy_attacks | BitBoard::from_square(enemy_king)
}

/// Term 2: Breitensuche ueber Koenigsschritte von `king` bis zur 8. Reihe,
/// ohne `forbidden` zu betreten. Liefert die Schrittzahl (0 = steht schon
/// dort) oder `NO_PATH`, wenn Reihe 8 in `MAX_PATH` Schritten nicht
/// erreichbar ist. Bitboard-Breitensuche: `frontier` sind die Felder der
/// aktuellen Schrittzahl, `reached` alles bisher Besuchte.
fn king_path_length(king: Square, forbidden: BitBoard) -> usize {
    let goal = get_rank(Rank::Eighth);
    let start = BitBoard::from_square(king);
    if start & goal != EMPTY {
        return 0;
    }
    let mut reached = start;
    let mut frontier = start;
    for steps in 1..=MAX_PATH {
        let mut next = EMPTY;
        for sq in frontier {
            next |= get_king_moves(sq);
        }
        next &= !reached & !forbidden;
        if next == EMPTY {
            return NO_PATH;
        }
        if next & goal != EMPTY {
            return steps;
        }
        reached |= next;
        frontier = next;
    }
    NO_PATH
}

/// Terme 1–4 EINER Seite aus deren Sicht (hoeher = besser fuer `us`).
/// Die Differenz beider Seiten bildet zusammen mit Term 5 `adjust`.
fn side_score<B: EngineBoard>(board: &B, ctx: &RaceContext, us: Color) -> i32 {
    let king = board.king_square(us);

    // Term 1: Reihe.
    let mut score = RANK_SCORE[king.get_rank().to_index()];

    // Term 2: freier Weg.
    score += PATH_SCORE[ctx.dist[us.to_index()]];

    // Term 3: Sperre vor dem GEGNERISCHEN Koenig. Seine Vorwaertsfelder
    // sind die Koenigsnachbarn eine Reihe hoeher (fuer beide Farben
    // "hoeher" = Richtung Reihe 8). Betretbar = weder von uns angegriffen
    // noch von seinen eigenen Figuren belegt. Steht er schon auf Reihe 8
    // (nur im Remis-Regel-Fall moeglich), gibt es nichts mehr zu sperren.
    let enemy_king = board.king_square(!us);
    let enemy_rank = enemy_king.get_rank().to_index();
    if enemy_rank < 7 {
        let forward = get_king_moves(enemy_king) & get_rank(Rank::from_index(enemy_rank + 1));
        let free = forward & !ctx.attacks[us.to_index()] & !*board.color_combined(!us);
        let free_count = free.popcnt() as i32;
        score += (3 - free_count) * FORWARD_BLOCK_PER_SQUARE;
        if free_count == 0 {
            score += KING_NO_FORWARD;
        }
    }

    // Term 4: Material.
    score + material(board, us)
}

/// Term 4: Materialsumme von `us` nach `PIECE_VALUE`.
fn material<B: EngineBoard>(board: &B, us: Color) -> i32 {
    let ours = *board.color_combined(us);
    [Piece::Knight, Piece::Bishop, Piece::Rook, Piece::Queen]
        .iter()
        .map(|&piece| (*board.pieces(piece) & ours).popcnt() as i32 * PIECE_VALUE[piece.to_index()])
        .sum()
}

/// Wettlauf-Prognose bei ungestoertem Rennen, in HALBZUEGEN gerechnet.
///
/// Herleitung: Weiss braucht `dw` Koenigsschritte, Schwarz `db`. Die Seite
/// am Zug zieht in den Halbzuegen 1, 3, 5, …, die andere in 2, 4, 6, ….
/// Der eigene n-te Zug faellt damit auf Halbzug `2n − 1` (wenn am Zug)
/// bzw. `2n` (wenn nicht am Zug). Wer den kleineren Ankunfts-Halbzug hat,
/// ist zuerst da. Die Remis-Regel macht das asymmetrisch: laeuft Weiss
/// zuerst ein, bekommt Schwarz noch GENAU EINEN Zug — kommt er damit auch
/// an (Ankunft Schwarz = Ankunft Weiss + 1), ist es Remis. Laeuft Schwarz
/// zuerst ein, ist sofort Schluss. Da die Paritaeten immer verschieden
/// sind, ist `lead = b_plies − w_plies` stets ungerade:
///
///   lead ≥ 3  → Weiss gewinnt (Reserve = lead − 3 Halbzuege)
///   lead = 1  → Remis
///   lead ≤ −1 → Schwarz gewinnt (Reserve = −1 − lead Halbzuege)
///
/// Beispiele (Weiss am Zug): dw = db = 1 → Kx8, Kx8, Remis (lead 1);
/// dw = 1, db = 2 → Weiss gewinnt (lead 3); dw = 2, db = 1 → Weiss zieht,
/// Schwarz laeuft ein, Schwarz gewinnt (lead −1). Blockierte Koenige
/// (`NO_PATH`) rechnen mit ihrem Sentinel-Wert weiter — sie sind einfach
/// "sehr weit weg"; sind BEIDE blockiert, gibt es keine Prognose.
fn race_lead(dw: usize, db: usize, stm: Color) -> Option<RaceForecast> {
    if dw >= NO_PATH && db >= NO_PATH {
        return None;
    }
    let plies = |d: usize, on_move: bool| 2 * d as i32 - i32::from(on_move);
    let w_plies = plies(dw, stm == Color::White);
    let b_plies = plies(db, stm == Color::Black);
    let lead = b_plies - w_plies;
    Some(if lead >= 3 {
        RaceForecast::WhiteWins {
            surplus_plies: lead - 3,
        }
    } else if lead == 1 {
        RaceForecast::Draw
    } else {
        RaceForecast::BlackWins {
            surplus_plies: -1 - lead,
        }
    })
}

/// Term 5: Prognose in Centipawns (Sicht von Weiss), siehe Modul-Doku und
/// `race_lead`.
fn race_projection<B: EngineBoard>(board: &B, ctx: &RaceContext) -> i32 {
    let dw = ctx.dist[Color::White.to_index()];
    let db = ctx.dist[Color::Black.to_index()];
    let stm = board.side_to_move();
    let Some(forecast) = race_lead(dw, db, stm) else {
        return 0;
    };
    let (winner, dist, surplus) = match forecast {
        RaceForecast::Draw => return 0,
        RaceForecast::WhiteWins { surplus_plies } => (Color::White, dw, surplus_plies),
        RaceForecast::BlackWins { surplus_plies } => (Color::Black, db, surplus_plies),
    };
    // Seite am Zug laeuft im naechsten Zug ein und der Gegner kann nicht
    // nachziehen: entschieden (Term-2-Distanz 1 heisst, das Zielfeld ist
    // legal betretbar).
    let value = if dist == 1 && winner == stm {
        WIN_NEXT_MOVE
    } else {
        RACE_LEAD[dist] + surplus.min(LEAD_EXTRA_MAX_PLIES) * LEAD_EXTRA_PER_PLY
    };
    if winner == Color::White {
        value
    } else {
        -value
    }
}

/// Alle Felder, die `side` mit der Belegung `occ` angreift (Springer,
/// Gleiter bis zum ersten Stein in `occ`, Koenig seine acht Nachbarn).
/// Bauern gibt es in Racing Kings nicht (shakmaty lehnt sie im Setup ab).
fn attack_map<B: EngineBoard>(board: &B, side: Color, occ: BitBoard) -> BitBoard {
    let theirs = *board.color_combined(side);
    let mut attacks = EMPTY;
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
    use crate::board_shak::{BoardRacingKings, ShakVariant};
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

    const STARTPOS: &str = "8/8/8/8/8/8/krbnNBRK/qrbnNBRQ w - - 0 1";

    fn board(fen: &str) -> BoardRacingKings {
        BoardRacingKings::from_fen(fen).unwrap_or_else(|e| panic!("FEN {}: {}", fen, e))
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

    fn perft(b: &BoardRacingKings, depth: u32) -> u64 {
        if depth == 0 {
            return 1;
        }
        b.legal_gen()
            .map(|mv| perft(&b.make_move_new(mv), depth - 1))
            .sum()
    }

    // --- Regeln / Adapter --------------------------------------------------

    #[test]
    fn startpos_parses_and_perft_matches_shakmaty() {
        let b = board(STARTPOS);
        assert_eq!(BoardRacingKings::startpos().get_hash(), b.get_hash());
        assert_eq!(b.king_square(Color::White), Square::H2);
        assert_eq!(b.king_square(Color::Black), Square::A2);
        assert!(!b.has_castle_rights());
        assert_eq!(b.status(), BoardStatus::Ongoing);
        let reference: shakmaty::variant::RacingKings = Fen::from_ascii(STARTPOS.as_bytes())
            .unwrap()
            .into_position(CastlingMode::Standard)
            .unwrap();
        for depth in 1..=3 {
            assert_eq!(
                perft(&b, depth),
                shakmaty::perft(&reference, depth),
                "perft({}) fuer {}",
                depth,
                STARTPOS
            );
        }
        assert_eq!(
            shakmaty::variant::RacingKings::KIND,
            crate::backend::VariantKind::RacingKings
        );
    }

    #[test]
    fn king_on_eighth_rank_is_terminal_win() {
        // Weisser Koenig auf Reihe 8, Weiss am Zug (Schwarz konnte nicht
        // nachziehen): Weiss hat gewonnen.
        let won = board("3K4/8/8/8/8/8/8/k7 w - - 0 1");
        assert!(won.is_variant_win());
        assert!(!won.is_variant_draw());
        assert_eq!(won.legal_gen().count(), 0);
        assert_eq!(won.status(), BoardStatus::Checkmate);
        // Schwarzer Koenig auf Reihe 8, Weiss am Zug: Weiss hat verloren —
        // fuer Weiss gibt es kein Nachziehen.
        let lost = board("3k4/8/8/8/8/8/8/7K w - - 0 1");
        assert!(lost.is_variant_loss());
        assert_eq!(lost.status(), BoardStatus::Checkmate);
        // Der Zug dorthin macht die Stellung terminal.
        let before = board("8/3K4/8/8/8/8/8/k7 w - - 0 1");
        let after = before.make_move_new(before.parse_uci_move("d7d8").unwrap());
        assert!(after.is_variant_loss());
        assert_eq!(after.legal_gen().count(), 0);
    }

    #[test]
    fn draw_when_black_catches_up_immediately() {
        // Weiss steht schon auf Reihe 8, Schwarz am Zug kann nachziehen:
        // Partie laeuft noch (kein Sieg!), nach Kh8 ist es Remis.
        let race = board("K7/7k/8/8/8/8/8/8 b - - 0 1");
        assert!(!race.is_variant_loss() && !race.is_variant_win());
        assert_eq!(race.status(), BoardStatus::Ongoing);
        let after = race.make_move_new(race.parse_uci_move("h7h8").unwrap());
        assert!(after.is_variant_draw());
        assert_eq!(after.legal_gen().count(), 0);
        assert_eq!(after.status(), BoardStatus::Stalemate);
        // Die Suche fuer Schwarz findet genau das Nachziehen und bewertet
        // die Stellung mit 0 (Remis, nicht "verloren").
        // (Kg8 oder Kh8 — beides zieht nach.)
        let result = run_search("K7/7k/8/8/8/8/8/8 b - - 0 1", Some(3), 10_000).unwrap();
        assert_eq!(result.best.get_source(), Square::H7);
        assert_eq!(result.best.get_dest().get_rank(), Rank::Eighth);
        assert_eq!(result.score, 0, "Remis-Fall muss 0 bewertet werden");
        // Statische Sicht auf dieselbe Stellung: Prognose "Remis" → keine
        // Wettlauf-Komponente, die Reihen-/Wegterme sind fast symmetrisch.
        let ctx = RaceContext::new(&race);
        assert_eq!(ctx.dist, [0, 1]);
        assert_eq!(race_projection(&race, &ctx), 0);
    }

    #[test]
    fn search_finds_win_in_one() {
        // Weisser Koenig d7, schwarzer Koenig a1: Kd8/Kc8/Ke8 gewinnt sofort,
        // Schwarz kann nicht nachziehen → Mate-Score.
        let fen = "8/3K4/8/8/8/8/8/k7 w - - 0 1";
        let result = run_search(fen, Some(2), 10_000).unwrap();
        assert_eq!(result.best.get_source(), Square::D7);
        assert_eq!(result.best.get_dest().get_rank(), Rank::Eighth);
        assert!(result.score > MATE_SCORE_MIN, "score {}", result.score);
        // Statisch: Seite am Zug kann einlaufen → "entschieden"-Wert.
        assert!(eval(fen) >= WIN_NEXT_MOVE, "eval {}", eval(fen));
        // Spiegelbild fuer Schwarz: schwarzer Koenig d7, weisser a1,
        // Schwarz am Zug — Schwarz gewinnt sofort.
        let fen_b = "8/3k4/8/8/8/8/8/K7 b - - 0 1";
        let result = run_search(fen_b, Some(2), 10_000).unwrap();
        assert_eq!(result.best.get_dest().get_rank(), Rank::Eighth);
        assert!(result.score > MATE_SCORE_MIN, "score {}", result.score);
        assert!(eval(fen_b) <= -WIN_NEXT_MOVE, "eval {}", eval(fen_b));
    }

    #[test]
    fn search_stops_opponent_from_entering() {
        // Schwarz am Zug, weisser Koenig g7 droht Kg8/Kh8/Kf8. Schwarzer
        // Koenig weit weg (a1), aber schwarze Dame a2 kann die 8. Reihe
        // sperren (Qa8 deckt b8..h8 — nur wenn nichts dazwischen). Nach dem
        // gefundenen Zug darf Weiss keinen sofortigen Einlauf mehr haben.
        let fen = "8/6K1/8/8/8/8/q7/k7 b - - 0 1";
        let result = run_search(fen, Some(4), 10_000).unwrap();
        let after = board(fen).make_move_new(result.best);
        let wins: Vec<ChessMove> = after
            .legal_gen()
            .filter(|mv| after.make_move_new(*mv).is_variant_loss())
            .collect();
        assert!(wins.is_empty(), "nach {} laeuft Weiss sofort ein: {:?}", result.best, wins);
    }

    // --- Bewertung ---------------------------------------------------------

    #[test]
    fn path_length_geometry_and_blockades() {
        // Freie Bahn: Reihe 1 → 8 sind 7 Schritte, Reihe 7 → 8 einer.
        assert_eq!(king_path_length(Square::E1, EMPTY), 7);
        assert_eq!(king_path_length(Square::E7, EMPTY), 1);
        assert_eq!(king_path_length(Square::E8, EMPTY), 0);
        // Komplette Sperre der 8. Reihe (z. B. gegnerischer Turm dort):
        // kein Weg.
        assert_eq!(king_path_length(Square::E7, get_rank(Rank::Eighth)), NO_PATH);
        // Mauer auf Reihe 5 mit einziger Luecke auf h5: Umweg ueber den
        // Rand — e4→f4→g4→h5→g6→g7→g8 = 6 Schritte statt 4.
        let wall = get_rank(Rank::Fifth) & !BitBoard::from_square(Square::H5);
        assert_eq!(king_path_length(Square::E4, wall), 6);
    }

    #[test]
    fn race_lead_tempo_rules() {
        use RaceForecast::*;
        // Weiss am Zug: gleich weit → Remis (Schwarz zieht nach).
        assert_eq!(race_lead(1, 1, Color::White), Some(Draw));
        // Weiss am Zug, ein Schritt voraus → Sieg ohne Reserve.
        assert_eq!(race_lead(1, 2, Color::White), Some(WhiteWins { surplus_plies: 0 }));
        // Weiss am Zug, aber Schwarz naeher → Schwarz gewinnt.
        assert_eq!(race_lead(2, 1, Color::White), Some(BlackWins { surplus_plies: 0 }));
        // Schwarz am Zug, gleich weit → Schwarz zuerst da, kein Nachziehen.
        assert_eq!(race_lead(1, 1, Color::Black), Some(BlackWins { surplus_plies: 0 }));
        // Schwarz am Zug, Weiss einen Schritt voraus → Remis.
        assert_eq!(race_lead(1, 2, Color::Black), Some(Draw));
        // Schwarz am Zug, Weiss zwei Schritte voraus → Weiss gewinnt.
        assert_eq!(race_lead(1, 3, Color::Black), Some(WhiteWins { surplus_plies: 0 }));
        // Reserve waechst mit dem Vorsprung.
        assert_eq!(race_lead(1, 5, Color::White), Some(WhiteWins { surplus_plies: 6 }));
        // Blockierter Weisser gegen freien Schwarzen → Schwarz; beide
        // blockiert → keine Prognose.
        assert!(matches!(race_lead(NO_PATH, 4, Color::White), Some(BlackWins { .. })));
        assert_eq!(race_lead(NO_PATH, NO_PATH, Color::White), None);
        // Weiss schon auf Reihe 8, Schwarz am Zug und einen Schritt
        // entfernt → Remis; zwei Schritte → Weiss gewinnt.
        assert_eq!(race_lead(0, 1, Color::Black), Some(Draw));
        assert_eq!(race_lead(0, 2, Color::Black), Some(WhiteWins { surplus_plies: 0 }));
    }

    #[test]
    fn startpos_is_symmetric() {
        // Spiegelsymmetrische Aufstellung, Prognose "Remis" (gleich weit,
        // Weiss am Zug) → 0.
        assert_eq!(eval(STARTPOS), 0);
        let b = board(STARTPOS);
        let ctx = RaceContext::new(&b);
        assert_eq!(ctx.dist[0], ctx.dist[1]);
        assert_eq!(side_score(&b, &ctx, Color::White), side_score(&b, &ctx, Color::Black));
    }

    #[test]
    fn advanced_king_scores_better() {
        // Reines Koenigsrennen, Schwarz am Zug (kein Sieg-im-naechsten-Zug
        // fuer Weiss). Schwarzer Koenig fest auf a1; weisser Koenig e2 <
        // e4 < e6. Die Prognose ist ueberall "Weiss gewinnt", nur der
        // Restweg wird kuerzer — jede Stufe muss deutlich besser sein.
        let r2 = eval("8/8/8/8/8/8/4K3/k7 b - - 0 1");
        let r4 = eval("8/8/8/8/4K3/8/8/k7 b - - 0 1");
        let r6 = eval("8/8/4K3/8/8/8/8/k7 b - - 0 1");
        assert!(r4 > r2 + 50, "e4 {} vs e2 {}", r4, r2);
        assert!(r6 > r4 + 100, "e6 {} vs e4 {}", r6, r4);
        // Farbspiegelbild (Weiss am Zug, schwarzer Koenig e4, weisser a1):
        // Vorzeichen dreht. NICHT betragsgleich — die Remis-Regel ist
        // asymmetrisch (Schwarz muss nur ZUERST ankommen, Weiss mit einem
        // vollen Zug Vorsprung), deshalb hat Schwarz hier 2 Halbzuege mehr
        // Reserve; der Unterschied ist genau der Reserve-Zuschlag.
        let mirror = eval("8/8/8/8/4k3/8/8/K7 w - - 0 1");
        assert!(mirror < 0, "mirror {}", mirror);
        assert_eq!(mirror + r4, -2 * LEAD_EXTRA_PER_PLY, "mirror {} r4 {}", mirror, r4);
        // Konvexitaet der Reihentabelle: die Zuwaechse steigen monoton.
        for r in 1..7 {
            assert!(
                RANK_SCORE[r + 1] - RANK_SCORE[r] >= RANK_SCORE[r] - RANK_SCORE[r - 1],
                "RANK_SCORE nicht konvex bei Index {}",
                r
            );
        }
    }

    #[test]
    fn blocked_path_is_worth_less_than_free_path() {
        // Weisser Koenig e5, Schwarz am Zug. Variante A: zwei schwarze
        // Tuerme a8/b8 decken sich gegenseitig und sperren die ganze
        // 8. Reihe → Weiss kommt nicht durch (NO_PATH). (Ein EINZELNER,
        // ungedeckter Turm auf a8 waere keine Sperre: der Koenig marschiert
        // hin und schlaegt ihn — genau das bildet die Wegsuche ab.)
        // Variante B: dieselben Tuerme auf a3/b3 (gleiches Material, keine
        // Sperre der Zielreihe) → freie Bahn. Die freie Bahn muss klar
        // besser sein, obwohl die Reihe identisch ist.
        let blocked = board("rr6/8/8/4K3/8/8/8/k7 b - - 0 1");
        let free = board("8/8/8/4K3/8/rr6/8/k7 b - - 0 1");
        let ctx_blocked = RaceContext::new(&blocked);
        let ctx_free = RaceContext::new(&free);
        assert_eq!(ctx_blocked.dist[0], NO_PATH);
        assert_eq!(ctx_free.dist[0], 3);
        let e_blocked = evaluate(&blocked, &EvalParams::default());
        let e_free = evaluate(&free, &EvalParams::default());
        assert!(e_free > e_blocked + 200, "frei {} vs blockiert {}", e_free, e_blocked);
    }

    #[test]
    fn controlling_squares_ahead_of_enemy_king_is_rewarded() {
        // Schwarzer Koenig e6, Weiss am Zug, weisser Koenig a1 (weit weg).
        // Variante A: weisser Turm h7 deckt d7/e7/f7 → alle drei
        // Vorwaertsfelder gesperrt (Term 3 voll + Wegverlaengerung).
        // Variante B: derselbe Turm auf h1 → nichts gesperrt. Nur der
        // Sperr-Term wird verglichen (side_score enthaelt Term 3 fuer
        // "uns" = Weiss gegen den schwarzen Koenig).
        let sealed = board("8/7R/4k3/8/8/8/8/K7 w - - 0 1");
        let open = board("8/8/4k3/8/8/8/8/K6R w - - 0 1");
        let ctx_sealed = RaceContext::new(&sealed);
        let ctx_open = RaceContext::new(&open);
        // Weg des schwarzen Koenigs: gesperrt laenger als offen.
        assert!(ctx_sealed.dist[1] > ctx_open.dist[1]);
        // Turm h1 deckt Reihe 1 und h-Linie — keine Vorwaertsfelder von e6.
        // Turm h7: d7/e7/f7 (Reihe 7) sind alle angegriffen → 3 gesperrt.
        let s_sealed = side_score(&sealed, &ctx_sealed, Color::White);
        let s_open = side_score(&open, &ctx_open, Color::White);
        // Reihe/Weg/Material von Weiss identisch (Koenig a1 beide Male,
        // Turm beide Male) → Differenz = 3 Felder + Zuschlag "gar nicht
        // vorwaerts".
        assert_eq!(
            s_sealed - s_open,
            3 * FORWARD_BLOCK_PER_SQUARE + KING_NO_FORWARD,
            "sealed {} open {}",
            s_sealed,
            s_open
        );
    }

    #[test]
    fn enemy_king_neighbourhood_is_forbidden() {
        // Weisser Koenig e5, schwarzer Koenig e7, Weiss am Zug: d6/e6/f6
        // sind Nachbarfelder des schwarzen Koenigs → nicht betretbar
        // (Koenige duerfen sich nicht beruehren). Der direkte Weg (3
        // Schritte) ist damit weg; es bleibt der Umweg um den Koenig herum
        // (z. B. e5→d5→c6→d7? nein, d7 ist Nachbar von e7 → c7→c8 = 4).
        let b = board("8/4k3/8/4K3/8/8/8/8 w - - 0 1");
        let ctx = RaceContext::new(&b);
        let black_attacks = ctx.attacks[Color::Black.to_index()];
        for sq in [Square::D6, Square::E6, Square::F6] {
            assert!(black_attacks & BitBoard::from_square(sq) != EMPTY, "{}", sq);
            assert!(b.parse_uci_move(&format!("e5{}", sq)).is_err(), "{}", sq);
        }
        assert_eq!(ctx.dist[Color::White.to_index()], 4);
        // Das Feld des gegnerischen Koenigs selbst ist ebenfalls tabu (die
        // Angriffskarte allein wuerde es nicht ausschliessen).
        let forbidden = forbidden_squares(&b, Color::White, black_attacks, Square::E7);
        assert!(forbidden & BitBoard::from_square(Square::E7) != EMPTY);
    }

    #[test]
    fn search_runs_on_middlegame_with_movetime() {
        // Typische Stellung nach ein paar Zuegen: legaler Zug, plausible
        // Groessenordnung (kein Panik-Score).
        let fen = "8/8/8/8/2k5/8/qrbnNBK1/1r1nNBRQ w - - 0 1";
        let result = run_search(fen, None, 300).expect("Suche liefert einen Zug");
        let legal: Vec<ChessMove> = board(fen).legal_gen().collect();
        assert!(legal.contains(&result.best), "illegaler bestmove {}", result.best);
        assert!(result.score.abs() < 2000, "score {}", result.score);
    }
}
