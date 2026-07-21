use crate::endgame;
use crate::eval::evaluate;
use crate::eval_config::EvalParams;
use crate::polyglot::BookSet;
use crate::position::move_to_uci;
use crate::syzygy::Syzygy;
use crate::tt::{TranspositionTable, TtFlag};
use crate::backend::{EngineBoard, MoveGenLike};
use chess::{BitBoard, BoardStatus, ChessMove, Color, Piece, Square, EMPTY};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const INF: i32 = 1_000_000;
const MATE: i32 = 100_000;
const MATE_THRESHOLD: i32 = MATE - 1000;

// --- Mate-Score-Normierung fuer die TT -------------------------------------
// Suchscores codieren Mattdistanz relativ zur WURZEL der laufenden Suche:
// "Matt in k Plies ab Wurzel" = MATE - k (bzw. -MATE + k, wenn die Seite am
// Zug selbst matt gesetzt wird). Ein TT-Eintrag entsteht aber an einem Knoten
// bei `ply` und wird spaeter von ganz anderen Suchen (anderer Wurzel, anderem
// ply) wiederverwendet. Wuerde der Score roh gespeichert, waere die Distanz
// dort ein Fossil der alten Suche: die Engine meldet Zug um Zug "mate 15",
// ohne dass die Distanz je schrumpft, waehlt ihre Zuege aus veralteten
// Eintraegen und schiebt Dame/Turm bis zum 50-Zuege- oder 3-fold-Remis
// (Repro 10.06.2026: zKfpQEn8/Wk1Ynq5F/rYuxSr81/NPiP3O5A, vier verschenkte
// Mop-up-Gewinne; Details docs/roadmap.md).
//
// Standardloesung (jede TT-Engine macht das so): beim SPEICHERN die Distanz
// auf den Knoten selbst normieren ("Matt in k Plies ab DIESEM Knoten" —
// unabhaengig davon, wo die Wurzel lag), beim LESEN zurueck auf die Wurzel
// der jetzigen Suche rechnen. Normale (Nicht-Matt-)Scores passieren beide
// Funktionen unveraendert.

/// Wurzelrelativ -> knotenrelativ: vor `tt.store` am Knoten `ply` aufrufen.
#[inline]
fn mate_score_to_tt(score: i32, ply: i32) -> i32 {
    if score > MATE_THRESHOLD {
        // Matt FUER die Seite am Zug: MATE - (ply + k)  ->  MATE - k
        score + ply
    } else if score < -MATE_THRESHOLD {
        // Matt GEGEN die Seite am Zug: -MATE + (ply + k)  ->  -MATE + k
        score - ply
    } else {
        score
    }
}

/// Knotenrelativ -> wurzelrelativ: nach `tt.probe` am Knoten `ply` aufrufen.
/// Exaktes Gegenstueck zu `mate_score_to_tt`.
#[inline]
fn mate_score_from_tt(score: i32, ply: i32) -> i32 {
    if score > MATE_THRESHOLD {
        score - ply
    } else if score < -MATE_THRESHOLD {
        score + ply
    } else {
        score
    }
}
// Maximale Summe aller Extensions in einer Suchlinie. 26.04.2026: 6 → 4
// reduziert. Hintergrund: Check-Extensions wurden gleichzeitig von +2 auf
// das Standard-+1 verringert. Cap 4 entspricht damit etwa der alten
// Reichweite — bis zu 2 Schach- + 1 anderer Kandidat oder 2 andere.
//
// 28.04.2026: Schach-Extension wieder phase-abhaengig — im Endspiel
// (game_phase < 16) zurueck auf +2, weil die Suche dort sonst zu wenig
// Tiefe in Mattlinien hat (Endgame-Blunder/Partie 0.49 → 0.60,
// missed_mate 0.04 → 0.075). Im Mittelspiel bleibt +1 (positiver Effekt
// auf positional_collapse / exposed_king bestaetigt). Cap bleibt 4 —
// im Endspiel sind damit nur 2 Schach-Extensions in Folge moeglich,
// das reicht fuer die kritischen Mating-Sequenzen.
const MAX_EXTENSION_PER_LINE: i32 = 4;
const MAX_DEPTH: i32 = 64;
// Plies gehen durch Extensions über MAX_DEPTH hinaus — großzügig dimensionieren.
const MAX_PLY: usize = 128;
// Obergrenze für History-Einträge. Muss deutlich unter dem Abstand zwischen
// Killer-Slots (-25_000) und Unterpromotion (-20_000) bleiben, damit die
// Ordering-Reihenfolge Capture > Killer > Unterpromotion > Quiet erhalten bleibt.
const MAX_HISTORY: i32 = 16_000;

// --- Null-Move-Pruning ---------------------------------------------------
// Idee: Wenn unsere Stellung so gut ist, dass selbst ein "geschenktes" Tempo
// für die Gegenseite die Bewertung nicht unter `beta` druecken kann, duerfen
// wir den ganzen Teilbaum abschneiden. Konstanter Reduktionsfaktor R = 2 als
// Einstieg — adaptive Varianten (R = 2 + depth/6) erst nach stabiler Basis.
// Mindesttiefe 3, weil bei depth ≤ 2 die reduzierte Suche schon in der
// Quiescence landet und nichts spart.
const NMP_REDUCTION: i32 = 2;
const NMP_MIN_DEPTH: i32 = 3;

// --- Late Move Reductions (LMR) -------------------------------------------
// Annahme: Move-Ordering hat die wahrscheinlich besten Zuege schon nach vorne
// sortiert (TT-Move, gute Captures, Killer, History-bevorzugte Quiet-Moves).
// Spaete Quiet-Moves haben empirisch eine sehr geringe Chance, die beste
// Antwort zu sein — wir suchen sie zuerst flacher und schalten bei Bedarf
// auf volle Tiefe zurueck (Re-Search).
//
// Variante A (Tobias' Wahl, 04.05.2026): einfache Stufenformel.
//   - depth >= 6 und Index >= 6 → R = 2
//   - depth >= 3 und Index >= 3 → R = 1
//   - sonst                     → R = 0
// Variante B (logarithmisch, Stockfish-Stil) ist im docs/lmr-plan.md
// vorbereitet und kommt erst, wenn Variante A vermessen ist.
//
// Mindesttiefe 3: bei kleinerer Tiefe wuerden wir effektiv in die Quiescence
// reduzieren und nichts gewinnen. Mindest-Index 3: die ersten drei Zuege
// werden nie reduziert (TT-Move, gute Captures, Killer).
const LMR_MIN_DEPTH: i32 = 3;
const LMR_MIN_MOVE_INDEX: usize = 3;

pub struct GoParams {
    pub wtime: Option<u64>,
    pub btime: Option<u64>,
    pub winc: Option<u64>,
    pub binc: Option<u64>,
    pub depth: Option<u32>,
    pub movetime: Option<u64>,
    pub ponder: bool,
}

impl Default for GoParams {
    fn default() -> Self {
        Self {
            wtime: None,
            btime: None,
            winc: None,
            binc: None,
            depth: None,
            movetime: None,
            ponder: false,
        }
    }
}

pub struct SearchResult {
    pub best: ChessMove,
    pub ponder: Option<ChessMove>,
    /// Score der letzten abgeschlossenen Iteration (wurzelrelativ, cp bzw.
    /// Mate-Codierung MATE-k). Bei Buch-Treffer/forciertem Zug 0, weil dort
    /// keine Suche laeuft. Genutzt von Tests (Matt-Distanz-Regression).
    #[allow(dead_code)] // im Bin-Target (uci.rs) bewusst ungenutzt
    pub score: i32,
}

pub struct SearchRequest<B: EngineBoard> {
    pub board: B,
    pub history: Vec<u64>,
    pub halfmove_clock: u8,
    pub params: GoParams,
    pub tt: Arc<Mutex<TranspositionTable>>,
    pub book: Arc<BookSet>,
    pub eval: Arc<EvalParams>,
    pub stop: Arc<AtomicBool>,
    pub pondering: Arc<AtomicBool>,
    pub move_overhead: u64,
    /// Optionaler Syzygy-Tablebase-Handle (None = aus). Wird in der Suche für
    /// WDL-Cutoffs an ≤N-Steine-Knoten genutzt.
    pub syzygy: Option<Arc<Syzygy>>,
}

struct SearchState<'a> {
    // Exklusiver Zugriff auf die TT fuer die Dauer EINER Suche. Der Lock wird
    // einmal in `search()` genommen und der Guard ueber die gesamte Suche
    // gehalten (single-thread → keine Contention); damit entfaellt das
    // per-Knoten `tt.lock()` im Hot-Path (probe/store). Bit-exakt, reine
    // Overhead-Reduktion (Code-Review 16.06., Punkt "Mutex aus Hot-Path").
    tt: &'a mut TranspositionTable,
    eval: Arc<EvalParams>,
    stop: Arc<AtomicBool>,
    pondering: Arc<AtomicBool>,
    // None = unbegrenzt (Ponder-Modus); wird beim ersten Ponderhit auf
    // now + think_time gesetzt.
    deadline: Option<Instant>,
    think_time: Duration,
    start: Instant,
    nodes: u64,
    // Historie + aktueller Suchpfad; zum Erkennen von Stellungswiederholungen.
    // Zu Suchbeginn enthaelt sie die Spielhistorie (vom Position-Tracker uebergeben);
    // waehrend der Suche pusht jeder Knoten seinen eigenen Hash, bevor er die
    // Kinder aufruft, und popt ihn wieder beim Ruecksprung.
    history: Vec<u64>,
    // Anzahl Hashes in `history` zum Suchstart. Trennt Spielhistorie (Index <
    // root_history_len) vom Suchpfad (Index >= root_history_len). Wird in
    // is_repetition_draw genutzt, um zwischen "schon einmal in der Partie
    // aufgetreten" (1-fold, kein Remis) und "im Suchpfad wiederholt"
    // (2-fold-as-draw-Trick) zu unterscheiden.
    root_history_len: usize,
    root_best_move: Option<ChessMove>,
    // Wenn die Wurzel nur einen legalen Zug hat, merken wir ihn vor: beim
    // Uebergang Ponder → Normal koennen wir dann sofort abbrechen.
    forced_only_move: Option<ChessMove>,
    // Debug-Schalter fuer Root-Move-Traces. Aktivieren mit
    // MARTUNI_DEBUG_ROOT=1, damit normale UCI-Ausgaben unveraendert bleiben.
    debug_root: bool,
    // Diagnose-Schalter: NMP komplett aus, wenn MARTUNI_NMP_OFF=1.
    nmp_off: bool,
    // Killer Moves: pro ply zwei Quiet-Züge, die zuletzt einen Beta-Cutoff
    // erzeugt haben. Werden in der Sortierung direkt hinter gewinnenden
    // Captures einsortiert.
    killers: [[Option<ChessMove>; 2]; MAX_PLY],
    // History-Heuristic: [side][from*64 + to]. Jedes Mal, wenn ein Quiet-Zug
    // einen Beta-Cutoff produziert, wird `depth*depth` aufaddiert (geclampt
    // auf MAX_HISTORY). Quiet Moves werden innerhalb ihres Ordering-Bands
    // nach dem History-Score absteigend sortiert.
    move_history: Vec<i32>,
    // Optionaler Syzygy-Tablebase-Handle (None = aus) und Trefferzähler für
    // die UCI-`tbhits`-Ausgabe.
    syzygy: Option<Arc<Syzygy>>,
    tb_hits: u64,
}

#[inline]
fn history_idx(side: Color, from: Square, to: Square) -> usize {
    let side_idx = match side {
        Color::White => 0,
        Color::Black => 1,
    };
    side_idx * 64 * 64 + from.to_index() * 64 + to.to_index()
}

