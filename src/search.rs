use crate::polyglot::BookSet;
use chess::{Board, BoardStatus, ChessMove, MoveGen};
use rand::seq::IteratorRandom;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

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

pub fn search(
    board: &Board,
    params: &GoParams,
    stop: Arc<AtomicBool>,
    book: Arc<BookSet>,
) -> Option<ChessMove> {
    if board.status() != BoardStatus::Ongoing {
        return None;
    }

    // Eroeffnungsbuch zuerst — Treffer wird sofort zurueckgegeben.
    if !book.is_empty() {
        if let Some(book_move) = book.probe(board) {
            println!("info string book hit");
            return Some(book_move);
        }
    }

    let mut rng = rand::thread_rng();
    let legal_moves = MoveGen::new_legal(board);
    let chosen = legal_moves.into_iter().choose(&mut rng)?;

    if params.depth.is_none() && !stop.load(Ordering::Relaxed) {
        let think_ms = calculate_think_time(params);
        let steps = (think_ms / 50).max(1);
        for _ in 0..steps {
            if stop.load(Ordering::Relaxed) {
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
    }

    Some(chosen)
}

fn calculate_think_time(params: &GoParams) -> u64 {
    if let Some(movetime) = params.movetime {
        return (movetime * 80 / 100).min(2000);
    }

    let remaining = params.wtime.unwrap_or(30000).max(params.btime.unwrap_or(30000));
    (remaining / 40).min(2000).max(100)
}
