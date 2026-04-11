mod config;
mod eval;
mod eval_config;
mod options;
mod polyglot;
mod position;
mod pst;
mod search;
mod tt;
mod uci;

fn main() {
    uci::uci_loop();
}