impl SearchState<'_> {
    fn record_killer(&mut self, ply: i32, mv: ChessMove) {
        let p = ply as usize;
        if p >= MAX_PLY {
            return;
        }
        if self.killers[p][0] == Some(mv) {
            return;
        }
        self.killers[p][1] = self.killers[p][0];
        self.killers[p][0] = Some(mv);
    }

    fn record_history(&mut self, side: Color, mv: ChessMove, depth: i32) {
        let idx = history_idx(side, mv.get_source(), mv.get_dest());
        let bonus = (depth * depth).min(MAX_HISTORY);
        self.move_history[idx] = (self.move_history[idx] + bonus).min(MAX_HISTORY);
    }

    fn killers_at(&self, ply: i32) -> [Option<ChessMove>; 2] {
        let p = ply as usize;
        if p >= MAX_PLY {
            [None, None]
        } else {
            self.killers[p]
        }
    }

    fn should_stop(&mut self) -> bool {
        if self.stop.load(Ordering::Relaxed) {
            return true;
        }
        // Uebergang Ponder → Normal: jetzt die echte Deadline setzen.
        // Bei forciertem Zug sofort abbrechen — der Zug steht fest.
        if self.deadline.is_none() && !self.pondering.load(Ordering::Relaxed) {
            if self.forced_only_move.is_some() {
                self.stop.store(true, Ordering::Relaxed);
                return true;
            }
            self.deadline = Some(Instant::now() + self.think_time);
        }
        if let Some(dl) = self.deadline {
            if self.nodes & 2047 == 0 && Instant::now() >= dl {
                self.stop.store(true, Ordering::Relaxed);
                return true;
            }
        }
        false
    }
}

pub fn search<B: EngineBoard>(req: SearchRequest<B>) -> Option<SearchResult> {
    if req.board.status() != BoardStatus::Ongoing {
        return None;
    }

    // TT EINMAL fuer die gesamte Suche locken. Es laeuft immer nur ein
    // Such-Thread; der Main-Thread (uci.rs) greift auf die TT (clear/resize)
    // nur zwischen Suchen zu (er joint den vorherigen Such-Thread vor jedem
    // neuen `go`). Der gehaltene Guard serialisiert das weiterhin korrekt,
    // erspart aber das per-Knoten Lock/Unlock im Hot-Path.
    let mut tt_guard = req.tt.lock().unwrap();
    // Neue Suche: Generation hochzaehlen, damit Eintraege frueherer Suchen
    // (vorheriger Zuege) als veraltet gelten und leichter verdraengt werden.
    tt_guard.new_search();

    // Eroeffnungsbuch zuerst — auch im Ponder-Modus erlaubt
    // Polyglot-Buecher sind Standard-Schach — nur probieren, wenn das
    // Backend eine Standard-Sicht hat (960-Backend: as_std() == None).
    if !req.book.is_empty() {
        if let Some(m) = req.board.as_std().and_then(|b| req.book.probe(b)) {
            println!("info string book hit");
            let ponder = ponder_move_from_tt(&req.board, m, &tt_guard);
            return Some(SearchResult { best: m, ponder, score: 0 });
        }
    }

    // Legale Wurzelzuege EINMAL erzeugen und fuer alle drei Wurzel-Zwecke
    // wiederverwenden (Forced-Move-Check, forced_only_move-Vormerkung,
    // Fallback last_move). MoveGen ist deterministisch, daher bit-identisch
    // zu drei separaten new_legal-Aufrufen — spart aber zwei MoveGen-Laeufe
    // pro `go` (Effizienz-Review Cursor-Auto 13.06.).
    let root_moves: Vec<ChessMove> = req.board.legal_gen().collect();

    // Forcierter Zug: nur eine legale Antwort → ohne Suche spielen.
    // Im Ponder-Modus muessen wir weiterdenken, bis ponderhit/stop kommt,
    // deshalb nur im normalen Modus kurzschliessen.
    if !req.params.ponder {
        if root_moves.len() == 1 {
            let only = root_moves[0];
            println!("info string forced move");
            let ponder = ponder_move_from_tt(&req.board, only, &tt_guard);
            return Some(SearchResult { best: only, ponder, score: 0 });
        }
    }

    // Syzygy-DTZ-Wurzel-Probe: an einer <= N-Steine-Wurzel ohne Rochaderechte
    // den 50-Zuege-sicher konvertierenden Zug direkt aus der Tabelle spielen.
    // Im Endspiel ist der DTZ-optimale Zug der beste Zug (die eigene Suche kann
    // ihn nicht schlagen), und er vermeidet die 50-Zuege-Remis-Klasse. Im
    // Ponder-Modus NICHT kurzschliessen — dort wird bis ponderhit weitergedacht.
    if !req.params.ponder {
        if let Some(syz) = &req.syzygy {
            if let Some((tb_move, tb_score)) =
                syz.probe_root_move(&req.board, req.halfmove_clock)
            {
                println!("info string syzygy root hit");
                let ponder = ponder_move_from_tt(&req.board, tb_move, &tt_guard);
                return Some(SearchResult {
                    best: tb_move,
                    ponder,
                    score: tb_score,
                });
            }
        }
    }

    let start = Instant::now();
    let think_time = calculate_think_time(&req.params, req.move_overhead, req.board.side_to_move());
    // Ponder: Deadline initial offen lassen, sie wird beim Ponderhit gesetzt.
    let deadline = if req.params.ponder {
        None
    } else {
        Some(start + think_time)
    };

    // Forcierter Zug im Ponder-Modus vormerken: sobald ponderhit kommt
    // (pondering=false), koennen wir ohne weitere Suche zurueckkehren.
    // Nutzt die oben einmalig erzeugten root_moves wieder.
    let forced_only_move = if root_moves.len() == 1 {
        Some(root_moves[0])
    } else {
        None
    };

    let history = req.history;
    let root_history_len = history.len();
    let mut state = SearchState {
        tt: &mut *tt_guard,
        eval: Arc::clone(&req.eval),
        stop: Arc::clone(&req.stop),
        pondering: Arc::clone(&req.pondering),
        deadline,
        think_time,
        start,
        nodes: 0,
        history,
        root_history_len,
        root_best_move: None,
        forced_only_move,
        debug_root: std::env::var_os("MARTUNI_DEBUG_ROOT").is_some(),
        nmp_off: std::env::var_os("MARTUNI_NMP_OFF").is_some(),
        killers: [[None; 2]; MAX_PLY],
        move_history: vec![0; 2 * 64 * 64],
        syzygy: req.syzygy,
        tb_hits: 0,
    };

    // Iteratives Deepening
    let max_depth = req.params.depth.map(|d| d as i32).unwrap_or(MAX_DEPTH);

    let mut completed_depth = 0;
    let mut last_score = 0;
    let mut last_move: Option<ChessMove> = None;

    for depth in 1..=max_depth {
        let score = alpha_beta(
            &req.board,
            depth,
            0,
            -INF,
            INF,
            0,
            req.halfmove_clock,
            true, // allow_null: an der Wurzel NMP grundsaetzlich erlauben
            &mut state,
        );

        if state.stop.load(Ordering::Relaxed) {
            // Laufende Iteration wurde abgebrochen — Ergebnis nicht verwerten
            break;
        }

        completed_depth = depth;
        last_score = score;
        last_move = state.root_best_move;

        emit_info(
            depth,
            score,
            state.nodes,
            state.start.elapsed(),
            last_move,
            state.tb_hits,
        );

        // Gefundenes Matt: nicht weitersuchen — aber nur, wenn die
        // Mattdistanz innerhalb der gerade abgeschlossenen Suchtiefe liegt,
        // die Iteration die Mattlinie also wirklich bis zum Ende verifiziert
        // hat. Ein "mate N" mit N > depth kann nur TT-gestuetzt entstanden
        // sein; vor dem Ply-Adjustment brach genau das die Suche schon bei
        // Tiefe 1 auf einem Fossil ab (0-ms-Zuege, Repro 10.06.2026). Die
        // Bedingung kostet praktisch nichts: ist das Matt echt, holt die
        // naechste Iteration es billig per TT wieder ein und der Break
        // greift dann.
        if score.abs() > MATE_THRESHOLD && MATE - score.abs() <= depth {
            break;
        }
    }

    if completed_depth == 0 {
        // Not a single iteration finished — spiele den ersten legalen Zug
        // (aus den oben einmalig erzeugten root_moves).
        last_move = root_moves.first().copied();
        println!(
            "info string fallback (no completed depth, nodes={})",
            state.nodes
        );
    }

    last_move.map(|best| {
        let ponder = ponder_move_from_tt(&req.board, best, &*state.tt);
        SearchResult {
            best,
            ponder,
            score: last_score,
        }
    })
}

/// Sucht einen Pondermove: Mache den besten Zug, schaue in der TT nach,
/// welcher Zug fuer die Antwortstellung gespeichert ist. Verifiziere
/// Legalitaet, falls die TT-Position eine Kollision war.
fn ponder_move_from_tt<B: EngineBoard>(
    board: &B,
    best: ChessMove,
    tt: &TranspositionTable,
) -> Option<ChessMove> {
    let next = board.make_move_new(best);
    if next.status() != BoardStatus::Ongoing {
        return None;
    }
    let key = next.get_hash();
    let mv = tt.probe(key).and_then(|e| e.best_move)?;
    if next.legal_gen().any(|m| m == mv) {
        Some(mv)
    } else {
        None
    }
}

fn emit_info(
    depth: i32,
    score: i32,
    nodes: u64,
    elapsed: Duration,
    best: Option<ChessMove>,
    tb_hits: u64,
) {
    let ms = elapsed.as_millis().max(1) as u64;
    let nps = (nodes * 1000) / ms;
    let score_str = if score.abs() > MATE_THRESHOLD {
        let mate_in = (MATE - score.abs() + 1) / 2;
        let sign = if score > 0 { 1 } else { -1 };
        format!("mate {}", sign * mate_in)
    } else {
        format!("cp {}", score)
    };
    let pv = best.map(move_to_uci).unwrap_or_default();
    // `tbhits` nur ausgeben, wenn es Tablebase-Treffer gab — so bleibt der
    // Default-Output (ohne Syzygy) byte-identisch zur Vorversion.
    let tb = if tb_hits > 0 {
        format!(" tbhits {tb_hits}")
    } else {
        String::new()
    };
    println!(
        "info depth {depth} score {score_str} nodes {nodes} time {ms} nps {nps}{tb} pv {pv}"
    );
}

