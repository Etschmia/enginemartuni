# Martuni — UCI Chess Engine

**Martuni** is a UCI-compatible chess engine written in Rust, developed from scratch by **Tobias Brendler**.

The engine is self-developed: all search logic, evaluation, and strategy are Tobias's own work. No engine code was copied from existing engines. External libraries are used only for board representation and legal move generation (the [`chess`](https://crates.io/crates/chess) crate by Jordan Bray).

---

## Play Martuni on Lichess

You can challenge the bot directly — no installation needed:

**[lichess.org/@/Martuni](https://lichess.org/@/Martuni)**

The bot accepts challenges in Blitz (3+0, 5+0) and Rapid (10+5, 15+10).

---

## Features

### Search
- **Alpha-Beta** with iterative deepening
- **Quiescence Search** — avoids horizon-effect blunders by resolving captures at leaf nodes
- **Transposition Table** — Zobrist-hashed, avoids re-searching known positions
- **Check Extensions** — extends search depth when the king is in check
- **Pondering** — thinks on the opponent's time (`go ponder` / `ponderhit`)

### Evaluation
- **Material** counting
- **Piece-Square Tables** with **Tapered Evaluation** — smoothly interpolates between midgame and endgame scores based on material left on the board
- **King Safety** — evaluates the 3×3 zone around the king, attacker weights, and pawn shield
- **Endgame Heuristics** — specialized scoring for pawn endgames, rook activity, trapped rooks, and king-pawn synergy

### Opening Book
Martuni supports **Polyglot opening books** (`.bin` format). Books are read from the directory set by `BOOK_DIR` and consulted in the priority order defined by `BOOK_FILES` — the first book that contains a move for the current position wins. Book lookups also happen during pondering.

See [Configuration](#configuration) below for details.

### Configuration
Martuni reads a `.env` file on startup (searched in the working directory, binary directory, and project root — in that order). A documented template is provided as `.env.example`.

| Variable | Description | Default |
|----------|-------------|---------|
| `HASH_SIZE_MB` | Transposition table size in MB | `64` |
| `BOOK_DIR` | Directory containing Polyglot `.bin` book files | `src/polyglot` |
| `BOOK_FILES` | Comma-separated list of book filenames in priority order | _(none)_ |

UCI options set via `setoption` (Hash, MoveOverhead, Ponder) override `.env` values at runtime.

Opening books are **not included** in this repository. Place your own Polyglot-format `.bin` files into `BOOK_DIR` and list them in `BOOK_FILES`. The engine plays without a book if no files are found.

### UCI Compliance
Martuni implements the full UCI protocol, including:
- `uci`, `isready`, `ucinewgame`, `position`, `go`, `stop`, `quit`
- `go ponder` / `ponderhit`
- `setoption` for Hash, MoveOverhead, Ponder

It works with any UCI-compatible GUI: [Arena](http://www.playwitharena.de/), [Cute Chess](https://cutechess.com/), [Lucas Chess](https://lucaschess.pythonanywhere.com/), [BanksiaGUI](https://banksiagui.com/), etc.

---

## Roadmap

Features currently planned or in development:

- **Null-Move Pruning** — prune branches where even passing a move leads to beta cutoff
- **Late Move Reductions (LMR)** — reduce search depth for moves that are unlikely to be best

---

## Prerequisites

- **Rust** (edition 2021, Rust 1.70 or newer recommended)
  Install via [rustup.rs](https://rustup.rs/)
- No other system dependencies — the `chess` crate is pure Rust

---

## Building

Clone the repository and build the release binary with Cargo:

```bash
git clone https://github.com/Etschmia/martuni.git
cd martuni
cargo build --release
```

The binary ends up at:

| Platform | Path |
|----------|------|
| Linux / macOS | `./target/release/martuni` |
| Windows | `target\release\martuni.exe` |

### Quick smoke test

```bash
echo -e "uci\nisready\nposition startpos\ngo movetime 1000\nquit" | ./target/release/martuni
```

### Opening books

Copy `.env.example` to `.env` and adjust `BOOK_DIR` and `BOOK_FILES` to point to your own Polyglot `.bin` files. The engine plays fine without any books.

---

## Using with a GUI

1. Build the binary (see above).
2. In your GUI, add a new UCI engine and point it to the `martuni` / `martuni.exe` binary.
3. Optional: set the `Hash` option (default: 64 MB) and enable `Ponder` if the GUI supports it.

---

## License & Attribution

The source code is **open** — you are welcome to read it, fork it, and build on it.

**Condition:** if you use or adapt this code, please credit **Tobias Brendler** as the original author in your README or about page.

There is no formal open-source license attached yet; the above condition is the only requirement.

---

## Author

**Tobias Brendler**
