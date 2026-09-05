use crate::config::Config;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UciVariant {
    Chess,
    Atomic,
    Crazyhouse,
    Antichess,
    KingOfTheHill,
    Horde,
    ThreeCheck,
    RacingKings,
}

pub struct EngineOptions {
    pub hash: u64,
    pub move_overhead: u64,
    /// Pfad(e) zu den Syzygy-Tablebases. Leer = aus. Wird in uci.rs zum
    /// (Neu-)Laden des Tablebase-Handles ausgewertet.
    pub syzygy_path: String,
    /// Chess960-Modus (UCI_Chess960). Schaltet in uci.rs auf das
    /// shakmaty-Backend um: Shredder-FEN-Parsing, Rochade als
    /// "Koenig x eigener Turm" (e1h1), kein Polyglot-Buch.
    pub chess960: bool,
    /// Regelvariante gemaess UCI_Variant. Chess960 bleibt als orthogonale
    /// Brettaufstellungs-Option separat, wie vom UCI-Protokoll vorgesehen.
    pub variant: UciVariant,
}

impl EngineOptions {
    pub fn from_config(cfg: &Config) -> Self {
        Self {
            hash: cfg.hash_size_mb as u64,
            move_overhead: 10,
            syzygy_path: cfg.syzygy_path.clone(),
            chess960: false,
            variant: UciVariant::Chess,
        }
    }

    pub fn print_uci_options(&self) {
        println!(
            "option name Hash type spin default {} min 1 max 65536",
            self.hash
        );
        println!("option name MoveOverhead type spin default 10 min 0 max 5000");
        println!("option name Ponder type check default false");
        println!("option name UCI_Chess960 type check default false");
        println!(
            "option name UCI_Variant type combo default chess var chess var atomic var crazyhouse \
             var antichess var kingofthehill var horde var 3check var racingkings"
        );
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
            "uci_chess960" => {
                self.chess960 = value.trim().eq_ignore_ascii_case("true");
            }
            "uci_variant" => {
                // Werte, wie python-chess/lichess-bot sie sendet; die
                // Aliasse (giveaway/suicide/threecheck) sind gaengige
                // Schreibweisen anderer GUIs.
                self.variant = match value.trim().to_ascii_lowercase().as_str() {
                    "atomic" => UciVariant::Atomic,
                    "crazyhouse" => UciVariant::Crazyhouse,
                    "antichess" | "giveaway" | "suicide" => UciVariant::Antichess,
                    "kingofthehill" => UciVariant::KingOfTheHill,
                    "horde" => UciVariant::Horde,
                    "3check" | "threecheck" => UciVariant::ThreeCheck,
                    "racingkings" => UciVariant::RacingKings,
                    _ => UciVariant::Chess,
                };
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