fn alpha_beta<B: EngineBoard>(
    board: &B,
    depth: i32,
    ply: i32,
    mut alpha: i32,
    beta: i32,
    extensions_used: i32,
    halfmove: u8,
    allow_null: bool,
    state: &mut SearchState<'_>,
) -> i32 {
    state.nodes += 1;

    if state.should_stop() {
        return 0;
    }

    // Interner Such-/TT-Key: Die chess-Crate pflegt diesen Zobrist-Hash
    // inkrementell in Board::make_move(). Das vermeidet die vorherige
    // Polyglot-Neuberechnung ueber alle 64 Felder an jedem Suchknoten.
    // Polyglot-Hashes bleiben nur fuer das Eroeffnungsbuch relevant.
    let key = board.get_hash();

    // Stellungswiederholung und 50-Zuege-Regel
    if ply > 0 {
        if is_repetition_draw(&state.history, state.root_history_len, key) {
            return 0;
        }
        if halfmove >= 100 {
            return 0;
        }
    }

    // Syzygy-Tablebase-Probe (WDL).
    //
    // Nur an inneren Knoten (ply > 0): die Wurzel braucht einen konkreten Zug,
    // den die WDL-Probe nicht liefert — die 50-Zuege-sichere Wurzel-Konversion
    // (DTZ) folgt als eigene Phase. An einem Knoten mit <= max_pieces Steinen
    // (und ohne Rochaderechte/en passant, von probe_wdl_score geprueft) gibt
    // die Tabelle die spieltheoretische Wahrheit zurueck — wir schneiden den
    // Teilbaum mit diesem Score ab (Win/Loss knotenrelativ, Remis = 0). Das
    // ersetzt dort die fehleranfaellige eigene Endspiel-Heuristik.
    //
    // None (kein Handle / nicht probebar / Tabelle fehlt) => normale Suche.
    // Der Borrow auf state.syzygy endet mit `and_then`, daher ist danach das
    // mutable Hochzaehlen von state.tb_hits konfliktfrei.
    if ply > 0 {
        let tb_score = state
            .syzygy
            .as_deref()
            .and_then(|syz| syz.probe_wdl_score(board, ply));
        if let Some(score) = tb_score {
            state.tb_hits += 1;
            return score;
        }
    }

    // Blattknoten: Quiescence-Suche (qply = 0 beim Eintritt)
    if depth <= 0 {
        return quiescence(board, alpha, beta, ply, 0, state);
    }

    // Transposition Table Probe
    //
    // WICHTIG: Score-Cutoff wird unterdrueckt, wenn die aktuelle Position
    // bereits in der Spielhistorie aufgetreten ist (1-fold game-history
    // match). Hintergrund (Partie PGQZhMjF, 06.05.2026, Wojtmic-Bot vs
    // Martuni): die TT speichert nur den Zobrist-Schluessel der Stellung,
    // nicht den Repetition-Kontext. Wenn dieselbe Stellung im Spielverlauf
    // wiederkehrt, kann ein gespeicherter Mate-/Cutoff-Score von einer
    // frueheren Begegnung stale sein — der zugehoerige Pfad lief frueher
    // zu einem echten Mate, jetzt fuehrt er aber durch eine 3-fold
    // Wiederholung. `is_repetition_draw` weiter unten erkennt das nur,
    // wenn die Suche tatsaechlich rekursiv durchlaeuft; ein TT-Cutoff vor
    // dem Durchlauf bleibt blind.
    //
    // Konsequenz: bei `key_seen_in_game_history` wird der Eintrag nur als
    // Move-Hint genutzt (Move-Ordering bleibt informiert), aber der
    // Score-Cutoff faellt weg. `slice.contains` ist O(n) auf einem kleinen
    // Slice (Spielhistorie typisch < 200 Eintraege), und der Aufruf
    // erfolgt nur, wenn ueberhaupt ein Cutoff-Kandidat vorliegt — kein
    // Hot-Path-Treffer.
    //
    // Spiegelbild des Repetition-Bugs vom 02.05.2026 (`history.contains`
    // zaehlte 1-fold falsch als Remis): die Repetition-Logik ist in beide
    // Richtungen heikel — zu pessimistisch verzerrt Wurzelzuege auf 0,
    // zu optimistisch laesst Engine in 3-fold laufen, obwohl Mate da ist.
    let tt_move: Option<ChessMove>;
    if let Some(entry) = state.tt.probe(key) {
        if entry.depth as i32 >= depth && ply > 0 {
            // Mate-Distanzen liegen knotenrelativ in der TT (siehe
            // mate_score_to_tt) — erst auf die aktuelle Wurzel
            // umrechnen, dann Bounds pruefen.
            let v = mate_score_from_tt(entry.eval, ply);
            let cutoff_fires = match entry.flag {
                TtFlag::Exact => true,
                TtFlag::Lower => v >= beta,
                TtFlag::Upper => v <= alpha,
                _ => false,
            };
            if cutoff_fires {
                let key_seen_in_game_history =
                    state.history[..state.root_history_len].contains(&key);
                if !key_seen_in_game_history {
                    return v;
                }
            }
        }
        tt_move = entry.best_move;
    } else {
        tt_move = None;
    }

    // --- Null-Move-Pruning ----------------------------------------------
    // Bedingungen (alle muessen erfuellt sein):
    //   - allow_null: keine zwei Null Moves hintereinander (sonst sinnlos)
    //   - !is_pv: nur in non-PV-Knoten — die Hauptvariante darf nicht durch
    //     einen Pruning-Trick verfaelscht werden
    //   - !in_check: Null Move waere illegal (Seite muss aus Schach ziehen)
    //   - depth >= NMP_MIN_DEPTH (3): bei kleinerer Tiefe spart NMP nichts,
    //     weil die reduzierte Suche direkt in der Quiescence landet
    //   - ply > 0: an der Wurzel brauchen wir einen echten Best-Move
    //   - has_non_pawn_material: Zugzwang-Schutz — in reinen Bauernendspielen
    //     ist "passen waere mindestens so gut wie ziehen" oft falsch
    //   - static_eval >= beta: nur wenn die Stellung *jetzt schon* gut aussieht
    //     lohnt der Test; sonst ist ein Cutoff unwahrscheinlich
    //
    // Ablauf: Null Move ausfuehren (chess::Board::null_move() — flippt
    // side_to_move, leert en passant), reduziert mit Nullfenster suchen,
    // bei score >= beta Cutoff. Reduktion R = 2 (constant). Die rekursive
    // Suche bekommt allow_null=false, damit kein zweites NMP folgt.
    let is_pv = beta - alpha > 1;
    let in_check = board.checkers().popcnt() > 0;

    // Terminal-Erkennung ohne board.status():
    // Board::status() baut intern selbst MoveGen::new_legal(). Direkt danach
    // brauchte die Suche dieselbe Zuggeneration erneut fuer Ordering. Wir
    // erzeugen die legalen Zuege deshalb genau einmal: erst nachdem TT-Cutoffs
    // die Arbeit eventuell vermeiden konnten, aber noch vor NMP, damit
    // Stalemate/Checkmate nicht faelschlich durch Null-Move-Pruning laufen.
    let legal_moves = board.legal_gen();
    if legal_moves.count_remaining() == 0 {
        return if in_check { -MATE + ply } else { 0 };
    }

    if !state.nmp_off
        && allow_null
        && !is_pv
        && !in_check
        && depth >= NMP_MIN_DEPTH
        && ply > 0
        && has_non_pawn_material(board, board.side_to_move())
    {
        let static_eval = eval_stm(board, &state.eval);
        if static_eval >= beta {
            if let Some(null_board) = board.null_move() {
                // History fuer Repetition-Check kohaerent halten — der
                // null_move-Hash gehoert zum Suchpfad wie jeder andere
                // Kindknoten.
                state.history.push(key);
                let null_score = -alpha_beta(
                    &null_board,
                    depth - 1 - NMP_REDUCTION,
                    ply + 1,
                    -beta,
                    -beta + 1,
                    extensions_used,
                    halfmove.saturating_add(1),
                    false, // allow_null: nach NMP keinen weiteren Null Move
                    state,
                );
                state.history.pop();

                if state.stop.load(Ordering::Relaxed) {
                    return 0;
                }

                if null_score >= beta {
                    // Mate-Scores aus reduzierter Suche sind unzuverlaessig —
                    // niemals als Mate weitergeben, sondern auf beta deckeln.
                    return beta;
                }
            }
        }
    }

    // Zuege ordnen (mit SEE-Cache fuer Captures, Killer + History). `legal_moves`
    // ist derselbe Generator, der oben bereits die Terminal-Erkennung erledigt
    // hat; damit faellt die fruehere zweite MoveGen-Runde pro Knoten weg.
    let killers_here = state.killers_at(ply);
    // Lazy, gestaffelter Picker statt eager Vec<ScoredMove> + Gesamt-Sort. Der
    // `&state.move_history`-Borrow lebt nur innerhalb von `new()` (dort werden
    // die Quiet-History-Scores bei Knoten-Eintritt gelesen) — danach hält der
    // Picker keinen State-Borrow mehr, der Loop darf `state` wieder mutieren.
    let picker = MovePicker::new(
        board,
        legal_moves,
        tt_move,
        killers_here,
        &state.move_history,
    );

    // Eigenen Hash fuer die Kinder in die Historie legen
    if ply > 0 {
        state.history.push(key);
    }

    let orig_alpha = alpha;
    let mut best_score = -INF;
    let mut best_move: Option<ChessMove> = None;
    let mut aborted = false;
    // PVS: der erste Zug bekommt das volle Fenster, alle weiteren werden
    // zuerst mit einem Nullfenster (Scout-Search) getestet.
    let mut first_move = true;

    // `picker.enumerate()` liefert dieselbe `move_idx`-Folge wie früher
    // `ordered.iter().enumerate()`: der Index zählt pro ausgegebenem Zug hoch,
    // unabhängig von `continue`/`break` im Body (enumerate zählt am Iterator,
    // nicht am Kontrollfluss).
    for (move_idx, sm) in picker.enumerate() {
        let mv = sm.mv;
        // Debug-Dump am Wurzelknoten (nur mit MARTUNI_DEBUG_ROOT). Früher als
        // Vorab-Block vor der Schleife; jetzt pro Zug beim Ausgeben — selbe
        // Information, nur mit der Suche verschränkt statt vorgezogen.
        if state.debug_root && ply == 0 {
            let see = sm
                .see_val
                .map(|v| v.to_string())
                .unwrap_or_else(|| "none".to_string());
            println!(
                "info string rootdbg depth {depth} generated move {} order {} see {}",
                move_to_uci(sm.mv),
                sm.order_key,
                see
            );
        }
        let alpha_before = alpha;
        let nb = board.make_move_new(mv);
        // Schach-Extension phase-abhaengig:
        //   Mittelspiel (game_phase >= 16) → +1   (CPW/Stockfish/Crafty-Standard)
        //   Endspiel    (game_phase <  16) → +2   (mehr Tiefe fuer Mating-Sequenzen)
        // Andere Kandidatenzuege (gewinnender Capture, erkanntes Endspiel,
        // Freibauer) bleiben unabhaengig von der Phase bei +2, weil sie
        // taktisch erzwingender sind und seltener auftreten.
        //
        // Historie:
        //  - vor 26.04.2026: Schach pauschal +2, Cap 6
        //  - 26.04.2026: Schach pauschal +1, Cap 4 (zu teuer im Mittelspiel)
        //  - 28.04.2026: Schach +1 im Mittelspiel, +2 im Endspiel — Mittelspiel-
        //    Verbesserung erhalten, Endspiel-Suche wieder tief genug.
        // Phase-Schwelle 16 deckt sich mit `king_activity_phase_threshold` aus
        // der Eval — derselbe Endspiel-Begriff in Suche und Bewertung.
        let child_in_check = nb.checkers().popcnt() > 0;

        // --- SEE-Pruning in der Hauptsuche (konservativ, 29.05.2026) --------
        // Befund analyse-29.05.2026: Martuni spielt Minor-fuer-Bauer-Opfer
        // (Nxf7, Nxe5, Bxe4 — alle SEE = -200), weil ihre Eval *nach* dem
        // Opfer einen spekulativen Angriff um ~270-340 cp ueberschaetzt
        // (Tiefen-Oszillation +10 -> -104 -> +67). Die Quiescence prunt
        // SEE<0 schon laenger, die Hauptsuche durchsuchte verlierende
        // Captures aber voll und liess sich so vom ueberbewerteten Blatt
        // taeuschen. Hier schneiden wir sie nahe den Blaettern weg: damit
        // kollabieren die Folge-Opfer, auf denen die Pseudo-Kompensation
        // aufbaut, und der zurueckgerechnete Wert des Wurzel-Opfers faellt
        // auf seinen echten (negativen) Materialwert.
        //
        // Konservative Gates (Risiko, ein echtes tiefes Opfer zu uebersehen,
        // bewusst minimal gehalten):
        //   - !is_pv          : die Hauptvariante nie beschneiden
        //   - !in_check       : stehen wir selbst im Schach, kein Pruning
        //   - !child_in_check : Schach-gebende Captures sind taktisch erzwingend
        //   - depth <= 2      : nur ganz nahe den Blaettern
        //   - move_idx > 0    : den bestgeordneten Zug nie ueberspringen
        //   - best_score > -MATE_THRESHOLD : nicht in Matt-Verzweiflung prunen
        //   - sm.see_val < 0  : nur klar materialverlierende Captures
        // sm.see_val ist aus dem Move-Ordering bereits gecacht -> keine
        // zusaetzlichen SEE-Aufrufe, das Pruning ist praktisch kostenlos.
        if !is_pv
            && !in_check
            && !child_in_check
            && depth <= 2
            && move_idx > 0
            && best_score > -MATE_THRESHOLD
        {
            if let Some(see) = sm.see_val {
                if see < 0 {
                    continue;
                }
            }
        }

        let other_cand = !child_in_check && is_candidate_move(board, mv, &nb, sm.see_val);
        let check_ext = if child_in_check {
            if crate::eval::game_phase(&nb) < 16 {
                2
            } else {
                1
            }
        } else {
            0
        };
        let ext = if other_cand && extensions_used + 2 <= MAX_EXTENSION_PER_LINE {
            2
        } else if child_in_check && extensions_used + check_ext <= MAX_EXTENSION_PER_LINE {
            check_ext
        } else {
            0
        };
        let new_depth = depth - 1 + ext;
        // `see_val` ist nur fuer echte Captures gesetzt (inkl. en passant).
        // Damit muss die irreversible-Zug-Pruefung hier nicht noch einmal
        // dieselbe Capture-Logik wiederholen; fuer die 50-Zuege-Regel fehlt
        // nur noch der Bauernzug-Test.
        let new_halfmove =
            if sm.see_val.is_some() || board.piece_on(mv.get_source()) == Some(Piece::Pawn) {
                0
            } else {
                halfmove.saturating_add(1)
            };

        // --- Principal Variation Search (PVS) -----------------------------
        // Annahme: durch gute Move-Ordering ist der erste Zug aller
        // Wahrscheinlichkeit nach der beste. Den verifizieren wir mit
        // vollem Fenster — er liefert den "Anker" fuer alpha. Alle
        // weiteren Zuege testen wir nur, ob sie diesen Anker schlagen
        // koennen, mit einem Nullfenster `(-alpha - 1, -alpha)`. Das ist
        // billiger, weil Alpha-Beta bei engerem Fenster mehr Cutoffs
        // erzeugt. Wenn der Test ueberraschend doch besser ist
        // (`alpha < score < beta`), wiederholen wir mit vollem Fenster
        // ("Re-Search"), um den exakten Wert zu bekommen.
        //
        // Zusatznutzen: Nullfenster-Knoten haben `beta - alpha == 1`,
        // d.h. unsere `is_pv`-Bedingung in NMP wird endlich falsch und
        // NMP greift in der Tiefe.
        let score = if first_move {
            -alpha_beta(
                &nb,
                new_depth,
                ply + 1,
                -beta,
                -alpha,
                extensions_used + ext,
                new_halfmove,
                true, // allow_null
                state,
            )
        } else {
            // --- Late Move Reductions (LMR) ----------------------------
            // Vorbedingungen (Tobias-Spezifikation 04.05.2026):
            //   - !is_pv: nur Non-PV-Knoten reduzieren — die Hauptvariante
            //     bleibt unangetastet.
            //   - depth >= LMR_MIN_DEPTH (3): bei kleinerer Tiefe nichts
            //     zu sparen, reduzierte Suche landet direkt in Quiescence.
            //   - move_idx >= LMR_MIN_MOVE_INDEX (3): die ersten drei
            //     sortierten Zuege (TT-Move, gute Captures, Killer) sind
            //     erfahrungsgemaess die wichtigen — niemals reduzieren.
            //   - !in_check: stehen wir selbst im Schach, ist jeder Zug
            //     erzwungen — Reduktion waere ein Bug.
            //   - !child_in_check: Schach-gebende Zuege sind taktisch und
            //     werden in der Schach-Extension ohnehin verlaengert.
            //   - ext == 0: keine Extension aktiv → kein taktischer
            //     Kandidat (gewinnender Capture per SEE>=0, Endspielzug,
            //     Freibauer). Wir wollen keinen Zug zugleich verlaengern
            //     und reduzieren.
            //   - sm.see_val.is_none(): keine Captures reduzieren — der
            //     SEE-Wert wird in `order_moves` ausschliesslich fuer
            //     Capture-Zuege berechnet, also dient `is_none()` als
            //     verlaesslicher "kein Capture"-Marker.
            //   - mv.get_promotion().is_none(): Umwandlungen sind zu
            //     entscheidend, um sie flacher zu rechnen.
            //   - !is_killer: Killer-Moves stehen schon weit vorne, aber
            //     als Sicherheitsnetz hier nochmal explizit ausgeschlossen.
            //
            // History-Heuristic ist BEWUSST kein zusaetzliches LMR-Kriterium
            // — sie wirkt nur ueber die Zugreihenfolge in `order_moves`.
            let is_killer = killers_here.iter().any(|k| *k == Some(mv));
            let can_reduce = !is_pv
                && depth >= LMR_MIN_DEPTH
                && move_idx >= LMR_MIN_MOVE_INDEX
                && !in_check
                && !child_in_check
                && ext == 0
                && sm.see_val.is_none()
                && mv.get_promotion().is_none()
                && !is_killer;
            let reduction = if can_reduce {
                lmr_reduction(depth, move_idx)
            } else {
                0
            };

            // Scout: Nullfenster-Test mit (evtl.) reduzierter Tiefe.
            // Wichtig: `.max(1)` darf nur wirken, wenn wir wirklich reduzieren.
            // Sonst wuerden wir den natuerlichen Uebergang `new_depth == 0`
            // → Quiescence aufblaehen (eine Extra-Ply pro Blattknoten,
            // Knoten explodieren). Bei `reduction == 0` bleibt `scout_depth`
            // exakt gleich `new_depth` — der Pfad ist dann strukturell
            // identisch zum Pre-LMR-PVS.
            let scout_depth = if reduction > 0 {
                (new_depth - reduction).max(1)
            } else {
                new_depth
            };
            let mut scout = -alpha_beta(
                &nb,
                scout_depth,
                ply + 1,
                -alpha - 1,
                -alpha,
                extensions_used + ext,
                new_halfmove,
                true,
                state,
            );

            if state.stop.load(Ordering::Relaxed) {
                aborted = true;
                break;
            }

            // LMR-Re-Search: wenn die reduzierte Suche ueberraschend
            // besser als alpha war, glauben wir der reduzierten Tiefe
            // nicht und suchen den Zug mit voller Tiefe noch einmal —
            // immer noch Nullfenster, weil wir nur wissen wollen, ob
            // er den Anker schlaegt.
            if reduction > 0 && scout > alpha {
                scout = -alpha_beta(
                    &nb,
                    new_depth,
                    ply + 1,
                    -alpha - 1,
                    -alpha,
                    extensions_used + ext,
                    new_halfmove,
                    true,
                    state,
                );

                if state.stop.load(Ordering::Relaxed) {
                    aborted = true;
                    break;
                }
            }

            // PVS-Re-Search: wenn der Zug den Anker schlaegt UND noch
            // nicht ueber beta hinaus liegt, mit vollem Fenster fuer den
            // exakten Wert. Bei `scout >= beta` haben wir ohnehin einen
            // Cutoff — exakter Wert nicht noetig.
            if scout > alpha && scout < beta {
                -alpha_beta(
                    &nb,
                    new_depth,
                    ply + 1,
                    -beta,
                    -alpha,
                    extensions_used + ext,
                    new_halfmove,
                    true,
                    state,
                )
            } else {
                scout
            }
        };

        if state.stop.load(Ordering::Relaxed) {
            aborted = true;
            break;
        }

        if state.debug_root && ply == 0 {
            println!(
                "info string rootdbg depth {depth} move {} score {score} alpha {alpha_before} beta {beta}",
                move_to_uci(mv)
            );
        }

        first_move = false;

        if score > best_score {
            best_score = score;
            best_move = Some(mv);
            if ply == 0 {
                state.root_best_move = Some(mv);
            }
        }
        if score > alpha {
            alpha = score;
        }
        if alpha >= beta {
            // Beta-Cutoff: wenn der kausale Zug ein Quiet-Move ist, als Killer
            // vormerken und History-Score erhöhen. Captures und Promotionen
            // haben eigene Sortier-Schienen und brauchen das nicht.
            if sm.see_val.is_none() && mv.get_promotion().is_none() {
                state.record_killer(ply, mv);
                state.record_history(board.side_to_move(), mv, depth);
            }
            break;
        }
    }

    if ply > 0 {
        state.history.pop();
    }

    if aborted {
        return 0;
    }

    // TT store
    let flag = if best_score >= beta {
        TtFlag::Lower
    } else if best_score > orig_alpha {
        TtFlag::Exact
    } else {
        TtFlag::Upper
    };
    // Mate-Scores knotenrelativ ablegen (siehe mate_score_to_tt) —
    // sonst transportiert der Eintrag die Wurzeldistanz DIESER Suche
    // als Fossil in alle spaeteren Suchen.
    state.tt.store(
        key,
        best_move,
        mate_score_to_tt(best_score, ply),
        depth as i8,
        flag,
    );

    best_score
}
// Maximale Quiescence-Tiefe: begrenzt Explosion bei vielen Captures.
const MAX_QPLY: i32 = 12;
// Delta-Pruning-Margin: ein Capture muss mindestens diesen Betrag über alpha
// liegen können, sonst ist er hoffnungslos (verhindert nutzlose Suche).
// Auf 150 reduziert (war 200): missed_capture-Rate war nach SEE-Einführung
// gestiegen, weil 200cp gute Captures fälschlicherweise prunte.
const DELTA_MARGIN: i32 = 150;

