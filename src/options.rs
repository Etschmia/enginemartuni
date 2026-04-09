pub struct EngineOptions {
    pub hash: u64,
    pub move_overhead: u64,
}

impl Default for EngineOptions {
    fn default() -> Self {
        Self {
            hash: 16,
            move_overhead: 10,
        }
    }
}

impl EngineOptions {
    pub fn print_uci_options() {
        println!("option name Hash type spin default 16 min 1 max 1024");
        println!("option name MoveOverhead type spin default 10 min 0 max 5000");
    }

    pub fn set_option(&mut self, name: &str, value: &str) {
        match name.to_lowercase().as_str() {
            "hash" => {
                if let Ok(v) = value.parse::<u64>() {
                    self.hash = v.clamp(1, 1024);
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
