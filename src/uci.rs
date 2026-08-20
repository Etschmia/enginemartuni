use crate::backend::EngineBoard;
use crate::board_atomic::BoardAtomic;
use crate::board960::Board960;
use crate::config::Config;
use crate::eval_config::EvalParams;
use crate::options::{EngineOptions, UciVariant};
use crate::polyglot::BookSet;
use crate::position::{move_to_uci, Position};
use crate::search::{search, GoParams, SearchRequest};
use crate::syzygy::Syzygy;
use crate::tt::TranspositionTable;
use chess::Board;
use std::io::{self, BufRead};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

pub fn uci_loop() {
    let cfg = Config::load();
    let book = Arc::new(BookSet::load(&cfg.book_dir, &cfg.book_files));
    let eval_params = Arc::new(EvalParams::load());
    let tt = Arc::new(Mutex::new(TranspositionTable::new(cfg.hash_size_mb)));

    // Aktives Spiel je nach Modus: Standard, Chess960 oder Atomic.
    // Umschalten via `UCI_Chess960` bzw. `UCI_Variant`.
    let mut position = GamePos::Std(Position::new());
    let mut options = EngineOptions::from_config(&cfg);
    // Tablebase-Handle: None solange kein SyzygyPath gesetzt ist (→ Engine
    // verhält sich exakt wie ohne Tablebases). Wird bei setoption SyzygyPath
    // neu geladen. Arc, damit es billig in den Such-Thread geklont werden kann.
    let mut syzygy: Option<Arc<Syzygy>> = load_syzygy(&options.syzygy_path);
    let stop = Arc::new(AtomicBool::new(false));
    let pondering = Arc::new(AtomicBool::new(false));
    let mut search_handle: Option<thread::JoinHandle<()>> = None;

    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        let tokens: Vec<&str> = line.split_whitespace().collect();
        if tokens.is_empty() {
            continue;
        }

        match tokens[0] {
            "uci" => {
                println!("id name Martuni");
                println!("id author Tobias Brendler");
                options.print_uci_options();
                println!("uciok");
            }
            "isready" => {
                if let Some(h) = search_handle.take() {
                    let _ = h.join();
                }
                println!("readyok");
            }
            "setoption" => {
                if let Some((name, value)) = parse_setoption(&tokens) {
                    let old_hash = options.hash;
                    let old_syzygy_path = options.syzygy_path.clone();
                    let old_960 = options.chess960;
                    let old_variant = options.variant;
                    options.set_option(&name, &value);
                    if options.chess960 != old_960 || options.variant != old_variant {
                        // Backend-Wechsel: Position auf Startstellung des neuen
                        // Modus zuruecksetzen (die konkrete Stellung kommt per
                        // `position`-Kommando ohnehin neu).
                        position = new_game_pos(&options);
                        // Gleiche Brettbelegung hat zwischen Regelvarianten
                        // denselben Zobrist-Key, aber nicht denselben Wert.
                        tt.lock().unwrap().clear();
                    }
                    if options.hash != old_hash {
                        let mut t = tt.lock().unwrap();
                        t.resize(options.hash as usize);
                        println!(
                            "info string hash resized to {} MB",
                            t.size_mb()
                        );
                    }
                    if options.syzygy_path != old_syzygy_path {
                        // Pfad geändert → Tablebases (neu) laden bzw. abschalten.
                        syzygy = load_syzygy(&options.syzygy_path);
                    }
                }
            }
            "ucinewgame" => {
                match &mut position {
                    GamePos::Std(p) => p.set_startpos(),
                    GamePos::Frc(p) => p.set_startpos(),
                    GamePos::Atomic(p) => p.set_startpos(),
                }
                tt.lock().unwrap().clear();
            }
            "position" => {
                match &mut position {
                    GamePos::Std(p) => handle_position(p, &tokens),
                    GamePos::Frc(p) => handle_position(p, &tokens),
                    GamePos::Atomic(p) => handle_position(p, &tokens),
                }
            }
            "go" => {
                if let Some(h) = search_handle.take() {
                    let _ = h.join();
                }

                stop.store(false, Ordering::Relaxed);
                let params = parse_go_params(&tokens);
                pondering.store(params.ponder, Ordering::Relaxed);

                search_handle = Some(match &position {
                    GamePos::Std(p) => spawn_search(
                        p, params, &tt, &book, &eval_params, &stop, &pondering,
                        options.move_overhead, &syzygy,
                    ),
                    GamePos::Frc(p) => spawn_search(
                        p, params, &tt, &book, &eval_params, &stop, &pondering,
                        options.move_overhead, &syzygy,
                    ),
                    GamePos::Atomic(p) => spawn_search(
                        p, params, &tt, &book, &eval_params, &stop, &pondering,
                        options.move_overhead, &syzygy,
                    ),
                });
            }
            "eval" => {
                // Debug-Kommando: druckt die komponentenweise Aufschluesselung
                // der statischen Bewertung der aktuell gesetzten Stellung.
                // Greift NICHT in die laufende Suche ein.
                match &position {
                    GamePos::Std(p) => {
                        crate::eval::print_eval_breakdown(p.board(), &eval_params)
                    }
                    GamePos::Frc(p) => {
                        crate::eval::print_eval_breakdown(p.board(), &eval_params)
                    }
                    GamePos::Atomic(p) => {
                        crate::eval::print_eval_breakdown(p.board(), &eval_params)
                    }
                }
            }
            "ponderhit" => {
                // Gegner hat den vorhergesagten Zug gespielt: aus dem Ponder-Modus
                // in normales Zeitmanagement umschalten. Die Suche erkennt den
                // Uebergang in should_stop() und setzt die echte Deadline.
                pondering.store(false, Ordering::Relaxed);
            }
            "stop" => {
                stop.store(true, Ordering::Relaxed);
                pondering.store(false, Ordering::Relaxed);
                if let Some(h) = search_handle.take() {
                    let _ = h.join();
                }
            }
            "quit" => {
                stop.store(true, Ordering::Relaxed);
                pondering.store(false, Ordering::Relaxed);
                if let Some(h) = search_handle.take() {
                    let _ = h.join();
                }
                return;
            }
            _ => {}
        }
    }

    stop.store(true, Ordering::Relaxed);
    pondering.store(false, Ordering::Relaxed);
    if let Some(h) = search_handle.take() {
        let _ = h.join();
    }
}