fn quiescence<B: EngineBoard>(
    board: &B,
    mut alpha: i32,
    beta: i32,
    ply: i32,
    qply: i32,
    state: &mut SearchState<'_>,
) -> i32 {
    state.nodes += 1;

    if state.should_stop() {
        return 0;
    }

    let in_check = board.checkers().popcnt() > 0;
    let mut legal_moves = board.legal_gen();
    if legal_moves.count_remaining() == 0 {
        return if in_check { -MATE + ply } else { 0 };
    }

    if in_check {
        // Im Schach: alle legalen Züge durchsuchen, kein Stand-Pat.
        // Stand-Pat wäre falsch, weil die Seite nicht einfach "passen" kann.
        // Tiefenlimit gilt nicht im Schach — sonst würden Matt-Drohungen übersehen.
        let mut best = -INF;
        for mv in legal_moves {
            let nb = board.make_move_new(mv);
            let score = -quiescence(&nb, -beta, -alpha, ply + 1, qply + 1, state);

            if state.stop.load(Ordering::Relaxed) {
                return 0;
            }

            if score > best {
                best = score;
            }
            if score > alpha {
                alpha = score;
            }
            if alpha >= beta {
                break;
            }
        }
        return best;
    }

    // Stand pat (statischer Score aus Sicht der Seite am Zug)
    let stand_pat = eval_stm(board, &state.eval);
    if stand_pat >= beta {
        return stand_pat;
    }
    if stand_pat > alpha {
        alpha = stand_pat;
    }

    // Tiefenlimit: bei ruhigen Stellungen nicht endlos suchen.
    //
    // WICHTIG: gemessen gegen `qply` (quiescence-RELATIV, 0 beim Eintritt),
    // nicht gegen den absoluten Root-`ply`. Vorher kappte `ply >= 12` die
    // Quiescence am absoluten ply 12 — bei tiefen Hauptsuchen (Rapid d14–18)
    // startete sie also schon jenseits des Caps und gab sofort `stand_pat`
    // zurueck, ohne Captures aufzuloesen (gemessen 5–9 % der Q-Knoten am
    // Cap, skalierend mit der Suchtiefe). Relativ gemessen bekommt jedes
    // Blatt dieselbe Aufloesungstiefe, unabhaengig davon, wie tief die
    // Hauptsuche schon ist. Der Name MAX_QPLY passt damit zur Bedeutung.
    if qply >= MAX_QPLY {
        return stand_pat;
    }

    // Taktische QSearch-Zuege: Captures per Zielmaske (gegnerische Figuren
    // plus EP-Zielfeld) und stille Damenumwandlungen separat. Damit iterieren
    // wir nicht mehr alle ruhigen Zielquadrate nur, um sie danach wegzufiltern.
    let mut target_mask = *board.color_combined(!board.side_to_move());
    if let Some(ep_sq) = board.en_passant() {
        target_mask |= BitBoard::from_square(ep_sq.uforward(board.side_to_move()));
    }
    legal_moves.set_iterator_mask(target_mask);

    let mut tactical: Vec<(ChessMove, Option<i32>, i32)> = Vec::new();
    for mv in legal_moves.by_ref() {
        if !board.is_capture(mv) {
            continue;
        }
        let v = see(board, mv);
        tactical.push((mv, Some(v), -v));
    }

    // Nach der maskierten Iteration liefert die Crate mit `!EMPTY` die
    // verbleibenden (nicht-schlagenden) Zuege. Davon nehmen wir stille
    // Queen-Promotions und — NUR am Quiescence-Eintritt (qply == 0) —
    // stille Schachgebote (2C). Letztere fangen forcierte Mattnetze,
    // Dauerschach und Koenigsjagd-Motive, die reine Capture-Quiescence am
    // Horizont uebersieht.
    //
    // Check-Maske (Stockfish-Stil): die Felder, von denen aus eine Figur des
    // jeweiligen Typs den gegnerischen Koenig DIREKT bedroht, haengen nur vom
    // Koenigsfeld und der Belegung ab — einmal vorberechnet, danach ein
    // billiger Bitboard-Test pro Zug (kein make_move noetig). Abzugschachs
    // sind hier bewusst NICHT erfasst (separates v2). Bei qply > 0 entfaellt
    // die Check-Generierung, damit Check-auf-Check-Ketten terminieren.
    let quiet_checks = qply == 0;
    let (knight_chk, bishop_chk, rook_chk, pawn_chk) = if quiet_checks {
        use chess::{get_bishop_moves, get_knight_moves, get_pawn_attacks, get_rook_moves};
        let occ = *board.combined();
        let stm = board.side_to_move();
        let ksq = board.king_square(!stm);
        (
            get_knight_moves(ksq),
            get_bishop_moves(ksq, occ),
            get_rook_moves(ksq, occ),
            // Felder, von denen ein EIGENER Bauer den Koenig bedroht =
            // Angriffsfelder eines GEGNERISCHEN Bauern auf dem Koenigsfeld
            // (gespiegelte Richtung; vgl. all_attackers_to).
            get_pawn_attacks(ksq, !stm, !EMPTY),
        )
    } else {
        (EMPTY, EMPTY, EMPTY, EMPTY)
    };

    legal_moves.set_iterator_mask(!EMPTY);
    for mv in legal_moves {
        if is_quiet_queen_promotion(board, mv) {
            tactical.push((mv, None, -10_000));
            continue;
        }
        // Stilles Schachgebot? Nur am Eintritt, keine Promotions (separat).
        if quiet_checks && mv.get_promotion().is_none() {
            let dest_bb = BitBoard::from_square(mv.get_dest());
            let is_direct_check = match board.piece_on(mv.get_source()) {
                Some(Piece::Knight) => knight_chk & dest_bb != EMPTY,
                Some(Piece::Bishop) => bishop_chk & dest_bb != EMPTY,
                Some(Piece::Rook) => rook_chk & dest_bb != EMPTY,
                Some(Piece::Queen) => (bishop_chk | rook_chk) & dest_bb != EMPTY,
                Some(Piece::Pawn) => pawn_chk & dest_bb != EMPTY,
                _ => false, // Koenig kann nicht direkt Schach bieten
            };
            // Nur SICHERE Schachgebote: die Checkfigur darf auf dem Zielfeld
            // nicht per statischem Abtausch verloren gehen (see_quiet >= 0).
            if is_direct_check && see_quiet(board, mv) >= 0 {
                // see_val = None -> unten kein Bad-Capture-/Delta-Pruning.
                // Ordnungsschluessel 1 sortiert Checks hinter alle Captures.
                tactical.push((mv, None, 1));
            }
        }
    }
    tactical.sort_by_key(|(_, _, order_key)| *order_key);

    for (mv, see_val, _) in tactical {
        if let Some(see_val) = see_val {
            // Bad Capture Pruning: verlierende Schlagzuege ueberspringen.
            if see_val < 0 {
                continue;
            }

            // Delta Pruning: wenn selbst ein optimistischer Gewinn den alpha-Wert
            // nicht mehr erreichen kann, diesen Capture überspringen.
            // Gilt nicht bei Beförderungen (Promotion kann viel mehr wert sein).
            if mv.get_promotion().is_none() && stand_pat + see_val + DELTA_MARGIN < alpha {
                continue;
            }
        }

        let nb = board.make_move_new(mv);
        let score = -quiescence(&nb, -beta, -alpha, ply + 1, qply + 1, state);

        if state.stop.load(Ordering::Relaxed) {
            return 0;
        }

        if score >= beta {
            return score;
        }
        if score > alpha {
            alpha = score;
        }
    }

    alpha
}

