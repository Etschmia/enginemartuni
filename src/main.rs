mod backend;
mod board_atomic;
mod board_crazyhouse;
mod board960;
mod config;
mod endgame;
mod eval;
mod eval_config;
mod options;
mod polyglot;
mod position;
mod pst;
mod search;
mod syzygy;
mod tt;
mod uci;

fn main() {
    uci::uci_loop();
}
