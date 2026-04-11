use crate::config::Config;
use crate::eval_config::EvalParams;
use crate::options::EngineOptions;
use crate::polyglot::BookSet;
use crate::position::{move_to_uci, Position};
use crate::search::{search, GoParams, SearchRequest};
use crate::tt::TranspositionTable;
use std::io::{self, BufRead};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

pub fn uci_loop() {
    let cfg = Config::load();
    let book = Arc::new(BookSet::load(&cfg.book_dir, &cfg.book_files));
    let eval_params = Arc::new(EvalParams::load());
    let tt = Arc::new(Mutex::new(TranspositionTable::new(cfg.hash_size_mb)));

    let mut position = Position::new();
    let mut options = EngineOptions::from_config(&cfg);
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
                    options.set_option(&name, &value);
                    if options.hash != old_hash {
                        let mut t = tt.lock().unwrap();
                        t.resize(options.hash as usize);
                        println!(
                            "info string hash resized to {} MB",
                            t.size_mb()
                        );
                    }
                }
            }
            "ucinewgame" => {
                position.set_startpos();
                tt.lock().unwrap().clear();
            }
            "position" => {
                handle_position(&mut position, &tokens);
            }
            "go" => {
                if let Some(h) = search_handle.take() {
                    let _ = h.join();
                }

                stop.store(false, Ordering::Relaxed);
                let params = parse_go_params(&tokens);
                pondering.store(params.ponder, Ordering::Relaxed);

                let req = SearchRequest {
                    board: *position.board(),
                    history: position.hash_history().to_vec(),
                    halfmove_clock: position.halfmove_clock(),
                    params,
                    tt: Arc::clone(&tt),
                    book: Arc::clone(&book),
                    eval: Arc::clone(&eval_params),
                    stop: Arc::clone(&stop),
                    pondering: Arc::clone(&pondering),
                    move_overhead: options.move_overhead,
                };

                search_handle = Some(thread::spawn(move || {
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
                }));
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

fn handle_position(position: &mut Position, tokens: &[&str]) {
    if tokens.len() < 2 {
        return;
    }

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
            if position.set_fen(&fen).is_err() {
                return;
            }
            if i < tokens.len() && tokens[i] == "moves" { i + 1 } else { 0 }
        }
        _ => return,
    };

    if move_start > 0 && move_start < tokens.len() {
        let moves: Vec<&str> = tokens[move_start..].to_vec();
        let _ = position.apply_moves(&moves);
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
