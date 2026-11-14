// CORAL_ANIM_DEBUG=1: Diagnose fuer springende Animationen (Displays, Armorstaende und alles,
// was auf ihnen sitzt oder sie traegt). Loggt jede Positions-, Mitfahr- und
// Transformations-Aenderung dieser Entities mit Uhrzeit in Millisekunden.
//
// Gebaut, weil sich aus dem Video allein nicht sagen laesst, WOHER ein Sprung kommt: aus einem
// Positionspaket, aus einer Mitfahrt, aus einer neuen Transformation oder aus der
// Interpolation selbst. Die Sprung-Erkennung beim Zeichnen sitzt im app-Crate
// (s. app::anim_debug), sie benutzt dasselbe Flag und dieselbe Zeile.

use super::*;

pub fn anim_debug_an() -> bool {
    static AN: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *AN.get_or_init(|| std::env::var_os("CORAL_ANIM_DEBUG").is_some())
}

pub fn anim_log(text: std::fmt::Arguments) {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    eprintln!("[anim] {}.{:03} {text}", t.as_secs() % 1000, t.subsec_millis());
}

// Displays und Armorstaende -- die Bausteine fast aller Server-Animationen.
pub(crate) fn anim_relevant(e: &Entity) -> bool {
    e.display.is_some() || display_kind(e.entity_type).is_some() || entity_type_name(e.entity_type) == Some("armor_stand")
}

pub(crate) fn fmt_pos(p: (f64, f64, f64)) -> String {
    format!("({:.3} {:.3} {:.3})", p.0, p.1, p.2)
}

pub(crate) fn fmt3(v: [f32; 3]) -> String {
    format!("({:.3} {:.3} {:.3})", v[0], v[1], v[2])
}

pub(crate) fn fmt4(v: [f32; 4]) -> String {
    format!("({:.3} {:.3} {:.3} {:.3})", v[0], v[1], v[2], v[3])
}

impl Entities {
    fn anim_relevant_id(&self, id: i32) -> bool {
        let Some(e) = self.by_id.get(&id) else { return false };
        anim_relevant(e) || self.vehicle_of.contains_key(&id) || self.vehicle_of.values().any(|(v, _)| *v == id)
    }

    // Nach einem Positions-/Rotationspaket: wo das Entity steht, wohin es soll und wie.
    pub(crate) fn anim_log_pos(&self, id: i32, was: &str, vorher: Option<(f64, f64, f64)>) {
        if !anim_debug_an() || !self.anim_relevant_id(id) {
            return;
        }
        let Some(e) = self.by_id.get(&id) else { return };
        let tp = e.display.as_ref().map_or(-1, |d| d.teleport_duration);
        let fahrzeug = self.vehicle_of.get(&id).map_or(String::new(), |(v, _)| format!(" faehrt_auf={v}"));
        anim_log(format_args!(
            "{was} id={id} typ={} alter={} vorher={} pos={} ziel={} yaw={:.1}->{:.1} schritte={} tp_dauer={tp}{fahrzeug}",
            entity_type_name(e.entity_type).unwrap_or("?"),
            e.age_ticks,
            vorher.map_or("-".into(), fmt_pos),
            fmt_pos(e.pos),
            fmt_pos(e.target_pos),
            e.yaw_deg,
            e.target_yaw_deg,
            e.interp_steps,
        ));
    }
}
