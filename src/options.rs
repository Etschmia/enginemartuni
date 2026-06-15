use crate::config::Config;

pub struct EngineOptions {
    pub hash: u64,
    pub move_overhead: u64,
    /// Pfad(e) zu den Syzygy-Tablebases. Leer = aus. Wird in uci.rs zum
    /// (Neu-)Laden des Tablebase-Handles ausgewertet.
    pub syzygy_path: String,
}

impl EngineOptions {
    pub fn from_config(cfg: &Config) -> Self {
        Self {
            hash: cfg.hash_size_mb as u64,
            move_overhead: 10,
            syzygy_path: cfg.syzygy_path.clone(),
        }
    }

    pub fn print_uci_options(&self) {
        println!(
            "option name Hash type spin default {} min 1 max 65536",
            self.hash
        );
        println!("option name MoveOverhead type spin default 10 min 0 max 5000");
        println!("option name Ponder type check default false");
        println!(
            "option name SyzygyPath type string default {}",
            if self.syzygy_path.is_empty() {
                "<empty>"
            } else {
                &self.syzygy_path
            }
        );
    }

    pub fn set_option(&mut self, name: &str, value: &str) {
        match name.to_lowercase().as_str() {
            "hash" => {
                if let Ok(v) = value.parse::<u64>() {
                    self.hash = v.clamp(1, 65536);
                }
            }
            "moveoverhead" => {
                if let Ok(v) = value.parse::<u64>() {
                    self.move_overhead = v.clamp(0, 5000);
                }
            }
            "syzygypath" => {
                // UCI-Konvention: "<empty>" bedeutet kein Pfad.
                self.syzygy_path = if value.trim() == "<empty>" {
                    String::new()
                } else {
                    value.trim().to_string()
                };
            }
            _ => {}
        }
    }
}
