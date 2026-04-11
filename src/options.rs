use crate::config::Config;

pub struct EngineOptions {
    pub hash: u64,
    pub move_overhead: u64,
}

impl EngineOptions {
    pub fn from_config(cfg: &Config) -> Self {
        Self {
            hash: cfg.hash_size_mb as u64,
            move_overhead: 10,
        }
    }

    pub fn print_uci_options(&self) {
        println!(
            "option name Hash type spin default {} min 1 max 65536",
            self.hash
        );
        println!("option name MoveOverhead type spin default 10 min 0 max 5000");
        println!("option name Ponder type check default false");
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
            _ => {}
        }
    }
}
