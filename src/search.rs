use crate::endgame;
use crate::eval::evaluate;
use crate::eval_config::EvalParams;
use crate::polyglot::hash::polyglot_hash;
use crate::polyglot::BookSet;
use crate::position::move_to_uci;
use crate::tt::{TranspositionTable, TtFlag};
use chess::{BitBoard, Board, BoardStatus, ChessMove, Color, MoveGen, Piece, Square};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const INF: i32 = 1_000_000;
const MATE: i32 = 100_000;
const MATE_THRESHOLD: i32 = MATE - 1000;
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
}

pub struct SearchRequest {
    pub board: Board,
    pub history: Vec<u64>,
    pub halfmove_clock: u8,
    pub params: GoParams,
    pub tt: Arc<Mutex<TranspositionTable>>,
    pub book: Arc<BookSet>,
    pub eval: Arc<EvalParams>,
    pub stop: Arc<AtomicBool>,
    pub pondering: Arc<AtomicBool>,
    pub move_overhead: u64,
}

struct SearchState {
    tt: Arc<Mutex<TranspositionTable>>,
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
    root_best_score: i32,
    // Wenn die Wurzel nur einen legalen Zug hat, merken wir ihn vor: beim
    // Uebergang Ponder → Normal koennen wir dann sofort abbrechen.
    forced_only_move: Option<ChessMove>,
    // Killer Moves: pro ply zwei Quiet-Züge, die zuletzt einen Beta-Cutoff
    // erzeugt haben. Werden in der Sortierung direkt hinter gewinnenden
    // Captures einsortiert.
    killers: [[Option<ChessMove>; 2]; MAX_PLY],
    // History-Heuristic: [side][from*64 + to]. Jedes Mal, wenn ein Quiet-Zug
    // einen Beta-Cutoff produziert, wird `depth*depth` aufaddiert (geclampt
    // auf MAX_HISTORY). Quiet Moves werden innerhalb ihres Ordering-Bands
    // nach dem History-Score absteigend sortiert.
    move_history: Vec<i32>,
}

fn history_idx(side: Color, from: Square, to: Square) -> usize {
    let side_idx = match side {
        Color::White => 0,
        Color::Black => 1,
    };
    side_idx * 64 * 64 + from.to_index() * 64 + to.to_index()
}