fn eval_stm<B: EngineBoard>(board: &B, params: &EvalParams) -> i32 {
    let score = evaluate(board, params);
    if board.side_to_move() == Color::White {
        score
    } else {
        -score
    }
}

/// Erkennt Stellungswiederholung mit Trennung zwischen Spielhistorie und
/// Suchpfad. Hintergrund: ein simples `history.contains(&key)` zaehlt jede
/// frueher gesehene Stellung als Remis (0). Das ist falsch — FIDE verlangt
/// 3-fold, und Engines duerfen nur dann den 2-fold-Suchpfad-Trick anwenden,
/// wenn die Wiederholung tatsaechlich im Suchpfad entsteht (sonst wuerden
/// Wurzelzuege wie 19.Qe4 in vGwmaXUy faelschlich auf 0 gedeckelt — Repro
/// 02.05.2026: 19.Qe4 mit Historie cp 0, ohne Historie cp -573).
///
/// Algorithmus (Stockfish-Stil, vereinfacht): wir gehen `history` rueckwaerts
/// durch und schauen, wo sich `key` wiederfindet.
///   - Match an Index >= `root_history_len`  → die Stellung ist schon einmal
///     im Suchpfad selbst aufgetreten. Das ist der klassische
///     2-fold-as-draw-Trick: ein rationaler Gegner wuerde die Wiederholung
///     vermeiden, wenn er gewinnen kann; wir duerfen den Teilbaum mit 0
///     abschneiden.
///   - Match an Index < `root_history_len`   → Treffer in der gespielten
///     Partie. Das alleine ist erst 2-fold ueber den ganzen Spielverlauf,
///     und FIDE verlangt 3-fold. Wir zaehlen weiter und brauchen ein zweites
///     Spielhistorie-Match (insgesamt 3-fold = aktuelle + 2 vorherige), um
///     wirklich 0 zurueckzugeben.
///
/// Beachte: der `key` der aktuellen Stellung wurde noch NICHT in `history`
/// gepusht — der Push passiert spaeter, vor dem Rekursionsaufruf der Kinder.
/// Daher ist die aktuelle Begegnung nicht in `history`, und ein einzelner
/// Match in der Spielhistorie bedeutet 2-fold (nicht 3-fold).
fn is_repetition_draw(history: &[u64], root_history_len: usize, key: u64) -> bool {
    let mut game_history_matches = 0;
    for (i, &h) in history.iter().enumerate().rev() {
        if h != key {
            continue;
        }
        if i >= root_history_len {
            // Wiederholung innerhalb des Suchpfads → 2-fold-as-draw.
            return true;
        }
        // Match in der Spielhistorie: brauchen ein zweites, um 3-fold zu erreichen.
        game_history_matches += 1;
        if game_history_matches >= 2 {
            return true;
        }
    }
    false
}

/// True, wenn `side` mindestens eine Leicht-/Schwerfigur (Springer, Laeufer,
/// Turm, Dame) besitzt. Dient als pragmatische Zugzwang-Heuristik fuer NMP:
/// in reinen Bauernendspielen (nur König und Bauern) ist die NMP-Annahme
/// "ein Zug zu machen ist mindestens so gut wie zu passen" oft falsch — dort
/// wird NMP deshalb deaktiviert. Deckt 95% der praktischen Zugzwang-Faelle ab.
fn has_non_pawn_material<B: EngineBoard>(board: &B, side: Color) -> bool {
    let side_bb = *board.color_combined(side);
    let non_pawns = *board.pieces(Piece::Knight)
        | *board.pieces(Piece::Bishop)
        | *board.pieces(Piece::Rook)
        | *board.pieces(Piece::Queen);
    (non_pawns & side_bb) != BitBoard::new(0)
}

#[inline]
fn is_quiet_queen_promotion<B: EngineBoard>(board: &B, mv: ChessMove) -> bool {
    mv.get_promotion() == Some(Piece::Queen) && !board.is_capture(mv)
}

/// Late-Move-Reductions-Stufenformel (Variante A).
///
/// Liefert den Reduktions-R-Wert in Plies. Der Aufrufer muss vorher
/// pruefen, ob LMR ueberhaupt erlaubt ist (siehe Vorbedingungen in
/// `alpha_beta`). Diese Funktion macht nur die reine Tabellen-
/// Entscheidung anhand von Tiefe und Move-Index.
///
/// Stufen:
///   - depth >= 6 und move_idx >= 6 → R = 2
///   - depth >= 3 und move_idx >= 3 → R = 1
///   - alles andere                 → R = 0
fn lmr_reduction(depth: i32, move_idx: usize) -> i32 {
    if depth >= 6 && move_idx >= 6 {
        2
    } else if depth >= LMR_MIN_DEPTH && move_idx >= LMR_MIN_MOVE_INDEX {
        1
    } else {
        0
    }
}
/*
fn is_candidate_move
Wird seit 26.04.2026 nur noch für *nicht-Schach*-Kandidaten aufgerufen
(gewinnender Capture, erkanntes Endspiel, Freibauer). Schachgebote werden
am Callsite separat mit +1-Extension behandelt — Standard-Variante.
Diese Helfer geben +2-Extension für taktisch erzwingende Nicht-Schach-Züge.

Offene Idee (LMR): späte Quiet-Moves könnten reduziert statt extended werden,
um der wachsenden Suchbreite Herr zu werden. Wartet auf eigene Sitzung.
*/

