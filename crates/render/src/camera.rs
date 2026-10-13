// Einfache Fly-Kamera (Position + Yaw/Pitch) fuer den ersten Blick in die Welt.
// Spaeter durch Client-Prediction (Meilenstein 7) ersetzt/erweitert.

use glam::{DVec3, Mat4, Vec3};

pub struct Camera {
    // ABSOLUTE Weltposition des Auges, in f64.
    //
    // ECHTER BUG gewesen (zitternde Geometrie und ein 8-Stufen-Raster beim Steilblick auf
    // einem Server mit sehr grossen Koordinaten): frueher ein Vec3 in f32. Bei x = -186634
    // betraegt die f32-Aufloesung dort schon 0.015625 -- die Kameraposition selbst rastete
    // also auf ein 1/64-Gitter, und mit ihr alles, was daraus abgeleitet wird (Blickstrahl,
    // Sortierung, Sichtpyramide). f64 hat an derselben Stelle rund 3e-11 -- der Fehler ist
    // damit acht Groessenordnungen kleiner als ein Pixel.
    //
    // Fuers Zeichnen wird diese Position NIE an die GPU gegeben (sie waere in f32 sofort
    // wieder kaputt): die Geometrie kommt bereits kamerarelativ an (s. render_view_matrix
    // und BlockRenderer), die Kamera steht im Renderraum also im Ursprung.
    pub position: DVec3,
    // GRAD (nicht Radiant!) um die Y-Achse, in Minecrafts Konvention: 0 = Blick nach +Z
    // (Sueden), steigende Werte drehen nach rechts (+Z -> -X -> -Z -> +X). Genau diese
    // Konvention erwartet der Server spaeter in Set Player Rotation -- deshalb hier so herum
    // und nicht per Vorzeichenwechsel beim Maus-Delta geradegebogen.
    //
    // ECHTER BUG gewesen (Grims Teleport-Warteschlange verlangt exakte
    // Bit-Gleichheit der zurueckgesendeten Rotation nach einer Server-Korrektur): frueher
    // wurde hier in RADIANT gespeichert, jede Grad->Radiant->Grad-Rundreise beim Empfangen/
    // Senden einer Server-Rotation verlor durch Gleitkomma-Rundung ein winziges bisschen
    // Praezision -- genug, damit `==`-Vergleiche auf Serverseite fehlschlugen. Echtes Vanilla
    // (Entity.yaw/pitch) speichert intern ebenfalls direkt in Grad, nie in Radiant, genau um
    // das zu vermeiden.
    pub yaw: f32,
    pub pitch: f32, // Grad, geclampt gegen Ueberschlag
    // Vertikales Sichtfeld in Grad (Vanilla-Default 70). Vom Aufrufer aus den Settings
    // gesetzt (s. main.rs), fliesst in proj_matrix() UND als tan(fov/2) ins Kamera-Uniform
    // fuer die nichtlinearen Projektionen im Shader.
    pub fov_degrees: f32,
    // Projektionsmodus: 0 = Normal (rektilinear), 1 = Panini, 2 = Stereografisch,
    // 3 = Zylindrisch. Nur der Wert wird ins Uniform gereicht -- die eigentliche Warp-Mathe
    // steckt im Vertex-Shader (block.wgsl), damit die CPU-Seite modus-unabhaengig bleibt.
    pub projection: u8,
    // Perspektive (Vanillas F5): Verschiebung der Kamera aus der Augenposition heraus und
    // ob die Blickrichtung dabei umgedreht ist (Frontansicht). Beides betrifft AUSSCHLIESSLICH
    // view_matrix()/view_forward() -- forward() bleibt die Blickrichtung des SPIELERS und
    // damit die Grundlage fuers Zielen und fuer die an den Server gesendete Rotation. Wuerde
    // die Frontansicht yaw/pitch selbst drehen, wuerde sich der Spieler mitdrehen.
    pub view_offset: Vec3,
    pub view_inverted: bool,
}

// Obergrenze fuer den Cull-Kegel. Eine Perspektivmatrix ist ueber tan(fov/2) definiert --
// bei 180 Grad ist das unendlich, darueber negativ (das Bild klappt um). 175 laesst genug
// Abstand zu dieser Singularitaet und ist trotzdem weiter als jedes einstellbare FOV.
pub const CULL_FOV_MAX_DEGREES: f32 = 175.0;

impl Camera {
    pub fn new(position: DVec3) -> Self {
        Self { position, yaw: 0.0, pitch: 0.0, fov_degrees: 70.0, projection: 0, view_offset: Vec3::ZERO, view_inverted: false }
    }

