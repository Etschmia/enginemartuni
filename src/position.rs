use crate::backend::EngineBoard;
use chess::{ChessMove, Piece};

/// Spielzustands-Tracker (UCI `position`-Kommando), generisch ueber das
/// Board-Backend: Standard-Schach (`chess::Board`) oder Chess960 (`Board960`).
/// FEN-/Zug-Parsing delegiert an das Backend, weil sich die Notation
/// unterscheidet (960: Shredder-FEN-Rochaderechte, Rochade als Koenig x Turm).
#[derive(Clone)]
pub struct Position<B: EngineBoard> {
    board: B,
    /// Hashes aller bisherigen Stellungen seit dem letzten irreversiblen
    /// Zug (Schlag oder Bauernzug). Wird fuer Stellungswiederholung genutzt.
    hash_history: Vec<u64>,
    /// Halbzug-Zaehler nach FEN — Anzahl Zuege seit letztem irreversiblem Zug.
    halfmove_clock: u8,
}

impl<B: EngineBoard> Position<B> {
    pub fn new() -> Self {
        let board = B::startpos();
        Self {
            hash_history: vec![board.get_hash()],
            halfmove_clock: 0,
            board,
        }
    }

    pub fn board(&self) -> &B {
        &self.board
    }

    pub fn hash_history(&self) -> &[u64] {
        &self.hash_history
    }

    pub fn halfmove_clock(&self) -> u8 {
        self.halfmove_clock
    }

    pub fn set_startpos(&mut self) {
        self.board = B::startpos();
        self.hash_history = vec![self.board.get_hash()];
        self.halfmove_clock = 0;
    }

    pub fn set_fen(&mut self, fen: &str) -> Result<(), String> {
        let board = B::from_fen(fen)?;
        let hmc = halfmove_clock_from_fen(fen);
        self.board = board;
        self.hash_history = vec![self.board.get_hash()];
        self.halfmove_clock = hmc;
        Ok(())
    }

    pub fn apply_moves(&mut self, moves: &[&str]) -> Result<(), String> {
        for uci_move in moves {
            let m = self.board.parse_uci_move(uci_move)?;
            let resets_halfmove = self.board.resets_halfmove(m);

            self.board = self.board.make_move_new(m);

            if resets_halfmove {
                // Irreversibler Zug — Historie kann geleert werden
                self.halfmove_clock = 0;
                self.hash_history.clear();
            } else {
                self.halfmove_clock = self.halfmove_clock.saturating_add(1);
            }
            self.hash_history.push(self.board.get_hash());
        }
        Ok(())
    }
}

/// Liest den Halbzugzaehler (50-Zuege-Regel) aus einer FEN.
///
/// Standard-FEN: Brett, Seite, Rochade, en passant, **Halbzugzaehler**,
/// Zugnummer — der Zaehler ist das fuenfte Feld. Three-Check schiebt aber
/// die verbleibenden Schachs davor (`... - - 3+3 0 1`, Lichess-Format);
/// die feste Position lieferte dort "3+3", das Parsen scheiterte und der
/// Zaehler wurde stillschweigend 0 (Code-Review 14.11.2026, #7). Deshalb:
/// ab dem fuenften Feld das erste nehmen, das eine reine Zahl ist. Das
/// aeltere Format mit angehaengtem Zaehler (`... - - 0 1 +0+0`) passt
/// ebenfalls, dort steht der Halbzugzaehler regulaer an fuenfter Stelle.
fn halfmove_clock_from_fen(fen: &str) -> u8 {
    fen.split_whitespace()
        .skip(4)
        .find(|s| s.bytes().all(|b| b.is_ascii_digit()))
        // Werte ueber 255 (ungueltig, die Regel greift bei 100) saettigen.
        .map(|s| s.parse::<u8>().unwrap_or(u8::MAX))
        .unwrap_or(0)
}

/// Zug → UCI-Text. Funktioniert fuer beide Backends: im 960-Backend ist die
/// Rochade als "Koenig x eigener Turm" codiert, was gedruckt genau der
/// UCI_Chess960-Notation entspricht (z. B. `e1h1`).
pub fn move_to_uci(m: ChessMove) -> String {
    // Crazyhouse-Drop-Encoding des gemeinsamen Zugtyps: Zielfeld steht in
    // Quelle und Ziel, die Drop-Figur im sonst fuer Promotions verwendeten
    // Feld. Kein legaler Brettzug kann Quelle == Ziel haben.
    if m.get_source() == m.get_dest() {
        if let Some(piece) = m.get_promotion() {
            let ch = match piece {
                Piece::Pawn => 'P',
                Piece::Knight => 'N',
                Piece::Bishop => 'B',
                Piece::Rook => 'R',
                Piece::Queen => 'Q',
                Piece::King => 'K',
            };
            return format!("{}@{}", ch, m.get_dest());
        }
    }
    let mut s = format!("{}{}", m.get_source(), m.get_dest());
    if let Some(promo) = m.get_promotion() {
        let ch = match promo {
            Piece::Queen => 'q',
            Piece::Rook => 'r',
            Piece::Bishop => 'b',
            Piece::Knight => 'n',
            // Antichess erlaubt die Umwandlung in einen Koenig (`e7e8k`).
            Piece::King => 'k',
            Piece::Pawn => 'q',
        };
        s.push(ch);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board_shak::BoardThreeCheck;
    use chess::Board;

    #[test]
    fn halfmove_clock_standard_fen() {
        assert_eq!(halfmove_clock_from_fen("7k/8/8/8/8/8/8/R3K3 w - - 99 1"), 99);
        assert_eq!(halfmove_clock_from_fen("7k/8/8/8/8/8/8/R3K3 w - - 0 1"), 0);
        // Verkuerzte FEN ohne Zaehler → 0.
        assert_eq!(halfmove_clock_from_fen("7k/8/8/8/8/8/8/R3K3 w - -"), 0);
    }

    #[test]
    fn halfmove_clock_three_check_fen() {
        // Lichess-Format: Schachzaehler VOR dem Halbzugzaehler.
        assert_eq!(halfmove_clock_from_fen("7k/8/8/8/8/8/8/R3K3 w - - 3+3 99 1"), 99);
        // Aelteres Format: Schachzaehler am Ende.
        assert_eq!(halfmove_clock_from_fen("7k/8/8/8/8/8/8/R3K3 w - - 42 1 +0+0"), 42);
    }

    #[test]
    fn set_fen_keeps_halfmove_clock_per_backend() {
        let mut std_pos: Position<Board> = Position::new();
        std_pos.set_fen("7k/8/8/8/8/8/8/R3K3 w - - 99 1").unwrap();
        assert_eq!(std_pos.halfmove_clock(), 99);

        let mut tc: Position<BoardThreeCheck> = Position::new();
        tc.set_fen("7k/8/8/8/8/8/8/R3K3 w - - 3+3 99 1").unwrap();
        assert_eq!(tc.halfmove_clock(), 99);
    }
}
