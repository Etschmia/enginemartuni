use chess::{Board, ChessMove, File, MoveGen, Piece, Rank, Square};

use crate::backend::{EngineBoard, VariantKind};
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
    /// Martunis Eigenbuecher fuer Varianten (seit 02.10.2026, gebaut mit
    /// tools/bookbuild). Pro Variante hoechstens ein Buch. Der Schluessel ist
    /// shakmatys Zobrist64-Hash (EnPassantMode::Legal), also exakt der Wert,
    /// den das Varianten-Brett (`BoardShak`) schon als `get_hash()` fuehrt.
    /// Fuer KotH ist das der Polyglot-Schluessel; in Three-check kommen die
    /// verbleibenden Schachs hinzu, sobald eine Seite weniger als 3 hat.
    variant_books: Vec<(VariantKind, Book)>,
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
        BookSet { books, variant_books: Vec::new() }
    }

    /// Wie `load`, zusaetzlich die Varianten-Buecher. Fehlt eine Datei, wird
    /// das gemeldet und die Variante spielt ohne Buch (wie bisher).
    pub fn load_with_variants(
        dir: &Path,
        files: &[String],
        variant_files: &[(VariantKind, String)],
    ) -> Self {
        let mut set = Self::load(dir, files);
        for (kind, name) in variant_files {
            match Book::load(&dir.join(name)) {
                Ok(book) => {
                    println!(
                        "info string variant book loaded: {:?} {} ({} entries)",
                        kind,
                        book.name(),
                        book.len()
                    );
                    set.variant_books.push((*kind, book));
                }
                Err(e) => {
                    println!("info string variant book not loaded: {} ({})", name, e);
                }
            }
        }
        set
    }

    pub fn is_empty(&self) -> bool {
        self.books.is_empty() && self.variant_books.is_empty()
    }

    /// Buchabfrage fuer Varianten-Bretter (alles ohne Standard-Sicht).
    ///
    /// Unterschied zu `probe`: Der Schluessel ist `board.get_hash()` statt
    /// des Polyglot-Hashes der chess-Crate (siehe `variant_books`). Die
    /// Zug-Dekodierung ist dieselbe, nur gegen die legalen Zuege des
    /// Varianten-Bretts geprueft. Chess960 liefert `VariantKind::Standard`
    /// und findet hier kein Buch, spielt also wie bisher ohne Buch.
    pub fn probe_variant<B: EngineBoard>(&self, board: &B) -> Option<ChessMove> {
        let kind = board.variant_kind();
        let book = &self.variant_books.iter().find(|(k, _)| *k == kind)?.1;
        let entries = book.find(board.get_hash());
        if entries.is_empty() {
            return None;
        }
        let legal_moves: Vec<ChessMove> = board.legal_gen().collect();
        let legal: Vec<(u16, ChessMove)> = entries
            .iter()
            .filter_map(|e| {
                decode_move_with(e.mv, |sq| board.piece_on(sq), &legal_moves)
                    .map(|mv| (e.weight, mv))
            })
            .collect();
        if legal.is_empty() {
            return None;
        }
        Some(weighted_choice(&legal))
    }

    /// Sucht die aktuelle Stellung in den Buechern (Prioritaet = Reihenfolge).
    /// Erstes Buch mit Treffer gewinnt; aus dessen Eintraegen wird gewichtet
    /// zufaellig ein legaler Zug ausgewaehlt.
    pub fn probe(&self, board: &Board) -> Option<ChessMove> {
        let key = polyglot_hash(board);
        let mut legal_moves: Option<Vec<ChessMove>> = None;
        for book in &self.books {
            let entries = book.find(key);
            if entries.is_empty() {
                continue;
            }
            let legal_moves =
                legal_moves.get_or_insert_with(|| MoveGen::new_legal(board).collect());
            let legal: Vec<(u16, ChessMove)> = entries
                .iter()
                .filter_map(|e| decode_move(e.mv, board, legal_moves).map(|mv| (e.weight, mv)))
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

fn decode_move(m: u16, board: &Board, legal_moves: &[ChessMove]) -> Option<ChessMove> {
    decode_move_with(m, |sq| board.piece_on(sq), legal_moves)
}

/// 16-Bit-Polyglot-Zug -> legaler `ChessMove`. Das Brett wird nur fuer die
/// Frage "steht auf dem Startfeld ein Koenig?" gebraucht (Rochade), deshalb
/// reicht eine `piece_on`-Funktion: so funktioniert dieselbe Dekodierung
/// fuer die chess-Crate und fuer die Varianten-Bretter.
fn decode_move_with(
    m: u16,
    piece_on: impl Fn(Square) -> Option<Piece>,
    legal_moves: &[ChessMove],
) -> Option<ChessMove> {
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
    if piece_on(from) == Some(Piece::King) {
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
    if legal_moves.iter().any(|&legal| legal == candidate) {
        return Some(candidate);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board_shak::{BoardKingOfTheHill, BoardThreeCheck};

    /// Die mitgelieferten Eigenbuecher aus src/polyglot (Tests laufen im
    /// Crate-Root, `cargo test` setzt das Arbeitsverzeichnis dorthin).
    fn eigenbuecher() -> BookSet {
        BookSet::load_with_variants(
            Path::new("src/polyglot"),
            &[],
            &[
                (VariantKind::ThreeCheck, "martuni_3check.bin".to_string()),
                (VariantKind::KingOfTheHill, "martuni_koth.bin".to_string()),
            ],
        )
    }

    fn after<B: EngineBoard>(moves: &str) -> B {
        let mut b = B::startpos();
        for uci in moves.split_whitespace() {
            let mv = b.parse_uci_move(uci).unwrap();
            b = b.make_move_new(mv);
        }
        b
    }

    fn uci(mv: ChessMove) -> String {
        format!("{}", mv)
    }

    #[test]
    fn three_check_grundstellung_trifft_buch() {
        let set = eigenbuecher();
        let b: BoardThreeCheck = after("");
        // Buch: e2e4 405, b1c3 369, g1f3 227 → jeder Treffer muss einer davon sein.
        for _ in 0..20 {
            let mv = uci(set.probe_variant(&b).expect("Grundstellung steht im Buch"));
            assert!(["e2e4", "b1c3", "g1f3"].contains(&mv.as_str()), "{mv}");
        }
    }

    /// Rochade steht im Buch als e8h8 (Polyglot) und muss als e8g8 (Martunis
    /// Varianten-Notation) herauskommen. Buch: e8g8 900, c5f2 100.
    #[test]
    fn three_check_rochade_wird_zurueckuebersetzt() {
        let set = eigenbuecher();
        let b: BoardThreeCheck = after("e2e4 e7e5 f1c4 g8h6 b1c3 f8c5 d1f3");
        let mut gesehen = std::collections::HashSet::new();
        for _ in 0..200 {
            let mv = uci(set.probe_variant(&b).expect("Stellung steht im Buch"));
            assert!(["e8g8", "c5f2"].contains(&mv.as_str()), "{mv}");
            gesehen.insert(mv);
        }
        assert!(gesehen.contains("e8g8"));
    }

    #[test]
    fn koth_rochade_wird_zurueckuebersetzt() {
        let set = eigenbuecher();
        let b: BoardKingOfTheHill = after("d2d4 d7d5 c2c4 g8f6 b1c3 e7e6 g1f3 f8b4 c1g5");
        // Buch: e8g8 354, b8c6 315, b4c3 168, b8d7 163 (Rochade als e8h8 gespeichert).
        let mut gesehen = std::collections::HashSet::new();
        for _ in 0..200 {
            let mv = uci(set.probe_variant(&b).expect("Stellung steht im KotH-Buch"));
            assert!(["e8g8", "b8c6", "b4c3", "b8d7"].contains(&mv.as_str()), "{mv}");
            gesehen.insert(mv);
        }
        assert!(gesehen.contains("e8g8"), "Rochade muss gewaehlt werden koennen");
    }

    /// Ausserhalb des Buchs kein Treffer, und ein Varianten-Buch gilt nur fuer
    /// seine Variante (gleiche Grundstellung, gleicher Schluessel!).
    #[test]
    fn kein_treffer_ausserhalb_und_keine_vermischung() {
        let set = eigenbuecher();
        let raus: BoardThreeCheck = after("a2a3 h7h6 h2h3 a7a6");
        assert!(set.probe_variant(&raus).is_none());

        let nur_koth = BookSet::load_with_variants(
            Path::new("src/polyglot"),
            &[],
            &[(VariantKind::KingOfTheHill, "martuni_koth.bin".to_string())],
        );
        let b: BoardThreeCheck = after("");
        assert!(nur_koth.probe_variant(&b).is_none());
    }

    #[test]
    fn standard_probe_unveraendert() {
        // Ohne Standard-Buecher bleibt probe() fuer chess::Board leer, auch
        // wenn Varianten-Buecher geladen sind.
        let set = eigenbuecher();
        assert!(!set.is_empty());
        assert!(set.probe(&Board::default()).is_none());
    }
}
