mod config;
mod options;
mod polyglot;
mod position;
mod search;
mod tt;
mod uci;

fn main() {
    uci::uci_loop();
}