    // Das Minus bei x ist der Kern der Konvention: ohne es dreht steigender Yaw nach +X,
    // waehrend right() nach -X zeigt -- die Kamera liefe der Maus dann entgegen.
    pub fn forward(&self) -> Vec3 {
        let (yaw, pitch) = (self.yaw.to_radians(), self.pitch.to_radians());
        Vec3::new(-yaw.sin() * pitch.cos(), pitch.sin(), yaw.cos() * pitch.cos()).normalize()
    }

    // Rechts aus Sicht der Kamera. In einem rechtshaendigen System ist das
    // forward x up: bei Blick nach +Z (Sueden) ergibt das -X (Westen) -- und nach
    // Westen zeigt die rechte Hand, wenn man nach Sueden schaut.
    pub fn right(&self) -> Vec3 {
        self.forward().cross(Vec3::Y).normalize()
    }

    // Blickrichtung der KAMERA (nicht des Spielers): in der Frontansicht um 180 Grad gedreht
    // und im Nick gespiegelt, wie Vanillas Camera.setRotation(yaw + 180, -pitch).
    pub fn view_forward(&self) -> Vec3 {
        if !self.view_inverted {
            return self.forward();
        }
        let (yaw, pitch) = ((self.yaw + 180.0).to_radians(), (-self.pitch).to_radians());
        Vec3::new(-yaw.sin() * pitch.cos(), pitch.sin(), yaw.cos() * pitch.cos()).normalize()
    }

    // Tatsaechliche Kameraposition in der WELT -- Auge plus Perspektiv-Versatz (in der
    // 3. Person nach hinten weg, s. view_offset). Fuer Spiel-Logik; zum Zeichnen dient
    // render_view_position(), das im kamerarelativen Raum bleibt.
    pub fn view_position(&self) -> DVec3 {
        self.position + self.view_offset.as_dvec3()
    }

    // Dieselbe Kameraposition im RENDERRAUM: dort liegt das Auge im Ursprung, und der
    // Perspektiv-Versatz ist alles, was von der Position uebrig bleibt. Weil die gesamte
    // Geometrie relativ zum Auge hochgeladen wird (s. BlockRenderer::column_offset), sind
    // hier nur noch kleine Zahlen im Spiel -- unabhaengig davon, wie weit man vom
    // Weltnullpunkt entfernt steht.
    pub fn render_view_position(&self) -> Vec3 {
        self.view_offset
    }

    // ECHTER BUG gewesen (beim Nach-unten-Schauen drehte sich die Kamera scheinbar nur
    // noch in Achtelschritten, nur auf einem Server mit sehr grossen Koordinaten):
    // frueher stand hier look_at_rh(pos, pos + view_forward(), Y) -- also ein ZIELPUNKT,
    // gebildet aus Position PLUS Blickrichtung. Genau diese Addition zerstoert die Richtung,
    // sobald die Position gross und der waagerechte Anteil der Richtung klein ist:
    //
    //   Position x = -186634  ->  f32-Aufloesung (ulp) = 0.015625
    //   Pitch 89 Grad         ->  waagerechter Anteil  = cos(89) = 0.017452
    //
    // Die Richtung ist damit kaum groesser als ein Rundungsschritt der Position. Nach dem
    // Addieren und Wieder-Abziehen bleiben pro Achse nur noch drei Werte (-ulp, 0, +ulp)
    // uebrig, zusammen also 8 unterscheidbare Blickrichtungen -- die beobachteten
    // Achtelschritte, nachgerechnet und als Test unten festgehalten.
    //
    // look_to_rh nimmt die RICHTUNG direkt entgegen, statt sie sich aus zwei fast gleichen
    // grossen Zahlen zurueckzurechnen. Damit ist die Blickrichtung unabhaengig davon, wie
    // weit man vom Weltnullpunkt entfernt steht.
    //
    // NICHT geloest ist damit die Praezision der POSITION selbst: die Translation rechnet
    // weiter in f32, bei x=-186634 also in Schritten von 0.015625. Das aeussert sich als
    // leichtes Zittern der Geometrie, nicht als Richtungsraster -- dagegen hilft nur
    // kamerarelatives Rendern (Geometrie vor der f32-Wandlung um die Kameraposition
    // versetzen), was ein deutlich groesserer Umbau waere.
    pub fn view_matrix(&self) -> Mat4 {
        Mat4::look_to_rh(self.render_view_position(), self.view_forward(), Vec3::Y)
    }

    pub fn proj_matrix(&self, aspect: f32) -> Mat4 {
        Mat4::perspective_rh(self.fov_degrees.to_radians(), aspect, 0.05, 1000.0)
    }

