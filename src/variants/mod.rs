//! Varianten-spezifische Bewertungs-Hooks.
//!
//! Martunis generische Bewertung (`eval::evaluate`) kennt nur orthodoxe
//! Schachlogik: Material, PSTs, Koenigssicherheit, Bauernstruktur usw. In
//! den Lichess-Varianten stimmt das teils gar nicht mehr (Antichess: Material
//! ist eine Last, nicht ein Vorteil; Racing Kings: nur der Wettlauf zaehlt)
//! oder braucht Zusatzterme (King of the Hill: Koenigsnaehe zum Zentrum,
//! Three-Check: verbleibende Schachs, Horde: Bauernmasse gegen Koenig).
//!
//! Dieses Modul ist die EINE Stelle, an der die Varianten eingreifen:
//! `adjust` bekommt die fertige generische Bewertung (`base`, Sicht von
//! Weiss) und darf sie ergaenzen ODER komplett ersetzen. Fuer Standard,
//! Chess960, Atomic und Crazyhouse wird `base` unveraendert durchgereicht —
//! der Standardpfad bleibt damit bit-exakt (der Dispatch ueber
//! `variant_kind()` ist fuer `chess::Board` eine Compile-Zeit-Konstante).
//!
//! Aufrufkonvention der Untermodule (alle identisch):
//!
//! ```text
//! pub fn adjust<B: EngineBoard>(board: &B, p: &EvalParams, phase: i32, base: i32) -> i32
//! ```
//!
//!   - `board`: die Stellung (koenigslose Seiten moeglich → `has_king`!)
//!   - `p`:     die geladenen Eval-Parameter (eval.toml)
//!   - `phase`: Spielphase 0..=24 (24 = volles Material), siehe `game_phase`
//!   - `base`:  generische Martuni-Bewertung in Centipawns, Sicht von Weiss
//!   - Rueckgabe: endgueltige Bewertung, Sicht von Weiss (positiv = gut
//!     fuer Weiss). Die Suche dreht das Vorzeichen selbst auf die Seite am
//!     Zug (`eval_stm`).
//!
//! Stand 05.09.2026: alle fuenf Untermodule sind implementiert. Antichess
//! und Racing Kings ERSETZEN `base` komplett (orthodoxe Bewertung passt
//! dort nicht), King of the Hill, Horde und Three-Check legen Zusatzterme
//! OBEN DRAUF. Die Bewertungsidee steht jeweils im Modulkopf.

pub mod antichess;
pub mod horde;
pub mod kingofthehill;
pub mod racingkings;
pub mod threecheck;

use crate::backend::{EngineBoard, VariantKind};
use crate::eval_config::EvalParams;

/// Dispatch auf die Varianten-Bewertung. Wird am Ende von `evaluate()` und
/// `evaluate_breakdown()` aufgerufen; beide Pfade muessen dasselbe Ergebnis
/// liefern (Breakdown-Sanity-Check in `print_eval_breakdown`).
#[inline]
pub fn adjust<B: EngineBoard>(board: &B, p: &EvalParams, phase: i32, base: i32) -> i32 {
    match board.variant_kind() {
        // Orthodoxe Regeln bzw. Varianten, deren Bewertung schon in der
        // generischen Eval steckt (Atomic/Crazyhouse ueber
        // `uses_standard_rules`/`pocket_count`): unveraendert.
        VariantKind::Standard | VariantKind::Atomic | VariantKind::Crazyhouse => base,
        VariantKind::Antichess => antichess::adjust(board, p, phase, base),
        VariantKind::KingOfTheHill => kingofthehill::adjust(board, p, phase, base),
        VariantKind::Horde => horde::adjust(board, p, phase, base),
        VariantKind::ThreeCheck => threecheck::adjust(board, p, phase, base),
        VariantKind::RacingKings => racingkings::adjust(board, p, phase, base),
    }
}
