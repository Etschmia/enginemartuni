use chess::{Board, ChessMove, MoveGen, Piece, Square};
use std::str::FromStr;

pub struct Position {
    board: Board,
}

impl Position {
    pub fn new() -> Self {
        Self {
            board: Board::default(),
        }
    }

    pub fn board(&self) -> &Board {
        &self.board
    }

    pub fn set_startpos(&mut self) {
        self.board = Board::default();
    }

    pub fn set_fen(&mut self, fen: &str) -> Result<(), String> {
        Board::from_str(fen)
            .map(|b| self.board = b)
            .map_err(|e| format!("Invalid FEN: {}", e))
    }

    pub fn apply_moves(&mut self, moves: &[&str]) -> Result<(), String> {
        for uci_move in moves {
            let m = parse_uci_move(&self.board, uci_move)?;
            self.board = self.board.make_move_new(m);
        }
        Ok(())
    }
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

    // Verify the move is legal
    let legal_moves = MoveGen::new_legal(board);
    for m in legal_moves {
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