fn is_candidate_move<B: EngineBoard>(
    board: &B,
    mv: ChessMove,
    new_board: &B,
    see_val: Option<i32>,
) -> bool {
    // Der Aufrufer ruft diesen Helfer nur fuer Nicht-Schachzuege auf. Das
    // Debug-Assert dokumentiert die Vorbedingung ohne Release-Takte fuer einen
    // Zustand zu verbrennen, der im aktuellen Kontrollfluss nicht eintreten kann.
    debug_assert_eq!(new_board.checkers().popcnt(), 0);
    // Schlagzug: nur wenn SEE >= 0 (gewinnender oder ausgeglichener Tausch).
    // Verlierende Captures (SEE < 0) brauchen keine Extra-Tiefe — sie werden
    // in der Quiescence ohnehin abgeschnitten.
    // SEE-Wert ist gecachet aus order_moves; kein zweiter Aufruf mehr.
    if let Some(v) = see_val {
        return v >= 0;
    }
    if board.is_capture(mv) {
        return see(board, mv) >= 0;
    }
    // Bekanntes Endspiel: aggressiver verlaengern, damit lange Mattsequenzen
    // noch in die Suchtiefe passen.
    if endgame::is_recognized(new_board) {
        return true;
    }
    // Freibauerzug: der bewegte Bauer ist in der neuen Stellung Freibauer
    if board.piece_on(mv.get_source()) == Some(Piece::Pawn) {
        let us = board.side_to_move();
        let their_pawns = *new_board.pieces(Piece::Pawn) & *new_board.color_combined(!us);
        if crate::eval::is_passed(mv.get_dest(), us, their_pawns) {
            return true;
        }
    }
    false
}

/// Zug mit vorberechneten Sortier-/SEE-Informationen. `see_val` ist nur
/// bei Captures gesetzt und wird durch die Suche gereicht, damit SEE pro
/// Capture genau einmal berechnet wird (Ordering + Extension-Check teilen
/// sich das Ergebnis).
#[derive(Clone, Copy)]
struct ScoredMove {
    mv: ChessMove,
    order_key: i32,
    see_val: Option<i32>,
}

/// Lazy, gestaffelter Move-Picker — liefert dieselbe Zugreihenfolge wie der
/// frühere eager `order_moves`, aber stufenweise on demand. Erzeugt eine frühe
/// Stufe einen Beta-Cutoff (typisch der TT-Move an Cut-Nodes), entfallen die
/// SEE-Berechnung *aller* Captures und das Sortieren komplett — das ist der
/// NPS-Hebel. Bit-exakt zur alten Reihenfolge: dieselben order_key-Schienen,
/// dieselbe stabile Ordnung (MoveGen-Reihenfolge bei Gleichstand), dieselben
/// `see_val`-Werte und damit dieselben `move_idx` (→ SEE-Pruning/LMR unberührt).
///
/// Stufen in Ausgabereihenfolge (niedrigster order_key zuerst):
///   0  TT-Move                  -100_000
///   1  Dame-Umwandlung (still)   -50_000   ← VOR den Captures (−50k < −40k)!
///   2  gewinnender Capture       -40_000 + MVV/LVA   (SEE ≥ 0)
///   3  Killer 1                  -30_000
///   4  Killer 2                  -25_000
///   5  Unterumwandlung (still)   -20_000
///   6  ruhiger Zug              -history             (Range [-16_000, 0])
///   7  verlierender Capture      10_000 - SEE        (SEE < 0, ganz zuletzt)
///
/// Warum das bit-exakt bleibt:
///   - Captures werden in Stufe 2 *einmal* per SEE klassifiziert; verlierende
///     wandern in den `bad_captures`-Pool für Stufe 7. `see(board, mv)` ist
///     eine reine Funktion der (während des Knotens unveränderten) Stellung —
///     ob früh oder spät berechnet, der Wert ist derselbe.
///   - Die History-Scores der ruhigen Züge werden bereits in `new()` (= bei
///     Knoten-Eintritt) gelesen, BEVOR eine Kind-Suche die globale
///     History-Tabelle verändern kann. Nur das *Sortieren* der Quiets ist
///     verzögert. Damit ist die Quiet-Reihenfolge identisch zum eager-Stand.
///   - Die Klassifikation in `new()` folgt exakt der alten if-else-Priorität
///     (tt > capture > Dame-Umwandlung > Killer > Unterumwandlung > quiet);
///     jeder Zug landet in genau einer Kategorie → kein Doppel-Ausgeben.
struct MovePicker<'a, B: EngineBoard> {
    board: &'a B,
    /// aktuelle Stufe (0..=8); 8 = erschöpft
    stage: u8,
    /// Index innerhalb der gerade ausgegebenen Stufenliste
    cursor: usize,

    tt: Option<ScoredMove>,
    queen_promos: Vec<ChessMove>,
    /// alle Captures in MoveGen-Reihenfolge; SEE folgt erst in Stufe 2
    captures: Vec<ChessMove>,
    killer1: Option<ChessMove>,
    killer2: Option<ChessMove>,
    under_promos: Vec<ChessMove>,
    /// ruhige Züge mit bereits in `new()` gelesenem History-Score (unsortiert)
    quiets: Vec<ScoredMove>,

    good_captures: Vec<ScoredMove>,
    bad_captures: Vec<ScoredMove>,
    captures_done: bool,
    quiets_sorted: bool,
}

impl<'a, B: EngineBoard> MovePicker<'a, B> {
    fn new(
        board: &'a B,
        moves: B::Gen,
        tt_move: Option<ChessMove>,
        killers: [Option<ChessMove>; 2],
        move_history: &[i32],
    ) -> Self {
        let stm = board.side_to_move();
        let mut tt = None;
        let mut queen_promos = Vec::new();
        let mut captures = Vec::new();
        let mut killer1 = None;
        let mut killer2 = None;
        let mut under_promos = Vec::new();
        let mut quiets = Vec::new();

        // Einmalige Klassifikation in MoveGen-Reihenfolge. Die Zweig-Reihenfolge
        // ist exakt die der alten `order_moves`-Prioritätskette. SEE wird hier
        // NICHT berechnet (außer für einen schlagenden TT-Move, der ohnehin als
        // Erster gesucht wird) — die teure Capture-SEE folgt erst in Stufe 2.
        // `move_history` wird nur hier gelesen (Knoten-Eintritt) und NICHT
        // gespeichert; danach hält der Picker keinen State-Borrow mehr.
        for mv in moves {
            if Some(mv) == tt_move {
                let see_val = if board.is_capture(mv) {
                    Some(see(board, mv))
                } else {
                    None
                };
                tt = Some(ScoredMove {
                    mv,
                    order_key: -100_000,
                    see_val,
                });
            } else if board.is_capture(mv) {
                captures.push(mv);
            } else if mv.get_promotion() == Some(Piece::Queen) {
                queen_promos.push(mv);
            } else if Some(mv) == killers[0] {
                killer1 = Some(mv);
            } else if Some(mv) == killers[1] {
                killer2 = Some(mv);
            } else if mv.get_promotion().is_some() {
                under_promos.push(mv);
            } else {
                let h = move_history[history_idx(stm, mv.get_source(), mv.get_dest())];
                quiets.push(ScoredMove {
                    mv,
                    order_key: -h,
                    see_val: None,
                });
            }
        }

        MovePicker {
            board,
            stage: 0,
            cursor: 0,
            tt,
            queen_promos,
            captures,
            killer1,
            killer2,
            under_promos,
            quiets,
            good_captures: Vec::new(),
            bad_captures: Vec::new(),
            captures_done: false,
            quiets_sorted: false,
        }
    }

    /// Stufe 2: SEE für alle Captures berechnen, in gewinnend (SEE ≥ 0) und
    /// verlierend (SEE < 0) aufteilen und jeweils stabil nach order_key
    /// sortieren. Wird höchstens einmal aufgerufen — und nur, wenn die Suche
    /// Stufe 2 überhaupt erreicht (sonst bleibt die ganze SEE-Arbeit aus).
    fn classify_captures(&mut self) {
        for i in 0..self.captures.len() {
            let mv = self.captures[i];
            let v = see(self.board, mv);
            if v >= 0 {
                let order_key = -40_000 + mvv_lva_key(self.board, mv);
                self.good_captures.push(ScoredMove {
                    mv,
                    order_key,
                    see_val: Some(v),
                });
            } else {
                self.bad_captures.push(ScoredMove {
                    mv,
                    order_key: 10_000 - v,
                    see_val: Some(v),
                });
            }
        }
        // sort_by_key ist stabil → MoveGen-Reihenfolge bei Gleichstand, wie der
        // frühere einzelne Gesamt-Sort.
        self.good_captures.sort_by_key(|sm| sm.order_key);
        self.bad_captures.sort_by_key(|sm| sm.order_key);
    }
}

impl<'a, B: EngineBoard> Iterator for MovePicker<'a, B> {
    type Item = ScoredMove;

    fn next(&mut self) -> Option<ScoredMove> {
        loop {
            match self.stage {
                // Stufe 0 — TT-Move
                0 => {
                    self.stage = 1;
                    if let Some(sm) = self.tt.take() {
                        return Some(sm);
                    }
                }
                // Stufe 1 — stille Dame-Umwandlungen
                1 => {
                    if self.cursor < self.queen_promos.len() {
                        let mv = self.queen_promos[self.cursor];
                        self.cursor += 1;
                        return Some(ScoredMove {
                            mv,
                            order_key: -50_000,
                            see_val: None,
                        });
                    }
                    self.cursor = 0;
                    self.stage = 2;
                }
                // Stufe 2 — gewinnende Captures (klassifiziert hier ALLE Captures)
                2 => {
                    if !self.captures_done {
                        self.classify_captures();
                        self.captures_done = true;
                    }
                    if self.cursor < self.good_captures.len() {
                        let sm = self.good_captures[self.cursor];
                        self.cursor += 1;
                        return Some(sm);
                    }
                    self.cursor = 0;
                    self.stage = 3;
                }
                // Stufe 3 — Killer 1
                3 => {
                    self.stage = 4;
                    if let Some(mv) = self.killer1.take() {
                        return Some(ScoredMove {
                            mv,
                            order_key: -30_000,
                            see_val: None,
                        });
                    }
                }
                // Stufe 4 — Killer 2
                4 => {
                    self.stage = 5;
                    if let Some(mv) = self.killer2.take() {
                        return Some(ScoredMove {
                            mv,
                            order_key: -25_000,
                            see_val: None,
                        });
                    }
                }
                // Stufe 5 — stille Unterumwandlungen
                5 => {
                    if self.cursor < self.under_promos.len() {
                        let mv = self.under_promos[self.cursor];
                        self.cursor += 1;
                        return Some(ScoredMove {
                            mv,
                            order_key: -20_000,
                            see_val: None,
                        });
                    }
                    self.cursor = 0;
                    self.stage = 6;
                }
                // Stufe 6 — ruhige Züge (History-Score in new() gelesen)
                6 => {
                    if !self.quiets_sorted {
                        self.quiets.sort_by_key(|sm| sm.order_key);
                        self.quiets_sorted = true;
                    }
                    if self.cursor < self.quiets.len() {
                        let sm = self.quiets[self.cursor];
                        self.cursor += 1;
                        return Some(sm);
                    }
                    self.cursor = 0;
                    self.stage = 7;
                }
                // Stufe 7 — verlierende Captures (in Stufe 2 vorbereitet)
                7 => {
                    if self.cursor < self.bad_captures.len() {
                        let sm = self.bad_captures[self.cursor];
                        self.cursor += 1;
                        return Some(sm);
                    }
                    self.stage = 8;
                }
                _ => return None,
            }
        }
    }
}

fn mvv_lva_key<B: EngineBoard>(board: &B, mv: ChessMove) -> i32 {
    let target = board.piece_on(mv.get_dest()).map(piece_rank).unwrap_or(1); // en passant schlaegt einen Bauern
    let attacker = board.piece_on(mv.get_source()).map(piece_rank).unwrap_or(0);
    // Hoher Target-Wert, niedriger Attacker-Wert → niedrigster Key
    -(target * 10 - attacker)
}