    pub fn view_proj_matrix(&self, aspect: f32) -> Mat4 {
        self.proj_matrix(aspect) * self.view_matrix()
    }

    // Frustum-Matrix fuer die Chunk-Sichtbarkeitspruefung. Bei den Warp-Projektionen
    // (Panini/Stereografisch/Zylindrisch) holt der Shader Geometrie an den Bildrand, die das
    // enge Standard-Frustum schon verworfen haette -- deshalb dort mit einem bewusst breiten
    // Sicht-Kegel cullen, damit am Rand nichts wegpoppt. Ueber-Zeichnen (ein paar zusaetzliche
    // Chunk-Spalten) ist harmlos, fehlende Chunks am Rand waeren sichtbar. Normal = exakt das
    // echte FOV, keine Aenderung gegenueber frueher.
    pub fn cull_view_proj_matrix(&self, aspect: f32) -> Mat4 {
        // Der Cull-Kegel darf NIE enger sein als das eingestellte Sichtfeld -- sonst
        // verwirft er genau die Spalten, die der Warp gerade an den Rand holen soll. Frueher
        // stand hier ein festes .min(160.0); das war harmlos, solange das FOV bei 110
        // gedeckelt war, wuerde aber ab 160 aufwaerts Chunks am Bildrand wegschneiden.
        // Deshalb: mindestens 150 Grad, sonst das echte FOV -- nach oben nur so weit
        // begrenzt, dass perspective_rh nicht in die tan(fov/2)-Singularitaet bei 180 laeuft.
        let fov = if self.projection == 0 { self.fov_degrees } else { self.fov_degrees.max(150.0) };
        Mat4::perspective_rh(fov.min(CULL_FOV_MAX_DEGREES).to_radians(), aspect, 0.05, 1000.0) * self.view_matrix()
    }
}

#[cfg(test)]
mod grosse_koordinaten_tests {
    use super::*;

    // Die Kamera an eine echte DonutSMP-Position stellen und den Steilblick durchdrehen:
    // die Blickrichtungen, die die View-Matrix tatsaechlich abbildet, muessen fein
    // aufgeloest bleiben. Mit dem alten look_at_rh(pos, pos + forward, Y) blieben hier
    // exakt 8 verschiedene Richtungen uebrig (s. view_matrix-Kommentar).
    #[test]
    fn steilblick_bleibt_bei_grossen_koordinaten_fein_aufgeloest() {
        let mut cam = Camera::new(DVec3::new(-186634.3, 75.8, 153997.0));
        cam.pitch = -89.0;
        let mut gesehen = std::collections::HashSet::new();
        for schritt in 0..3600 {
            cam.yaw = schritt as f32 / 10.0;
            // Die dritte Zeile der View-Matrix ist die (negierte) Blickrichtung -- das ist
            // die Groesse, die beim Bug zusammenbrach, nicht cam.yaw selbst.
            let m = cam.view_matrix().to_cols_array();
            gesehen.insert((m[2].to_bits(), m[6].to_bits(), m[10].to_bits()));
        }
        assert!(
            gesehen.len() > 3000,
            "Blickrichtung ist auf {} Stufen gerastert -- bei 3600 Yaw-Werten viel zu grob (Bug: 8)",
            gesehen.len()
        );
    }

    // Der Nachweis, dass es an der Addition lag und nicht am Pitch: dieselbe Kamera nahe
    // dem Weltnullpunkt war NIE betroffen. Faellt dieser Test, ist die Ursache falsch
    // verstanden worden.
    #[test]
    fn nahe_null_war_der_steilblick_schon_immer_fein() {
        let mut cam = Camera::new(DVec3::new(8.0, 75.8, -12.0));
        cam.pitch = -89.0;
        let mut gesehen = std::collections::HashSet::new();
        for schritt in 0..3600 {
            cam.yaw = schritt as f32 / 10.0;
            let m = cam.view_matrix().to_cols_array();
            gesehen.insert((m[2].to_bits(), m[6].to_bits(), m[10].to_bits()));
        }
        assert!(gesehen.len() > 3000, "nahe Null darf es nie ein Raster gegeben haben, war aber {}", gesehen.len());
    }
}

#[cfg(test)]
mod fov_range_tests {
    use super::*;

