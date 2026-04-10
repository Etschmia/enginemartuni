use chess::{Board, ChessMove, File, MoveGen, Piece, Rank, Square};
use rand::Rng;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::hash::polyglot_hash;

#[derive(Debug, Clone, Copy)]
pub struct BookEntry {
    pub key: u64,
    pub mv: u16,
    pub weight: u16,
    #[allow(dead_code)]
    pub learn: u32,
}

pub struct Book {
    name: String,
    entries: Vec<BookEntry>,
}

impl Book {
    pub fn load(path: &Path) -> io::Result<Self> {
        let data = fs::read(path)?;
        if data.len() % 16 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "polyglot file size not a multiple of 16 bytes",
            ));
        }
        let mut entries = Vec::with_capacity(data.len() / 16);
        for chunk in data.chunks_exact(16) {
            let key = u64::from_be_bytes(chunk[0..8].try_into().unwrap());
            let mv = u16::from_be_bytes(chunk[8..10].try_into().unwrap());
            let weight = u16::from_be_bytes(chunk[10..12].try_into().unwrap());
            let learn = u32::from_be_bytes(chunk[12..16].try_into().unwrap());
            entries.push(BookEntry { key, mv, weight, learn });
        }
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();
        Ok(Book { name, entries })
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn find(&self, key: u64) -> &[BookEntry] {
        let idx = match self.entries.binary_search_by_key(&key, |e| e.key) {
            Ok(i) => i,
            Err(_) => return &[],
        };
        let mut lo = idx;
        while lo > 0 && self.entries[lo - 1].key == key {
            lo -= 1;
        }
        let mut hi = idx + 1;
        while hi < self.entries.len() && self.entries[hi].key == key {
            hi += 1;
        }
        &self.entries[lo..hi]
    }
}

pub struct BookSet {
    books: Vec<Book>,
}

impl BookSet {
    pub fn load(dir: &Path, files: &[String]) -> Self {
        let mut books = Vec::new();
        for name in files {
            let path: PathBuf = dir.join(name);
            match Book::load(&path) {
                Ok(book) => {
                    println!(
                        "info string book loaded: {} ({} entries)",
                        book.name(),
                        book.len()
                    );
                    books.push(book);
                }
                Err(e) => {
                    println!("info string book not loaded: {} ({})", name, e);
                }
            }
        }
        BookSet { books }
    }

    pub fn is_empty(&self) -> bool {
        self.books.is_empty()
    }

    /// Sucht die aktuelle Stellung in den Buechern (Prioritaet = Reihenfolge).
    /// Erstes Buch mit Treffer gewinnt; aus dessen Eintraegen wird gewichtet
    /// zufaellig ein legaler Zug ausgewaehlt.
    pub fn probe(&self, board: &Board) -> Option<ChessMove> {
        let key = polyglot_hash(board);
        for book in &self.books {
            let entries = book.find(key);
            if entries.is_empty() {
                continue;
            }
            let legal: Vec<(u16, ChessMove)> = entries
                .iter()
                .filter_map(|e| decode_move(e.mv, board).map(|mv| (e.weight, mv)))
                .collect();
            if legal.is_empty() {
                continue;
            }
            return Some(weighted_choice(&legal));
        }
        None
    }
}

fn weighted_choice(candidates: &[(u16, ChessMove)]) -> ChessMove {
    let total: u32 = candidates.iter().map(|(w, _)| *w as u32).sum();
    let mut rng = rand::thread_rng();
    if total == 0 {
        let idx = rng.gen_range(0..candidates.len());
        return candidates[idx].1;
    }
    let mut pick = rng.gen_range(0..total);
    for (w, mv) in candidates {
        let w = *w as u32;
        if pick < w {
            return *mv;
        }
        pick -= w;
    }
    candidates.last().unwrap().1
}

fn decode_move(m: u16, board: &Board) -> Option<ChessMove> {
    let to_file = (m & 0x7) as usize;
    let to_rank = ((m >> 3) & 0x7) as usize;
    let from_file = ((m >> 6) & 0x7) as usize;
    let from_rank = ((m >> 9) & 0x7) as usize;
    let promo = ((m >> 12) & 0x7) as usize;

    let from = Square::make_square(Rank::from_index(from_rank), File::from_index(from_file));
    let mut to = Square::make_square(Rank::from_index(to_rank), File::from_index(to_file));

    let promotion = match promo {
        0 => None,
        1 => Some(Piece::Knight),
        2 => Some(Piece::Bishop),
        3 => Some(Piece::Rook),
        4 => Some(Piece::Queen),
        _ => return None,
    };

    // Polyglot kodiert Rochade als "Koenig schlaegt eigenen Turm".
    // Wir uebersetzen auf das Koenig-Zielfeld.
    if board.piece_on(from) == Some(Piece::King) {
        let castle = match (from, to) {
            (f, t) if f == Square::E1 && t == Square::H1 => Some(Square::G1),
            (f, t) if f == Square::E1 && t == Square::A1 => Some(Square::C1),
            (f, t) if f == Square::E8 && t == Square::H8 => Some(Square::G8),
            (f, t) if f == Square::E8 && t == Square::A8 => Some(Square::C8),
            _ => None,
        };
        if let Some(dest) = castle {
            to = dest;
        }
    }

    let candidate = ChessMove::new(from, to, promotion);
    for legal in MoveGen::new_legal(board) {
        if legal == candidate {
            return Some(candidate);
        }
    }
    None
}