fn piece_rank(p: Piece) -> i32 {
    match p {
        Piece::Pawn => 1,
        Piece::Knight => 3,
        Piece::Bishop => 3,
        Piece::Rook => 5,
        Piece::Queen => 9,
        Piece::King => 100,
    }
}

// ---------------------------------------------------------------------------
// SEE — Static Exchange Evaluation
// ---------------------------------------------------------------------------
//
// Simuliert eine Schlagserie auf einem einzelnen Feld und liefert den
// Materialgewinn/-verlust aus Sicht der Seite, die den ersten Schlag macht.
//
// Wird genutzt für:
// - Bad Capture Pruning in der Quiescence-Suche
// - Move Ordering (verlierende Captures hinter Quiet Moves)
// - Selektive Extensions (nur gewinnende Captures extenden)

/// Materialwert einer Figur für SEE (unabhängig von EvalParams, damit SEE
/// keine Referenz auf die Eval braucht und schnell bleibt).
#[inline]
fn see_piece_value(p: Piece) -> i32 {
    match p {
        Piece::Pawn => 100,
        Piece::Knight => 300,
        Piece::Bishop => 300,
        Piece::Rook => 500,
        Piece::Queen => 900,
        Piece::King => 100_000,
    }
}

/// Alle Figuren (beider Seiten), die `target` angreifen, gegeben das
/// aktuelle `occupied`-Bitboard. Gleiter werden korrekt berechnet, sodass
/// X-Ray-Angriffe nach Entfernung einer Figur automatisch auftauchen.
fn all_attackers_to<B: EngineBoard>(board: &B, target: Square, occupied: BitBoard) -> BitBoard {
    use chess::{get_bishop_moves, get_king_moves, get_knight_moves, get_rook_moves};

    let knights = *board.pieces(Piece::Knight) & occupied;
    let bishops_queens = (*board.pieces(Piece::Bishop) | *board.pieces(Piece::Queen)) & occupied;
    let rooks_queens = (*board.pieces(Piece::Rook) | *board.pieces(Piece::Queen)) & occupied;
    let kings = *board.pieces(Piece::King) & occupied;

    let mut attackers = BitBoard::new(0);

    // Springer
    attackers |= get_knight_moves(target) & knights;
    // Läufer + Dame (diagonal)
    attackers |= get_bishop_moves(target, occupied) & bishops_queens;
    // Türme + Dame (gerade)
    attackers |= get_rook_moves(target, occupied) & rooks_queens;
    // König
    attackers |= get_king_moves(target) & kings;

    // Bauern: "wer greift target an?" ist äquivalent zu "von target rückwärts
    // schauen" — ein weißer Bauer auf sq greift target an, wenn target in den
    // Angriffsfeldern von sq liegt. Das ist dasselbe wie: sq liegt in den
    // Angriffsfeldern eines *schwarzen* Bauern auf target (gespiegelte Richtung).
    let white_pawns = *board.pieces(Piece::Pawn) & *board.color_combined(Color::White) & occupied;
    let black_pawns = *board.pieces(Piece::Pawn) & *board.color_combined(Color::Black) & occupied;
    attackers |= chess::get_pawn_attacks(target, Color::Black, white_pawns);
    attackers |= chess::get_pawn_attacks(target, Color::White, black_pawns);

    attackers
}

/// Billigsten Angreifer einer Seite aus dem `attackers`-Bitboard finden.
/// Gibt (Square, Piece, Wert) zurück.
fn least_valuable_attacker<B: EngineBoard>(
    board: &B,
    attackers: BitBoard,
    side: Color,
    occupied: BitBoard,
) -> Option<(Square, Piece, i32)> {
    let side_attackers = attackers & *board.color_combined(side) & occupied;
    // Reihenfolge: Bauer, Springer, Läufer, Turm, Dame, König
    for &piece in &[
        Piece::Pawn,
        Piece::Knight,
        Piece::Bishop,
        Piece::Rook,
        Piece::Queen,
        Piece::King,
    ] {
        let candidates = side_attackers & *board.pieces(piece);
        if candidates != BitBoard::new(0) {
            // Nimm irgendeinen (to_square liefert den niedrigsten)
            let sq = candidates.to_square();
            return Some((sq, piece, see_piece_value(piece)));
        }
    }
    None
}

/// Static Exchange Evaluation: liefert den Materialgewinn/-verlust für den
/// Schlagzug `mv` aus Sicht der Seite am Zug.
///
/// Positiver Wert = der Schlagzug gewinnt Material.
/// Negativer Wert = der Schlagzug verliert Material.
///
/// Der Algorithmus baut ein Gain-Array auf (wer gewinnt was in jedem Schritt)
/// und faltet es am Ende per Minimax zurück: jede Seite wählt das Maximum aus
/// "aufhören" und "weiterschlagen".
pub fn see<B: EngineBoard>(board: &B, mv: ChessMove) -> i32 {
    let target = mv.get_dest();
    let source = mv.get_source();
    let mover = board.side_to_move();

    // Figur, die geschlagen wird (en passant: Bauer)
    let captured_piece = board.piece_on(target).unwrap_or(Piece::Pawn);
    // Figur, die schlägt
    let moving_piece = board.piece_on(source).unwrap_or(Piece::Pawn);

    // Promotion: die schlagende Figur wird zur beförderten Figur
    let moving_value = if let Some(promo) = mv.get_promotion() {
        see_piece_value(promo)
    } else {
        see_piece_value(moving_piece)
    };

    // Gain-Array: gain[d] = was die Seite im Schritt d gewinnt (vor Rückschlag)
    let mut gain: [i32; 33] = [0; 33];
    gain[0] = see_piece_value(captured_piece);
    if mv.get_promotion().is_some() {
        // Bei Promotion gewinnen wir zusätzlich die Differenz Promo-Bauer
        gain[0] += see_piece_value(mv.get_promotion().unwrap()) - see_piece_value(Piece::Pawn);
    }

    // Occupied-Bitboard: Quellfigur entfernen (sie steht jetzt auf target)
    let mut occupied = *board.combined() ^ BitBoard::from_square(source);

    // En passant: geschlagener Bauer steht nicht auf target
    if board.piece_on(source) == Some(Piece::Pawn)
        && board.piece_on(target).is_none()
        && source.get_file() != target.get_file()
    {
        // En-passant-Capture: der geschlagene Bauer steht auf derselben Spalte
        // wie target, aber auf der Reihe der Quelle
        let ep_square = Square::make_square(source.get_rank(), target.get_file());
        occupied ^= BitBoard::from_square(ep_square);
    }

    // Alle Angreifer auf target (aktualisiert sich, wenn Figuren entfernt werden)
    let mut attackers = all_attackers_to(board, target, occupied);

    // Angreifer, den wir gerade bewegt haben, ist nicht mehr auf source
    attackers &= occupied;

    let mut side = !mover; // Gegenseite ist als Nächstes dran
    let mut current_value = moving_value; // Wert der Figur, die gerade auf target steht
    let mut depth = 0;

    loop {
        // Erst prüfen, ob die Seite überhaupt einen Angreifer hat — sonst entsteht
        // ein Phantom-Eintrag in gain[], der alle Werte invertiert.
        let Some((att_sq, _att_piece, att_value)) =
            least_valuable_attacker(board, attackers, side, occupied)
        else {
            break; // Kein Angreifer mehr → fertig
        };

        depth += 1;
        // Seite gewinnt die Figur auf target, riskiert dabei aber current_value.
        gain[depth] = current_value - gain[depth - 1];

        // Angreifer entfernen → deckt ggf. Gleiter dahinter auf (X-Ray)
        occupied ^= BitBoard::from_square(att_sq);
        attackers = all_attackers_to(board, target, occupied) & occupied;

        current_value = att_value;
        side = !side;

        if depth >= 32 {
            break;
        }
    }

    // Minimax rückwärts: jede Seite wählt max(aufhören, weiterschlagen)
    while depth > 0 {
        gain[depth - 1] = -((-gain[depth - 1]).max(gain[depth]));
        depth -= 1;
    }

    gain[0]
}

/// Static Exchange Evaluation für einen NICHT-schlagenden Zug (2C-Filter für
/// stille Schachgebote): Materialsaldo, wenn die eigene Figur auf das leere
/// Zielfeld zieht und der Gegner dort den Abtausch eröffnet. `>= 0` heißt: die
/// Figur steht auf dem Zielfeld sicher und geht per statischem Abtausch nicht
/// verloren. Spiegelt `see()` exakt, nur mit `gain[0] = 0` (es wird nichts
/// geschlagen) und der Gegenseite zuerst am Zug.
fn see_quiet<B: EngineBoard>(board: &B, mv: ChessMove) -> i32 {
    let to = mv.get_dest();
    let src = mv.get_source();
    let mover = board.side_to_move();
    let moving_piece = board.piece_on(src).unwrap_or(Piece::Pawn);

    // Quellfigur steht jetzt auf `to`; `src` wird frei und deckt dabei ggf.
    // einen dahinterstehenden gegnerischen Gleiter auf (X-Ray korrekt).
    let mut occupied = *board.combined() ^ BitBoard::from_square(src);
    let mut attackers = all_attackers_to(board, to, occupied) & occupied;

    let mut gain: [i32; 33] = [0; 33]; // gain[0] = 0: der stille Zug schlägt nichts
    let mut side = !mover; // der Gegner eröffnet den Abtausch auf `to`
    let mut current_value = see_piece_value(moving_piece);
    let mut depth = 0;

    loop {
        let Some((att_sq, _att_piece, att_value)) =
            least_valuable_attacker(board, attackers, side, occupied)
        else {
            break;
        };

        depth += 1;
        gain[depth] = current_value - gain[depth - 1];

        occupied ^= BitBoard::from_square(att_sq);
        attackers = all_attackers_to(board, to, occupied) & occupied;

        current_value = att_value;
        side = !side;

        if depth >= 32 {
            break;
        }
    }

    while depth > 0 {
        gain[depth - 1] = -((-gain[depth - 1]).max(gain[depth]));
        depth -= 1;
    }

    gain[0]
}