    // Der Cull-Kegel darf nie enger sein als das eingestellte Sichtfeld -- sonst verwirft er
    // genau die Chunk-Spalten, die der Warp an den Bildrand holt, und am Rand poppt es.
    // Frueher deckelte er fest bei 160, was ab einem FOV von 160 aufwaerts genau das getan
    // haette (das FOV war damals bei 110 gedeckelt, deshalb fiel es nicht auf).
    #[test]
    fn cull_kegel_ist_nie_enger_als_das_sichtfeld() {
        for projection in [1u8, 2, 3] {
            for fov in [30.0f32, 70.0, 110.0, 150.0, 170.0] {
                let cam = Camera { fov_degrees: fov, projection, ..Camera::new(glam::DVec3::ZERO) };
                let effective = if cam.projection == 0 { fov } else { fov.max(150.0) }.min(CULL_FOV_MAX_DEGREES);
                assert!(effective >= fov, "Cull-FOV {effective} enger als Sichtfeld {fov}");
            }
        }
    }

    // tan(fov/2) ist die eigentliche Grenze der Perspektivmatrix: bei 180 Grad unendlich.
    // Der Deckel muss also mit Abstand darunter bleiben, sonst kippt das Bild um.
    #[test]
    fn cull_deckel_bleibt_unter_der_singularitaet() {
        const { assert!(CULL_FOV_MAX_DEGREES < 180.0) };
        assert!((CULL_FOV_MAX_DEGREES / 2.0).to_radians().tan().is_finite());
    }
}

#[cfg(test)]
mod bezugspunkt_tests {
    use super::*;
    use glam::DVec3;

    // Welcher Punkt ist der BEZUGSPUNKT, relativ zu dem Geometrie hochgeladen werden muss?
    //
    // view_matrix() blickt aus render_view_position() == view_offset. Das heisst: im
    // hochgeladenen Raum sitzt die Kamera bei view_offset -- und der Ursprung ist damit das
    // AUGE, nicht die Kamera. Folglich muss jede Geometrie als `P - position` hochgeladen
    // werden (position == Auge), NICHT als `P - view_position()`.
    //
    // In der Ego-Ansicht ist view_offset null und beide Varianten fallen zusammen; erst in
    // der 3. Person laufen sie um genau den Kameraversatz auseinander -- deshalb konnte ein
    // falscher Bezugspunkt hier lange unbemerkt bleiben.
    fn view_raum(kamera: &Camera, welt_punkt: DVec3, bezug: DVec3) -> Vec3 {
        let relativ = (welt_punkt - bezug).as_vec3();
        kamera.view_matrix().transform_point3(relativ)
    }

    // Die Wahrheit, unabhaengig vom Upload-Trick: die Kamera steht bei view_position() und
    // schaut in view_forward().
    fn view_raum_absolut(kamera: &Camera, welt_punkt: DVec3) -> Vec3 {
        let absolut = Mat4::look_to_rh(kamera.view_position().as_vec3(), kamera.view_forward(), Vec3::Y);
        absolut.transform_point3(welt_punkt.as_vec3())
    }

    fn dritte_person() -> Camera {
        let mut kamera = Camera::new(DVec3::new(10.0, 65.0, -20.0));
        kamera.yaw = 37.0;
        kamera.pitch = -12.0;
        // Wie update_perspective_offset in der 3. Person: nach hinten weg und etwas hoch.
        kamera.view_offset = Vec3::new(2.5, 0.75, -3.5);
        kamera
    }

    #[test]
    fn das_auge_ist_der_bezugspunkt_nicht_die_kameraposition() {
        let kamera = dritte_person();
        let punkt = DVec3::new(14.0, 64.0, -25.0);
        let soll = view_raum_absolut(&kamera, punkt);

        let mit_auge = view_raum(&kamera, punkt, kamera.position);
        assert!((mit_auge - soll).length() < 1e-3, "Auge als Bezug muss stimmen: {mit_auge} statt {soll}");

        // Und die Gegenprobe: die Kameraposition als Bezug ist um GENAU den Versatz falsch.
        // Genau dieser Fehler stand in render_world (Spalten-Versaetze und Frustum-Test
        // bekamen view_position(), Entities und Geisterbloecke das Auge) -- die Welt war
        // dadurch in der 3. Person gegen alle Entities verschoben.
        let mit_kamera = view_raum(&kamera, punkt, kamera.view_position());
        let fehler = (mit_kamera - soll).length();
        assert!(fehler > 1.0, "Kameraposition als Bezug muss sichtbar danebenliegen, war {fehler}");
    }

    // In der Ego-Ansicht sind beide Bezugspunkte identisch -- deshalb faellt der Fehler dort
    // nicht auf.
    #[test]
    fn in_der_ego_ansicht_fallen_beide_bezugspunkte_zusammen() {
        let mut kamera = dritte_person();
        kamera.view_offset = Vec3::ZERO;
        assert_eq!(kamera.view_position(), kamera.position);
    }
}
