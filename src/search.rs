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

pub fn search(board: &Board, params: &GoParams, stop: Arc<AtomicBool>) -> Option<ChessMove> {
    if board.status() != BoardStatus::Ongoing {
        return None;
    }

    let mut rng = rand::thread_rng();
    let legal_moves = MoveGen::new_legal(board);
    let chosen = legal_moves.into_iter().choose(&mut rng)?;

    // Simulate thinking time (unless depth-based or already stopped)
    if params.depth.is_none() && !stop.load(Ordering::Relaxed) {
        let think_ms = calculate_think_time(params);
        // Sleep in small increments so we can react to stop
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
        // Use 80% of allocated time, max 2 seconds
        return (movetime * 80 / 100).min(2000);
    }

    // Time-based: use remaining_time / 40, max 2 seconds
    let remaining = params.wtime.unwrap_or(30000).max(params.btime.unwrap_or(30000));
    (remaining / 40).min(2000).max(100)
}
