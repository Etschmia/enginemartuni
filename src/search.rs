use crate::eval::evaluate;
use crate::eval_config::EvalParams;
use crate::polyglot::hash::polyglot_hash;
use crate::polyglot::BookSet;
use crate::position::move_to_uci;
use crate::tt::{TranspositionTable, TtFlag};
use chess::{Board, BoardStatus, ChessMove, Color, MoveGen, Piece};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const INF: i32 = 1_000_000;
const MATE: i32 = 100_000;
const MATE_THRESHOLD: i32 = MATE - 1000;
const MAX_EXTENSION_PER_LINE: i32 = 6;
const MAX_DEPTH: i32 = 64;

pub struct GoParams {
    pub wtime: Option<u64>,
    pub btime: Option<u64>,
    pub winc: Option<u64>,
    pub binc: Option<u64>,
    pub depth: Option<u32>,
    pub movetime: Option<u64>,
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
        }
    }
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
    pub move_overhead: u64,
}

struct SearchState {
    tt: Arc<Mutex<TranspositionTable>>,
    eval: Arc<EvalParams>,
    stop: Arc<AtomicBool>,
    deadline: Instant,
    start: Instant,
    nodes: u64,
    // Historie + aktueller Suchpfad; zum Erkennen von Stellungswiederholungen
    history: Vec<u64>,
    root_best_move: Option<ChessMove>,
    root_best_score: i32,
}

impl SearchState {
    fn should_stop(&mut self) -> bool {
        if self.stop.load(Ordering::Relaxed) {
            return true;
        }
        if self.nodes & 2047 == 0 && Instant::now() >= self.deadline {
            self.stop.store(true, Ordering::Relaxed);
            return true;
        }
        false
    }
}

pub fn search(req: SearchRequest) -> Option<ChessMove> {
    if req.board.status() != BoardStatus::Ongoing {
        return None;
    }

    // Eroeffnungsbuch zuerst
    if !req.book.is_empty() {
        if let Some(m) = req.book.probe(&req.board) {
            println!("info string book hit");
            return Some(m);
        }
    }

    let start = Instant::now();
    let think_time = calculate_think_time(&req.params, req.move_overhead, req.board.side_to_move());
    let deadline = start + think_time;

    let mut state = SearchState {
        tt: Arc::clone(&req.tt),
        eval: Arc::clone(&req.eval),
        stop: Arc::clone(&req.stop),
        deadline,
        start,
        nodes: 0,
        history: req.history,
        root_best_move: None,
        root_best_score: 0,
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
    last_move
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
    state: &mut SearchState,
) -> i32 {
    state.nodes += 1;

    if state.should_stop() {
        return 0;
    }

    let key = polyglot_hash(board);

    // Stellungswiederholung und 50-Zuege-Regel
    if ply > 0 {
        if state.history.contains(&key) {
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

    // Zuege generieren + ordnen
    let moves: Vec<ChessMove> = MoveGen::new_legal(board).collect();
    if moves.is_empty() {
        return 0;
    }
    let ordered = order_moves(board, moves, tt_move);

    // Eigenen Hash fuer die Kinder in die Historie legen
    if ply > 0 {
        state.history.push(key);
    }

    let orig_alpha = alpha;
    let mut best_score = -INF;
    let mut best_move: Option<ChessMove> = None;
    let mut aborted = false;

    for mv in &ordered {
        let nb = board.make_move_new(*mv);
        let is_cand = is_candidate_move(board, *mv, &nb);
        let ext = if is_cand && extensions_used + 2 <= MAX_EXTENSION_PER_LINE {
            2
        } else {
            0
        };
        let new_depth = depth - 1 + ext;
        let new_halfmove = if is_irreversible(board, *mv) {
            0
        } else {
            halfmove.saturating_add(1)
        };

        let score = -alpha_beta(
            &nb,
            new_depth,
            ply + 1,
            -beta,
            -alpha,
            extensions_used + ext,
            new_halfmove,
            state,
        );

        if state.stop.load(Ordering::Relaxed) {
            aborted = true;
            break;
        }

        if score > best_score {
            best_score = score;
            best_move = Some(*mv);
            if ply == 0 {
                state.root_best_move = Some(*mv);
                state.root_best_score = score;
            }
        }
        if score > alpha {
            alpha = score;
        }
        if alpha >= beta {
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

    // Stand pat (statischer Score aus Sicht der Seite am Zug)
    let stand_pat = eval_stm(board, &state.eval);
    if stand_pat >= beta {
        return stand_pat;
    }
    if stand_pat > alpha {
        alpha = stand_pat;
    }

    // Nur Schlagzuege generieren (inkl. en passant)
    let mut captures: Vec<ChessMove> = MoveGen::new_legal(board)
        .filter(|mv| is_capture(board, *mv))
        .collect();
    captures.sort_by_key(|mv| mvv_lva_key(board, *mv));

    for mv in captures {
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

fn is_candidate_move(board: &Board, mv: ChessMove, new_board: &Board) -> bool {
    // Schachgebot
    if new_board.checkers().popcnt() > 0 {
        return true;
    }
    // Schlagzug
    if is_capture(board, mv) {
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

fn order_moves(
    board: &Board,
    mut moves: Vec<ChessMove>,
    tt_move: Option<ChessMove>,
) -> Vec<ChessMove> {
    moves.sort_by_key(|mv| {
        if Some(*mv) == tt_move {
            return -10_000;
        }
        // MVV-LVA fuer Schlagzuege
        if is_capture(board, *mv) {
            return mvv_lva_key(board, *mv);
        }
        // Befoerderungen
        if mv.get_promotion().is_some() {
            return -500;
        }
        0
    });
    moves
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