impl SearchState {
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

pub fn search(req: SearchRequest) -> Option<SearchResult> {
    if req.board.status() != BoardStatus::Ongoing {
        return None;
    }

    // Eroeffnungsbuch zuerst — auch im Ponder-Modus erlaubt
    if !req.book.is_empty() {
        if let Some(m) = req.book.probe(&req.board) {
            println!("info string book hit");
            let ponder = ponder_move_from_tt(&req.board, m, &req.tt);
            return Some(SearchResult { best: m, ponder });
        }
    }

    // Forcierter Zug: nur eine legale Antwort → ohne Suche spielen.
    // Im Ponder-Modus muessen wir weiterdenken, bis ponderhit/stop kommt,
    // deshalb nur im normalen Modus kurzschliessen.
    if !req.params.ponder {
        let mut legal = MoveGen::new_legal(&req.board);
        if let Some(only) = legal.next() {
            if legal.next().is_none() {
                println!("info string forced move");
                let ponder = ponder_move_from_tt(&req.board, only, &req.tt);
                return Some(SearchResult { best: only, ponder });
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
    let forced_only_move = {
        let mut it = MoveGen::new_legal(&req.board);
        let first = it.next();
        match (first, it.next()) {
            (Some(m), None) => Some(m),
            _ => None,
        }
    };

    let history = req.history;
    let root_history_len = history.len();
    let mut state = SearchState {
        tt: Arc::clone(&req.tt),
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
        root_best_score: 0,
        forced_only_move,
        killers: [[None; 2]; MAX_PLY],
        move_history: vec![0; 2 * 64 * 64],
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

        emit_info(depth, score, state.nodes, state.start.elapsed(), last_move);

        // Gefundenes Matt: nicht weitersuchen
        if score.abs() > MATE_THRESHOLD {
            break;
        }
    }

    if completed_depth == 0 {
        // Not a single iteration finished — spiele den ersten legalen Zug
        last_move = MoveGen::new_legal(&req.board).next();
        println!(
            "info string fallback (no completed depth, nodes={})",
            state.nodes
        );
    }

    let _ = last_score; // unused in final output for now
    last_move.map(|best| {
        let ponder = ponder_move_from_tt(&req.board, best, &req.tt);
        SearchResult { best, ponder }
    })
}

/// Sucht einen Pondermove: Mache den besten Zug, schaue in der TT nach,
/// welcher Zug fuer die Antwortstellung gespeichert ist. Verifiziere
/// Legalitaet, falls die TT-Position eine Kollision war.
fn ponder_move_from_tt(
    board: &Board,
    best: ChessMove,
    tt: &Arc<Mutex<TranspositionTable>>,
) -> Option<ChessMove> {
    let next = board.make_move_new(best);
    if next.status() != BoardStatus::Ongoing {
        return None;
    }
    let key = polyglot_hash(&next);
    let stored = {
        let tt = tt.lock().unwrap();
        tt.probe(key).and_then(|e| e.best_move)
    };
    let mv = stored?;
    if MoveGen::new_legal(&next).any(|m| m == mv) {
        Some(mv)
    } else {
        None
    }
}

fn emit_info(depth: i32, score: i32, nodes: u64, elapsed: Duration, best: Option<ChessMove>) {
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
    println!(
        "info depth {depth} score {score_str} nodes {nodes} time {ms} nps {nps} pv {pv}"
    );
}

fn alpha_beta(
    board: &Board,
    depth: i32,
    ply: i32,
    mut alpha: i32,
    beta: i32,
    extensions_used: i32,
    halfmove: u8,
    allow_null: bool,
    state: &mut SearchState,
) -> i32 {
    state.nodes += 1;

    if state.should_stop() {
        return 0;
    }

    let key = polyglot_hash(board);

    // Stellungswiederholung und 50-Zuege-Regel
    if ply > 0 {
        if is_repetition_draw(&state.history, state.root_history_len, key) {
            return 0;
        }
        if halfmove >= 100 {
            return 0;
        }
    }

    // Terminalstellungen
    match board.status() {
        BoardStatus::Checkmate => return -MATE + ply,
        BoardStatus::Stalemate => return 0,
        BoardStatus::Ongoing => {}
    }

    // Blattknoten: Quiescence-Suche
    if depth <= 0 {
        return quiescence(board, alpha, beta, ply, state);
    }

    // Transposition Table Probe
    let tt_move: Option<ChessMove>;
    {
        let tt = state.tt.lock().unwrap();
        if let Some(entry) = tt.probe(key) {
            if entry.depth as i32 >= depth && ply > 0 {
                let v = entry.eval;
                match entry.flag {
                    TtFlag::Exact => return v,
                    TtFlag::Lower if v >= beta => return v,
                    TtFlag::Upper if v <= alpha => return v,
                    _ => {}
                }
            }
            tt_move = entry.best_move;
        } else {
            tt_move = None;
        }
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
    if allow_null
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

    // Zuege generieren + ordnen (mit SEE-Cache für Captures, Killer + History)
    let moves: Vec<ChessMove> = MoveGen::new_legal(board).collect();
    if moves.is_empty() {
        return 0;
    }
    let killers_here = state.killers_at(ply);
    let ordered = order_moves(board, moves, tt_move, killers_here, &state.move_history);

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

    for (move_idx, sm) in ordered.iter().enumerate() {
        let mv = sm.mv;
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
        let other_cand = !child_in_check && is_candidate_move(board, mv, &nb, sm.see_val);
        let check_ext = if crate::eval::game_phase(&nb) < 16 { 2 } else { 1 };
        let ext = if other_cand && extensions_used + 2 <= MAX_EXTENSION_PER_LINE {
            2
        } else if child_in_check && extensions_used + check_ext <= MAX_EXTENSION_PER_LINE {
            check_ext
        } else {
            0
        };
        let new_depth = depth - 1 + ext;
        let new_halfmove = if is_irreversible(board, mv) {
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

        first_move = false;

        if score > best_score {
            best_score = score;
            best_move = Some(mv);
            if ply == 0 {
                state.root_best_move = Some(mv);
                state.root_best_score = score;
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
    {
        let mut tt = state.tt.lock().unwrap();
        tt.store(key, best_move, best_score, depth as i8, flag);
    }

    best_score
}
// Maximale Quiescence-Tiefe: begrenzt Explosion bei vielen Captures.
const MAX_QPLY: i32 = 12;
// Delta-Pruning-Margin: ein Capture muss mindestens diesen Betrag über alpha
// liegen können, sonst ist er hoffnungslos (verhindert nutzlose Suche).
// Auf 150 reduziert (war 200): missed_capture-Rate war nach SEE-Einführung
// gestiegen, weil 200cp gute Captures fälschlicherweise prunte.
const DELTA_MARGIN: i32 = 150;

fn quiescence(
    board: &Board,
    mut alpha: i32,
    beta: i32,
    ply: i32,
    state: &mut SearchState,
) -> i32 {
    state.nodes += 1;

    if state.should_stop() {
        return 0;
    }

    match board.status() {
        BoardStatus::Checkmate => return -MATE + ply,
        BoardStatus::Stalemate => return 0,
        BoardStatus::Ongoing => {}
    }

    let in_check = board.checkers().popcnt() > 0;

    if in_check {
        // Im Schach: alle legalen Züge durchsuchen, kein Stand-Pat.
        // Stand-Pat wäre falsch, weil die Seite nicht einfach "passen" kann.
        // Tiefenlimit gilt nicht im Schach — sonst würden Matt-Drohungen übersehen.
        let moves: Vec<ChessMove> = MoveGen::new_legal(board).collect();

        let mut best = -INF;
        for mv in moves {
            let nb = board.make_move_new(mv);
            let score = -quiescence(&nb, -beta, -alpha, ply + 1, state);

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
    if ply >= MAX_QPLY {
        return stand_pat;
    }

    // Nur Schlagzuege generieren (inkl. en passant), SEE einmal pro Zug.
    // Sortierung nach SEE absteigend: beste Captures zuerst → frühere Cutoffs.
    let mut captures: Vec<(ChessMove, i32)> = MoveGen::new_legal(board)
        .filter(|mv| is_capture(board, *mv))
        .map(|mv| {
            let v = see(board, mv);
            (mv, v)
        })
        .collect();
    captures.sort_by_key(|(_, v)| -*v);

    for (mv, see_val) in captures {
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

        let nb = board.make_move_new(mv);
        let score = -quiescence(&nb, -beta, -alpha, ply + 1, state);

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

fn eval_stm(board: &Board, params: &EvalParams) -> i32 {
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
fn has_non_pawn_material(board: &Board, side: Color) -> bool {
    let side_bb = *board.color_combined(side);
    for &p in &[Piece::Knight, Piece::Bishop, Piece::Rook, Piece::Queen] {
        if (*board.pieces(p) & side_bb) != BitBoard::new(0) {
            return true;
        }
    }
    false
}

fn is_capture(board: &Board, mv: ChessMove) -> bool {
    if board.piece_on(mv.get_dest()).is_some() {
        return true;
    }
    // en passant
    if board.piece_on(mv.get_source()) == Some(Piece::Pawn)
        && mv.get_source().get_file() != mv.get_dest().get_file()
        && board.piece_on(mv.get_dest()).is_none()
    {
        return true;
    }
    false
}

fn is_irreversible(board: &Board, mv: ChessMove) -> bool {
    is_capture(board, mv) || board.piece_on(mv.get_source()) == Some(Piece::Pawn)
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


fn is_candidate_move(
    board: &Board,
    mv: ChessMove,
    new_board: &Board,
    see_val: Option<i32>,
) -> bool {
    // Defensiv: falls jemand diesen Helfer doch mal mit einem Schachzug
    // füttert, soll er nicht "kein Kandidat" sagen — gleiche Semantik wie
    // vorher behalten. Im Hauptpfad wird das aber durch in_check abgefangen.
    if new_board.checkers().popcnt() > 0 {
        return true;
    }
    // Schlagzug: nur wenn SEE >= 0 (gewinnender oder ausgeglichener Tausch).
    // Verlierende Captures (SEE < 0) brauchen keine Extra-Tiefe — sie werden
    // in der Quiescence ohnehin abgeschnitten.
    // SEE-Wert ist gecachet aus order_moves; kein zweiter Aufruf mehr.
    if let Some(v) = see_val {
        return v >= 0;
    }
    if is_capture(board, mv) {
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
        if is_passed_simple(mv.get_dest(), us, their_pawns) {
            return true;
        }
    }
    false
}

fn is_passed_simple(sq: chess::Square, us: Color, their_pawns: chess::BitBoard) -> bool {
    use chess::{BitBoard, File, Rank, Square};
    let file_idx = sq.get_file().to_index() as i32;
    let rank_idx = sq.get_rank().to_index() as i32;

    for r in 0..8 {
        let ahead = match us {
            Color::White => r > rank_idx,
            Color::Black => r < rank_idx,
        };
        if !ahead {
            continue;
        }
        for df in [-1i32, 0, 1] {
            let f = file_idx + df;
            if !(0..8).contains(&f) {
                continue;
            }
            let check_sq = Square::make_square(
                Rank::from_index(r as usize),
                File::from_index(f as usize),
            );
            if (their_pawns & BitBoard::from_square(check_sq)) != BitBoard::new(0) {
                return false;
            }
        }
    }
    true
}

/// Zug mit vorberechneten Sortier-/SEE-Informationen. `see_val` ist nur
/// bei Captures gesetzt und wird durch die Suche gereicht, damit SEE pro
/// Capture genau einmal berechnet wird (Ordering + Extension-Check teilen
/// sich das Ergebnis).
struct ScoredMove {
    mv: ChessMove,
    order_key: i32,
    see_val: Option<i32>,
}

/// Sortier-Schlüssel (niedrig = zuerst):
///   TT-Move:                 -100_000
///   Promotion zu Dame:        -50_000
///   Gewinnender Capture:      -40_000 + MVV/LVA
///   Killer 1:                 -30_000
///   Killer 2:                 -25_000
///   Unterpromotion:           -20_000
///   Quiet Move (History):     -history                (Range [-16_000, 0])
///   Verlierender Capture:     +10_000 - SEE           (stark negative zuletzt)
fn order_moves(
    board: &Board,
    moves: Vec<ChessMove>,
    tt_move: Option<ChessMove>,
    killers: [Option<ChessMove>; 2],
    move_history: &[i32],
) -> Vec<ScoredMove> {
    let stm = board.side_to_move();
    let mut scored: Vec<ScoredMove> = moves
        .into_iter()
        .map(|mv| {
            if Some(mv) == tt_move {
                return ScoredMove {
                    mv,
                    order_key: -100_000,
                    see_val: if is_capture(board, mv) { Some(see(board, mv)) } else { None },
                };
            }
            if is_capture(board, mv) {
                let v = see(board, mv);
                let order_key = if v >= 0 {
                    -40_000 + mvv_lva_key(board, mv)
                } else {
                    10_000 - v
                };
                return ScoredMove { mv, order_key, see_val: Some(v) };
            }
            if mv.get_promotion() == Some(Piece::Queen) {
                return ScoredMove { mv, order_key: -50_000, see_val: None };
            }
            if Some(mv) == killers[0] {
                return ScoredMove { mv, order_key: -30_000, see_val: None };
            }
            if Some(mv) == killers[1] {
                return ScoredMove { mv, order_key: -25_000, see_val: None };
            }
            if mv.get_promotion().is_some() {
                return ScoredMove { mv, order_key: -20_000, see_val: None };
            }
            let h = move_history[history_idx(stm, mv.get_source(), mv.get_dest())];
            ScoredMove { mv, order_key: -h, see_val: None }
        })
        .collect();
    scored.sort_by_key(|sm| sm.order_key);
    scored
}

fn mvv_lva_key(board: &Board, mv: ChessMove) -> i32 {
    let target = board
        .piece_on(mv.get_dest())
        .map(piece_rank)
        .unwrap_or(1); // en passant schlaegt einen Bauern
    let attacker = board
        .piece_on(mv.get_source())
        .map(piece_rank)
        .unwrap_or(0);
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
fn all_attackers_to(board: &Board, target: Square, occupied: BitBoard) -> BitBoard {
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
fn least_valuable_attacker(
    board: &Board,
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
        if candidates.popcnt() > 0 {
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
pub fn see(board: &Board, mv: ChessMove) -> i32 {
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
}
