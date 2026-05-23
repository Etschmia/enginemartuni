use chess::ChessMove;
use std::mem::size_of;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TtFlag {
    /// Leerer Slot — die TT hat fuer diesen Schluessel keinen Eintrag.
    Empty,
    /// Exakte Bewertung im Such-Window (alpha < score < beta).
    Exact,
    /// Lower Bound (Fail-High: score >= beta). Der wahre Wert kann hoeher
    /// liegen, ist aber mindestens `score`.
    Lower,
    /// Upper Bound (Fail-Low: score <= alpha). Der wahre Wert kann
    /// niedriger liegen, ist aber hoechstens `score`.
    Upper,
}

#[derive(Debug, Clone, Copy)]
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
/// Stellungen. Hash-Slot pro Schluessel ueber Modulo, einfache Replace-
/// Always-Strategie (siehe `store`). Wird aktiv von Alpha-Beta + Quiescence
/// in `search.rs` befuellt und in der Move-Ordering ueber den Hash-Move
/// genutzt. Groesse via UCI-Option `Hash` (MB), Default in `config.rs`.
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

    pub fn probe(&self, key: u64) -> Option<&TtEntry> {
        let idx = (key as usize) % self.entries.len();
        let e = &self.entries[idx];
        if e.flag != TtFlag::Empty && e.key == key {
            Some(e)
        } else {
            None
        }
    }

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