/// Lädt die Syzygy-Tablebases vom angegebenen Pfad und meldet das Ergebnis als
/// `info string`. Leerer Pfad oder Ladefehler → `None` (Engine arbeitet dann
/// ohne Tablebases weiter — kein Abbruch).
fn load_syzygy(path: &str) -> Option<Arc<Syzygy>> {
    if path.trim().is_empty() {
        return None;
    }
    match Syzygy::load(path) {
        Some(s) => {
            println!(
                "info string Syzygy: tablebases loaded (up to {} men) from {}",
                s.max_pieces(),
                path
            );
            Some(Arc::new(s))
        }
        None => {
            println!("info string Syzygy: no tablebases loaded from {}", path);
            None
        }
    }
}

/// Aktives Spiel: Standard-, Chess960- oder Atomic-Backend. Ein Enum statt eines
/// Trait-Objekts, weil die Suche generisch (monomorphisiert) laeuft und die
/// wenigen Dispatch-Stellen hier im UCI-Loop liegen.
enum GamePos {
    Std(Position<Board>),
    Frc(Position<Board960>),
    Atomic(Position<BoardAtomic>),
}

fn new_game_pos(options: &EngineOptions) -> GamePos {
    match (options.variant, options.chess960) {
        (UciVariant::Atomic, _) => GamePos::Atomic(Position::new()),
        (UciVariant::Chess, true) => GamePos::Frc(Position::new()),
        (UciVariant::Chess, false) => GamePos::Std(Position::new()),
    }
}