fn calculate_think_time(params: &GoParams, move_overhead: u64, stm: Color) -> Duration {
    if let Some(movetime) = params.movetime {
        let ms = movetime.saturating_sub(move_overhead).max(1);
        return Duration::from_millis(ms);
    }

    let (time, inc) = match stm {
        Color::White => (params.wtime, params.winc),
        Color::Black => (params.btime, params.binc),
    };

    let remaining = time.unwrap_or(30_000);
    let increment = inc.unwrap_or(0);

    // ~1/30 der verbleibenden Zeit + 80% des Inkrements, minus Overhead,
    // gedeckelt auf "verbleibende Zeit minus Sicherheitsabstand".
    let budget = remaining / 30 + (increment * 8 / 10);
    let budget = budget.saturating_sub(move_overhead).max(50);
    let ceiling = remaining.saturating_sub(50).max(50);
    Duration::from_millis(budget.min(ceiling))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chess::{Board, MoveGen};

    // Repetition-Helper: pruefe die vier relevanten Faelle einzeln.

    #[test]
    fn repetition_no_match_is_not_draw() {
        // Saubere Stellung — kein Hash gleicht `key`.
        let history = vec![0xAAAA, 0xBBBB, 0xCCCC];
        assert!(!is_repetition_draw(&history, 1, 0x1234));
    }

    #[test]
    fn repetition_one_game_history_match_is_not_draw() {
        // Match nur in der Spielhistorie (Index < root_history_len).
        // FIDE braucht 3-fold — ein einzelnes Match alleine ergibt 0
        // (key + 1 vorheriges Vorkommen = 2-fold) und ist KEIN Remis.
        let history = vec![0xDEAD, 0xBEEF, 0xCAFE];
        let root = 3; // alle Eintraege sind Spielhistorie
        assert!(!is_repetition_draw(&history, root, 0xBEEF));
    }

    #[test]
    fn repetition_two_game_history_matches_is_draw() {
        // Position war schon 2x in der Partie — die aktuelle Begegnung ist die
        // dritte → 3-fold-Remis.
        let history = vec![0xBEEFu64, 0xCAFE, 0xBEEF, 0xF00D];
        let root = 4;
        assert!(is_repetition_draw(&history, root, 0xBEEF));
    }

    #[test]
    fn repetition_match_in_search_path_is_draw() {
        // Match liegt im Suchpfad (Index >= root_history_len) → klassischer
        // 2-fold-as-draw-Trick: einmaliges Wiedersehen reicht.
        // root_history_len=2 → Indizes 2,3 sind Suchpfad.
        let history = vec![0xAAAA, 0xBBBB, 0xCCCC, 0xDDDD];
        let root = 2;
        assert!(is_repetition_draw(&history, root, 0xCCCC));
    }

    #[test]
    fn repetition_match_in_game_only_is_not_draw_even_if_root_zero() {
        // Edge-Case: leere Spielhistorie zum Suchstart (root=0). Dann ist
        // jeder Match per Definition im Suchpfad → 2-fold-as-draw.
        let history = vec![0x1111, 0x2222, 0x3333];
        let root = 0;
        assert!(is_repetition_draw(&history, root, 0x2222));
    }

    #[test]
    fn repetition_recent_search_path_match_short_circuits() {
        // Selbst wenn weiter hinten in der Spielhistorie ein Match waere,
        // ein Suchpfad-Match (rueckwaerts zuerst gefunden) loest sofort aus.
        let history = vec![0xAAAA, 0xBBBB, 0xAAAA];
        let root = 1; // Eintrag 0 = Spielhistorie, 1+2 = Suchpfad
        assert!(is_repetition_draw(&history, root, 0xAAAA));
    }

    // --- TT-Repetition-Vergiftung ---------------------------------------
    //
    // Regression auf den Bug aus Partie PGQZhMjF (06.05.2026): TT speichert
    // nur den Brett-Hash, nicht den Repetition-Kontext. Wenn dieselbe
    // Stellung im Spielverlauf wiederkehrt, kann ein gespeicherter
    // Mate-Score von einer frueheren Begegnung stale sein. Der Fix in der
    // TT-Probe unterdrueckt den Cutoff in genau diesem Fall.
    //
    // Setup unten: Anfangsstellung. Wir vergiften den TT-Eintrag fuer die
    // Folgestellung nach 1. a3 mit einem absurden Mate-Score (-29000 aus
    // Schwarz' Sicht = Schwarz steht auf Verlust, aus Weiss' Sicht +29000
    // = Weiss gewinnt). Zusaetzlich legen wir den Hash dieser Folgestellung
    // in die Spielhistorie — als waere sie schon einmal aufgetreten.
    //
    // Erwartung:
    //   - Ohne Fix: TT-Cutoff feuert in Iteration depth=2, Wurzel sieht
    //     fuer 1. a3 einen Mate-Score, MATE_THRESHOLD-Break, bestmove=a3.
    //   - Mit Fix: TT-Cutoff blockiert (key in game history), echte Suche
    //     liefert realistischen Score nahe 0 fuer a3, andere Wurzelzuege
    //     (e4, d4, …) gewinnen das Move-Ordering. bestmove != a3.

    use crate::polyglot::BookSet;
    use std::path::Path;
    use std::str::FromStr;
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};

    fn poisoned_tt_setup() -> (Board, Vec<u64>, Arc<Mutex<TranspositionTable>>, ChessMove) {
        let start = Board::default();
        let a3 = ChessMove::from_san(&start, "a3").expect("a3 ist in der Anfangsstellung legal");
        let after_a3 = start.make_move_new(a3);
        let key_after_a3 = after_a3.get_hash();

        let tt = Arc::new(Mutex::new(TranspositionTable::new(1)));
        // -29000 aus Schwarz' Sicht (Schwarz steht "kurz vor Mate"). Negiert
        // an der Wurzel ergibt +29000 fuer Weiss → vergifteter Mate-Score.
        // Tiefe absurd hoch, damit der TT-Cutoff garantiert greift.
        tt.lock()
            .unwrap()
            .store(key_after_a3, None, -29_000, 99, TtFlag::Exact);

        // Spielhistorie enthaelt genau diesen Hash (1-fold game-history).
        let history = vec![key_after_a3];
        (start, history, tt, a3)
    }

    fn run_search(
        board: Board,
        history: Vec<u64>,
        tt: Arc<Mutex<TranspositionTable>>,
    ) -> ChessMove {
        let req = SearchRequest {
            board,
            history,
            halfmove_clock: 0,
            params: GoParams {
                depth: Some(2),
                movetime: Some(2_000),
                ..GoParams::default()
            },
            tt,
            book: Arc::new(BookSet::load(Path::new("."), &[])),
            eval: Arc::new(EvalParams::default()),
            stop: Arc::new(AtomicBool::new(false)),
            pondering: Arc::new(AtomicBool::new(false)),
            move_overhead: 0,
            syzygy: None,
        };
        search(req).expect("Suche liefert ein Ergebnis").best
    }

    #[test]
    fn tt_cutoff_suppressed_when_key_in_game_history() {
        // Wir simulieren manuell die Wirkung des Fixes auf einer Helper-Ebene:
        // verifiziere, dass der `contains`-Check, den die TT-Probe nutzt,
        // genau dann positiv wird, wenn die Stellung in der Spielhistorie
        // (Index < root_history_len) schon einmal aufgetreten ist.
        let (start, history, _tt, a3) = poisoned_tt_setup();
        let after_a3 = start.make_move_new(a3);
        let key_after_a3 = after_a3.get_hash();

        let root_history_len = history.len();
        // Der Fix prueft genau diesen Slice — Position muss in der
        // Spielhistorie auftauchen, sonst ist die Vergiftung wirkungslos.
        assert!(history[..root_history_len].contains(&key_after_a3));
        // Wurzelposition selbst ist NICHT in der Spielhistorie — TT-Cutoff
        // fuer die Wurzel waere also weiterhin erlaubt.
        let key_start = start.get_hash();
        assert!(!history[..root_history_len].contains(&key_start));
    }

    #[test]
    fn poisoned_tt_does_not_select_repeated_move() {
        // Echter Verhaltens-Test: bei vergifteter TT + Repetition-Match
        // darf die Engine den vergifteten Zug NICHT waehlen.
        let (start, history, tt, a3) = poisoned_tt_setup();
        let best = run_search(start, history, tt);
        assert_ne!(
            best, a3,
            "Engine ist auf vergifteten TT-Score reingefallen — der Fix in der \
             TT-Probe greift nicht. Stellung: Anfangsbrett, vergifteter Eintrag \
             auf Folgeposition nach 1. a3."
        );
    }

    // --- TT-Mate-Ply-Adjustment (Bug 10.06.2026) --------------------------
    //
    // Regression auf die verschenkten Mop-up-Gewinne (zKfpQEn8 u. a.):
    // Mate-Scores wurden roh (wurzelrelativ) in der TT abgelegt. Spaetere
    // Suchen bekamen damit fossile Mattdistanzen serviert — die gemeldete
    // Distanz schrumpfte von Zug zu Zug nie, die Engine schob Dame/Turm
    // bis zum 50-Zuege-/3-fold-Remis. Details: docs/roadmap.md (10.06.).

    #[test]
    fn mate_score_tt_normierung_roundtrip() {
        // Matt in 2 Plies ab einem Knoten bei ply=3, aus Sicht der dortigen
        // Seite am Zug: wurzelrelativ MATE-(3+2). Knotenrelativ muss MATE-2
        // gespeichert werden — unabhaengig von der Wurzel.
        assert_eq!(mate_score_to_tt(MATE - 5, 3), MATE - 2);
        // Gelesen von einer ANDEREN Suche, deren Knoten bei ply=7 liegt:
        // Matt in 2 ab Knoten = Matt in 9 ab deren Wurzel.
        assert_eq!(mate_score_from_tt(MATE - 2, 7), MATE - 9);
        // Spiegelbild fuer "wird selbst matt gesetzt" (negative Codierung).
        assert_eq!(mate_score_to_tt(-(MATE - 5), 3), -(MATE - 2));
        assert_eq!(mate_score_from_tt(-(MATE - 2), 7), -(MATE - 9));
        // Normale Scores passieren unveraendert.
        assert_eq!(mate_score_to_tt(123, 9), 123);
        assert_eq!(mate_score_from_tt(-450, 9), -450);
        // Roundtrip am selben Knoten ist die Identitaet.
        assert_eq!(mate_score_from_tt(mate_score_to_tt(MATE - 11, 4), 4), MATE - 11);
    }

    fn run_search_scored(
        board: Board,
        tt: Arc<Mutex<TranspositionTable>>,
        depth: u32,
    ) -> SearchResult {
        let req = SearchRequest {
            board,
            history: Vec::new(),
            halfmove_clock: 0,
            params: GoParams {
                depth: Some(depth),
                movetime: Some(10_000),
                ..GoParams::default()
            },
            tt,
            book: Arc::new(BookSet::load(Path::new("."), &[])),
            eval: Arc::new(EvalParams::default()),
            stop: Arc::new(AtomicBool::new(false)),
            pondering: Arc::new(AtomicBool::new(false)),
            move_overhead: 0,
            syzygy: None,
        };
        search(req).expect("Suche liefert ein Ergebnis")
    }

    #[test]
    fn tt_mate_distance_shrinks_across_searches() {
        // KQvK (Figurensatz aus der Live-Partie zKfpQEn8, Weiss am Zug).
        // Ablauf wie live: erst eine Suche, die das Matt findet und die TT
        // fuellt; dann zwei Plies weiterruecken (bester Zug + erwartete
        // Verteidigung) und mit DERSELBEN TT erneut suchen. Die gemeldete
        // Mattdistanz MUSS jetzt kleiner sein — vor dem Ply-Adjustment
        // blieb sie stehen oder wuchs (live: 12 -> 14 -> 15 -> 15 ...).
        let p0 = Board::from_str("6Q1/8/8/5K2/8/4k3/8/8 w - - 0 1")
            .expect("FEN ist gueltig");
        let tt = Arc::new(Mutex::new(TranspositionTable::new(16)));

        let r0 = run_search_scored(p0, Arc::clone(&tt), 16);
        assert!(
            r0.score > MATE_THRESHOLD,
            "Erste Suche muss das KQvK-Matt sehen, Score war {}",
            r0.score
        );
        let dist0 = MATE - r0.score;

        // Zwei Plies entlang der erwarteten Linie weiterruecken. Faellt der
        // Pondermove aus (kein TT-Eintrag), tut es jeder legale Zug — auch
        // gegen suboptimale Verteidigung muss die Distanz schrumpfen.
        let p_after_best = p0.make_move_new(r0.best);
        let reply = r0.ponder.unwrap_or_else(|| {
            MoveGen::new_legal(&p_after_best)
                .next()
                .expect("Verteidiger hat einen legalen Zug")
        });
        let p1 = p_after_best.make_move_new(reply);

        let r1 = run_search_scored(p1, Arc::clone(&tt), 16);
        assert!(
            r1.score > MATE_THRESHOLD,
            "Zweite Suche muss das Matt weiterhin sehen, Score war {}",
            r1.score
        );
        let dist1 = MATE - r1.score;
        assert!(
            dist1 < dist0,
            "Mattdistanz schrumpft nicht ({} -> {} Plies): TT liefert \
             fossile Mate-Scores — Ply-Adjustment defekt.",
            dist0,
            dist1
        );
    }
}
