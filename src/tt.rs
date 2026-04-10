use chess::ChessMove;
use std::mem::size_of;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TtFlag {
    Empty,
    #[allow(dead_code)]
    Exact,
    #[allow(dead_code)]
    Lower,
    #[allow(dead_code)]
    Upper,
}

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub struct TtEntry {
    pub key: u64,
    pub best_move: Option<ChessMove>,
    pub eval: i32,
    pub depth: i8,
    pub flag: TtFlag,
}

impl Default for TtEntry {
    fn default() -> Self {
        Self {
            key: 0,
            best_move: None,
            eval: 0,
            depth: -1,
            flag: TtFlag::Empty,
        }
    }
}

/// Transposition Table — reservierter RAM-Bereich fuer bereits bewertete
/// Stellungen. Phase 1: Grundgeruest; wird von der Suche erst gefuellt,
/// sobald Alpha-Beta implementiert ist.
pub struct TranspositionTable {
    entries: Vec<TtEntry>,
    size_mb: usize,
}

impl TranspositionTable {
    pub fn new(size_mb: usize) -> Self {
        let num = Self::num_entries(size_mb);
        Self {
            entries: vec![TtEntry::default(); num],
            size_mb: size_mb.max(1),
        }
    }

    fn num_entries(size_mb: usize) -> usize {
        let bytes = size_mb.max(1) * 1024 * 1024;
        (bytes / size_of::<TtEntry>()).max(1)
    }

    pub fn clear(&mut self) {
        for e in self.entries.iter_mut() {
            *e = TtEntry::default();
        }
    }

    pub fn resize(&mut self, size_mb: usize) {
        let size_mb = size_mb.max(1);
        if size_mb == self.size_mb {
            self.clear();
            return;
        }
        self.size_mb = size_mb;
        self.entries = vec![TtEntry::default(); Self::num_entries(size_mb)];
    }

    pub fn size_mb(&self) -> usize {
        self.size_mb
    }

    #[allow(dead_code)]
    pub fn capacity(&self) -> usize {
        self.entries.len()
    }

    #[allow(dead_code)]
    pub fn probe(&self, key: u64) -> Option<&TtEntry> {
        let idx = (key as usize) % self.entries.len();
        let e = &self.entries[idx];
        if e.flag != TtFlag::Empty && e.key == key {
            Some(e)
        } else {
            None
        }
    }

    #[allow(dead_code)]
    pub fn store(
        &mut self,
        key: u64,
        best_move: Option<ChessMove>,
        eval: i32,
        depth: i8,
        flag: TtFlag,
    ) {
        let idx = (key as usize) % self.entries.len();
        self.entries[idx] = TtEntry {
            key,
            best_move,
            eval,
            depth,
            flag,
        };
    }
}
