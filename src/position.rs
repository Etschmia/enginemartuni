use crate::polyglot::hash::polyglot_hash;
use chess::{Board, ChessMove, MoveGen, Piece, Square};
use std::str::FromStr;

pub struct Position {
    board: Board,
    /// Hashes aller bisherigen Stellungen seit dem letzten irreversiblen
    /// Zug (Schlag oder Bauernzug). Wird fuer Stellungswiederholung genutzt.
    hash_history: Vec<u64>,
    /// Halbzug-Zaehler nach FEN — Anzahl Zuege seit letztem irreversiblem Zug.
    halfmove_clock: u8,
}

impl Position {
    pub fn new() -> Self {
        let board = Board::default();
        Self {
            hash_history: vec![polyglot_hash(&board)],
            halfmove_clock: 0,
            board,
        }
    }

    pub fn board(&self) -> &Board {
        &self.board
    }

    pub fn hash_history(&self) -> &[u64] {
        &self.hash_history
    }

    pub fn halfmove_clock(&self) -> u8 {
        self.halfmove_clock
    }

    pub fn set_startpos(&mut self) {
        self.board = Board::default();
        self.hash_history = vec![polyglot_hash(&self.board)];
        self.halfmove_clock = 0;
    }

    pub fn set_fen(&mut self, fen: &str) -> Result<(), String> {
        let board = Board::from_str(fen).map_err(|e| format!("Invalid FEN: {}", e))?;
        let hmc = fen
            .split_whitespace()
            .nth(4)
            .and_then(|s| s.parse::<u8>().ok())
            .unwrap_or(0);
        self.board = board;
        self.hash_history = vec![polyglot_hash(&self.board)];
        self.halfmove_clock = hmc;
        Ok(())
    }

    pub fn apply_moves(&mut self, moves: &[&str]) -> Result<(), String> {
        for uci_move in moves {
            let m = parse_uci_move(&self.board, uci_move)?;
            let is_capture = self.board.piece_on(m.get_dest()).is_some()
                || is_en_passant(&self.board, m);
            let is_pawn_move = self.board.piece_on(m.get_source()) == Some(Piece::Pawn);

            self.board = self.board.make_move_new(m);

            if is_capture || is_pawn_move {
                // Irreversibler Zug — Historie kann geleert werden
                self.halfmove_clock = 0;
                self.hash_history.clear();
            } else {
                self.halfmove_clock = self.halfmove_clock.saturating_add(1);
            }
            self.hash_history.push(polyglot_hash(&self.board));
        }
        Ok(())
    }
}

fn is_en_passant(board: &Board, m: ChessMove) -> bool {
    board.piece_on(m.get_source()) == Some(Piece::Pawn)
        && m.get_source().get_file() != m.get_dest().get_file()
        && board.piece_on(m.get_dest()).is_none()
}

fn parse_uci_move(board: &Board, uci: &str) -> Result<ChessMove, String> {
    let uci = uci.trim();
    if uci.len() < 4 || uci.len() > 5 {
        return Err(format!("Invalid UCI move: {}", uci));
    }

    let from = Square::from_str(&uci[0..2])
        .map_err(|_| format!("Invalid source square: {}", &uci[0..2]))?;
    let to = Square::from_str(&uci[2..4])
        .map_err(|_| format!("Invalid target square: {}", &uci[2..4]))?;

    let promotion = if uci.len() == 5 {
        match uci.as_bytes()[4] {
            b'q' => Some(Piece::Queen),
            b'r' => Some(Piece::Rook),
            b'b' => Some(Piece::Bishop),
            b'n' => Some(Piece::Knight),
            c => return Err(format!("Invalid promotion piece: {}", c as char)),
        }
    } else {
        None
    };

    let candidate = ChessMove::new(from, to, promotion);
    for m in MoveGen::new_legal(board) {
        if m == candidate {
            return Ok(candidate);
        }
    }

    Err(format!("Illegal move: {}", uci))
}

pub fn move_to_uci(m: ChessMove) -> String {
    let mut s = format!("{}{}", m.get_source(), m.get_dest());
    if let Some(promo) = m.get_promotion() {
        let ch = match promo {
            Piece::Queen => 'q',
            Piece::Rook => 'r',
            Piece::Bishop => 'b',
            Piece::Knight => 'n',
            _ => 'q',
        };
        s.push(ch);
    }
    s
}