/// Startet den Such-Thread fuer ein beliebiges Backend.
#[allow(clippy::too_many_arguments)]
fn spawn_search<B: EngineBoard>(
    position: &Position<B>,
    params: GoParams,
    tt: &Arc<Mutex<TranspositionTable>>,
    book: &Arc<BookSet>,
    eval_params: &Arc<EvalParams>,
    stop: &Arc<AtomicBool>,
    pondering: &Arc<AtomicBool>,
    move_overhead: u64,
    syzygy: &Option<Arc<Syzygy>>,
) -> thread::JoinHandle<()> {
    let req = SearchRequest {
        board: position.board().clone(),
        history: position.hash_history().to_vec(),
        halfmove_clock: position.halfmove_clock(),
        params,
        tt: Arc::clone(tt),
        book: Arc::clone(book),
        eval: Arc::clone(eval_params),
        stop: Arc::clone(stop),
        pondering: Arc::clone(pondering),
        move_overhead,
        syzygy: syzygy.as_ref().map(Arc::clone),
    };

    thread::spawn(move || {
        if let Some(result) = search(req) {
            match result.ponder {
                Some(p) => println!(
                    "bestmove {} ponder {}",
                    move_to_uci(result.best),
                    move_to_uci(p)
                ),
                None => println!("bestmove {}", move_to_uci(result.best)),
            }
        } else {
            println!("bestmove 0000");
        }
    })
}

fn handle_position<B: EngineBoard>(position: &mut Position<B>, tokens: &[&str]) {
    if tokens.len() < 2 {
        return;
    }

    let previous = position.clone();
    let move_start = match tokens[1] {
        "startpos" => {
            position.set_startpos();
            if tokens.len() > 2 && tokens[2] == "moves" { 3 } else { 0 }
        }
        "fen" => {
            let mut fen_parts = Vec::new();
            let mut i = 2;
            while i < tokens.len() && tokens[i] != "moves" {
                fen_parts.push(tokens[i]);
                i += 1;
            }
            let fen = fen_parts.join(" ");
            if let Err(e) = position.set_fen(&fen) {
                println!("info string position error: {e}");
                *position = previous;
                return;
            }
            if i < tokens.len() && tokens[i] == "moves" { i + 1 } else { 0 }
        }
        _ => return,
    };

    if move_start > 0 && move_start < tokens.len() {
        let moves: Vec<&str> = tokens[move_start..].to_vec();
        if let Err(e) = position.apply_moves(&moves) {
            println!("info string position error: {e}");
            *position = previous;
        }
    }
}

fn parse_setoption(tokens: &[&str]) -> Option<(String, String)> {
    let mut name_parts = Vec::new();
    let mut value_parts = Vec::new();
    let mut in_value = false;
    let mut in_name = false;

    for &token in &tokens[1..] {
        if token == "name" && !in_value {
            in_name = true;
            continue;
        }
        if token == "value" {
            in_name = false;
            in_value = true;
            continue;
        }
        if in_name {
            name_parts.push(token);
        } else if in_value {
            value_parts.push(token);
        }
    }

    if name_parts.is_empty() {
        return None;
    }

    Some((name_parts.join(" "), value_parts.join(" ")))
}

fn parse_go_params(tokens: &[&str]) -> GoParams {
    let mut params = GoParams::default();
    let mut i = 1;
    while i < tokens.len() {
        match tokens[i] {
            "wtime" if i + 1 < tokens.len() => {
                params.wtime = tokens[i + 1].parse().ok();
                i += 2;
            }
            "btime" if i + 1 < tokens.len() => {
                params.btime = tokens[i + 1].parse().ok();
                i += 2;
            }
            "winc" if i + 1 < tokens.len() => {
                params.winc = tokens[i + 1].parse().ok();
                i += 2;
            }
            "binc" if i + 1 < tokens.len() => {
                params.binc = tokens[i + 1].parse().ok();
                i += 2;
            }
            "depth" if i + 1 < tokens.len() => {
                params.depth = tokens[i + 1].parse().ok();
                i += 2;
            }
            "movetime" if i + 1 < tokens.len() => {
                params.movetime = tokens[i + 1].parse().ok();
                i += 2;
            }
            "ponder" => {
                params.ponder = true;
                i += 1;
            }
            _ => { i += 1; }
        }
    }
    params
}
