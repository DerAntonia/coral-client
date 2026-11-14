// CORAL_ANIM_DEBUG=1, Teil beim Zeichnen: meldet jedes Display und jeden Armorstand, der von
// einem Bild zum naechsten springt (Ursprung um mehr als SPRUNG_WEITE Bloecke oder Drehung um
// mehr als SPRUNG_GRAD Grad). Die Paket-Seite loggt world::anim_log -- beides landet mit
// derselben Uhrzeit im Client-Log, so laesst sich ein Sprung seinem Paket zuordnen.
//
// Rechnet die Lage selbst (ohne transform_at), damit das Log den gemerkten Fortschritt der
// Displays (last_progress) nicht veraendert.

use super::*;

const SPRUNG_WEITE: f32 = 0.3;
const SPRUNG_GRAD: f32 = 20.0;
// Nur in der Naehe: weiter weg sieht man es ohnehin nicht, und das Log bleibt lesbar.
const REICHWEITE: f64 = 48.0;

thread_local! {
    static VORIGES_BILD: std::cell::RefCell<std::collections::HashMap<i32, (glam::Vec3, glam::Quat)>> = std::cell::RefCell::new(std::collections::HashMap::new());
}

impl App {
    pub(crate) fn anim_debug_frame(&self) {
        if !world::anim_debug_an() {
            return;
        }
        let progress = self.tick_progress();
        let auge = self.camera.position;
        let (cam_yaw, cam_pitch) = self.vanilla_camera_angles();
        VORIGES_BILD.with_borrow_mut(|vorher| {
            let mut jetzt = std::collections::HashMap::new();
            for (&id, e) in self.entities.iter() {
                let display = e.display.as_ref();
                if display.is_none() && world::entity_type_name(e.entity_type) != Some("armor_stand") {
                    continue;
                }
                let (x, y, z) = e.render_pos(progress);
                if DVec3::new(x, y, z).distance(auge) > REICHWEITE {
                    continue;
                }
                let mut m = glam::Mat4::from_translation(glam::Vec3::new(x as f32, y as f32, z as f32));
                let mut fortschritt = 1.0;
                if let Some(d) = display {
                    let age = e.age_ticks(progress);
                    if d.interp_duration > 0 {
                        fortschritt = ((age - d.interp_start) / d.interp_duration as f32).clamp(0.0, 1.0);
                    }
                    m = m * display_billboard_matrix(d.billboard, e.render_yaw(progress), e.render_pitch(progress), cam_yaw, cam_pitch)
                        * display_affine_matrix(&d.transform_with(fortschritt));
                } else {
                    m *= glam::Mat4::from_rotation_y(-e.render_yaw(progress).to_radians());
                }
                let (_, drehung, ursprung) = m.to_scale_rotation_translation();
                if let Some(&(alt_ursprung, alt_drehung)) = vorher.get(&id) {
                    let weite = ursprung.distance(alt_ursprung);
                    let grad = alt_drehung.angle_between(drehung).to_degrees();
                    if weite > SPRUNG_WEITE || grad > SPRUNG_GRAD {
                        let extra = display.map_or(String::new(), |d| {
                            format!(" alter={:.2} start={} dauer={} fortschritt={fortschritt:.3} tp_dauer={}", e.age_ticks(progress), d.interp_start, d.interp_duration, d.teleport_duration)
                        });
                        world::anim_log(format_args!(
                            "SPRUNG_IM_BILD id={id} typ={} weite={weite:.3} grad={grad:.1} von=({:.3} {:.3} {:.3}) nach=({:.3} {:.3} {:.3}) tick_anteil={progress:.3}{extra}",
                            world::entity_type_name(e.entity_type).unwrap_or("?"),
                            alt_ursprung.x, alt_ursprung.y, alt_ursprung.z, ursprung.x, ursprung.y, ursprung.z,
                        ));
                    }
                }
                jetzt.insert(id, (ursprung, drehung));
            }
            *vorher = jetzt;
        });
    }
}
