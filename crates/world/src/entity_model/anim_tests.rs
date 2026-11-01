// Tests zu Posen, Skins, Umhang, Armschwingen, Koerperdrehung, Licht, Ich-Sicht und Ausruestung.

#[allow(unused_imports)]
use super::*;

#[cfg(test)]
mod limb_tests {
    use super::*;

    // Der Kern: Sprinten muss die Schrittfrequenz ERHOEHEN. Gefuettert
    // mit Vanillas echten Strecken pro Tick (s. physics.rs::strecke_pro_tick_trifft_vanilla).
    const GEHEN: f32 = 0.2159;
    const SPRINTEN: f32 = 0.2806;

    fn eingelaufen(moved: f32) -> LimbAnimator {
        let mut a = LimbAnimator::new();
        // Genug Ticks, damit speed den Zielwert praktisch erreicht hat (Rate 0.4).
        for _ in 0..40 {
            a.tick(moved, 1.0);
        }
        a
    }

    #[test]
    fn sprinten_beschleunigt_die_schrittfrequenz() {
        let gehen = eingelaufen(GEHEN);
        let sprinten = eingelaufen(SPRINTEN);
        // Frequenz = Phasenzuwachs pro Tick.
        let f_gehen = gehen.animation_progress(1.0) - gehen.animation_progress(0.0);
        let f_sprinten = sprinten.animation_progress(1.0) - sprinten.animation_progress(0.0);
        assert!(f_sprinten > f_gehen, "Sprinten muss schneller schwingen ({f_sprinten} vs {f_gehen})");
        // Vanillas Verhaeltnis: 1.0 / 0.8636 = 1.158.
        let verhaeltnis = f_sprinten / f_gehen;
        assert!((verhaeltnis - 1.158).abs() < 0.01, "Verhaeltnis {verhaeltnis:.3}, erwartet 1.158");
    }

    #[test]
    fn amplitude_deckelt_bei_eins() {
        assert!((eingelaufen(GEHEN).amplitude(1.0) - 0.8636).abs() < 0.002);
        // Exakt 1.0 wird per Annaeherung nie ganz erreicht (Rate 0.4 halbiert den Rest je
        // Tick) -- geprueft wird deshalb "praktisch am Deckel", nicht Gleichheit.
        assert!(eingelaufen(SPRINTEN).amplitude(1.0) > 0.9999, "sprintend ist die Amplitude ausgereizt");
        // Auch bei absurd hoher Geschwindigkeit (Elytra, Knockback) nie UEBER 1.
        assert!(eingelaufen(5.0).amplitude(1.0) <= 1.0);
        assert!(eingelaufen(5.0).amplitude(1.0) > 0.9999);
    }

    // Der Fehler, der zum Bug fuehrte: die gedaempfte GESCHWINDIGKEIT statt der Strecke. Auf
    // Boden daempft Vanilla mit 0.6 * 0.91 = 0.546 -- damit landet man weit unter dem Deckel
    // und der Unterschied zwischen Gehen und Sprinten verschwindet in einer Mini-Bewegung.
    #[test]
    fn gedaempfte_geschwindigkeit_waere_viel_zu_klein() {
        let daempfung = 0.6 * 0.91;
        let falsch = eingelaufen(GEHEN * daempfung).amplitude(1.0);
        let richtig = eingelaufen(GEHEN).amplitude(1.0);
        assert!(falsch < richtig * 0.6, "die falsche Groesse war grob halb so gross ({falsch} vs {richtig})");
    }

    // Stehenbleiben muss die Schwingung auslaufen lassen, nicht abrupt stoppen.
    #[test]
    fn stehenbleiben_laeuft_aus() {
        let mut a = eingelaufen(SPRINTEN);
        let voll = a.amplitude(1.0);
        a.tick(0.0, 1.0);
        let nach_einem_tick = a.amplitude(1.0);
        assert!(nach_einem_tick < voll && nach_einem_tick > 0.0, "muss abklingen, nicht springen ({nach_einem_tick})");
        for _ in 0..40 {
            a.tick(0.0, 1.0);
        }
        assert!(a.amplitude(1.0) < 1e-4, "nach genug Ticks steht sie still");
    }

    // Die Phase muss zwischen zwei Ticks GLEITEN -- ohne die Rueckrechnung springt sie 20 mal
    // pro Sekunde weiter, was bei hoher Framerate als Zucken sichtbar ist.
    #[test]
    fn phase_gleitet_zwischen_den_ticks() {
        let a = eingelaufen(SPRINTEN);
        let anfang = a.animation_progress(0.0);
        let mitte = a.animation_progress(0.5);
        let ende = a.animation_progress(1.0);
        assert!(anfang < mitte && mitte < ende, "monoton ueber den Tick ({anfang} / {mitte} / {ende})");
        assert!(((mitte - anfang) - (ende - mitte)).abs() < 1e-5, "gleichmaessig, nicht sprunghaft");
    }

    // Jungtiere trippeln dreimal so schnell (Vanillas timeScale) -- dieselbe Strecke, dreifache
    // Phase.
    #[test]
    fn jungtiere_trippeln_dreifach() {
        let mut erwachsen = LimbAnimator::new();
        let mut jung = LimbAnimator::new();
        for _ in 0..40 {
            erwachsen.tick(GEHEN, 1.0);
            jung.tick(GEHEN, 3.0);
        }
        assert!((jung.animation_progress(1.0) / erwachsen.animation_progress(1.0) - 3.0).abs() < 1e-4);
    }
}

#[cfg(test)]
mod pose_tests {
    use super::*;

    // Ohne Bewegung darf nichts schwingen -- sonst zappeln stehende Mobs.
    #[test]
    fn ohne_bewegung_keine_schwingung() {
        let pose = biped_pose(0.0, 0.0, 0.0, 0.0, false, 0.0);
        for part in ["right_arm", "left_arm"] {
            assert_eq!(pose.get(part).rotation[0], 0.0, "{part} darf stillstehen");
        }
    }

    // Arme und Beine muessen GEGENLAEUFIG schwingen (rechter Arm mit linkem Bein) -- laufen
    // sie gleichphasig, sieht die Figur aus wie ein huepfender Zinnsoldat.
    #[test]
    fn arme_und_beine_schwingen_gegenlaeufig() {
        let pose = biped_pose(0.0, 0.0, 1.0, 1.0, false, 0.0);
        let ra = pose.get("right_arm").rotation[0];
        let la = pose.get("left_arm").rotation[0];
        let rl = pose.get("right_leg").rotation[0];
        assert!(ra * la < 0.0, "die beiden Arme muessen entgegengesetzt schwingen ({ra} / {la})");
        assert!(ra * rl < 0.0, "rechter Arm und rechtes Bein muessen entgegengesetzt schwingen ({ra} / {rl})");
    }

    // Die Amplitude muss durchskalieren: halbe Geschwindigkeit, halber Ausschlag.
    #[test]
    fn amplitude_skaliert_den_ausschlag() {
        let voll = biped_pose(0.0, 0.0, 2.0, 1.0, false, 0.0).get("right_leg").rotation[0];
        let halb = biped_pose(0.0, 0.0, 2.0, 0.5, false, 0.0).get("right_leg").rotation[0];
        assert!((voll / 2.0 - halb).abs() < 1e-6, "halbe Amplitude = halber Ausschlag ({voll} / {halb})");
    }

    // Kopfwinkel kommen in GRAD herein und muessen als Radiant herauskommen.
    #[test]
    fn kopfwinkel_werden_nach_radiant_umgerechnet() {
        let pose = biped_pose(90.0, -45.0, 0.0, 0.0, false, 0.0);
        let r = pose.get("head").rotation;
        assert!((r[1] - std::f32::consts::FRAC_PI_2).abs() < 1e-5, "90 Grad Gierung = PI/2, ist {}", r[1]);
        assert!((r[0] + std::f32::consts::FRAC_PI_4).abs() < 1e-5, "-45 Grad Neigung = -PI/4, ist {}", r[0]);
    }

    // Schleichen kippt den Rumpf und hebt den Kopf-Pivot -- ohne das steckt der Kopf beim
    // Schleichen im Boden statt sich zu senken.
    #[test]
    fn schleichen_kippt_den_rumpf_und_verschiebt_pivots() {
        let pose = biped_pose(0.0, 0.0, 0.0, 0.0, true, 0.0);
        assert_eq!(pose.get("body").rotation[0], 0.5);
        assert_eq!(pose.get("body").pivot_offset[1], 3.2);
        assert_eq!(pose.get("head").pivot_offset[1], 4.2);
        assert_eq!(pose.get("right_leg").pivot_offset[2], 4.0);
        // pivotY 12.0 -> 12.2 (s. Schleich-Zweig): die Beine senken sich mit.
        assert_eq!(pose.get("right_leg").pivot_offset[1], 0.2);
        assert_eq!(pose.get("left_leg").pivot_offset[1], 0.2);
        // Die Arme behalten ihre Schwingung UND bekommen den Schleich-Zuschlag.
        let arm = biped_pose(0.0, 0.0, 1.0, 1.0, true, 0.0).get("right_arm").rotation[0];
        let arm_ohne = biped_pose(0.0, 0.0, 1.0, 1.0, false, 0.0).get("right_arm").rotation[0];
        assert!((arm - arm_ohne - 0.4).abs() < 1e-6, "Schleichen addiert 0.4 auf die Armschwingung");
    }

    // Jeder Eintrag der Tabelle muss ein Modell mit Teilen und einer Textur haben, und jeder
    // Typname muss es in der echten Registry geben -- ein Tippfehler waere sonst still.
    #[test]
    fn modelltabelle_ist_konsistent() {
        let table = model_table();
        assert!(table.len() >= 20, "Tabelle sollte die gaengigen Mobs abdecken, hat {}", table.len());
        for (name, model, _kind) in &table {
            assert!(crate::entity_types::ENTITY_TYPE_NAMES.contains(name), "'{name}' steht nicht in der Entity-Registry");
            assert!(!model.parts.is_empty(), "'{name}' hat keine Teile");
            assert!(model.texture.starts_with("entity/"), "'{name}' hat einen unplausiblen Texturpfad '{}'", model.texture);
            assert!(model.tex_size[0] > 0.0 && model.tex_size[1] > 0.0);
        }
        // Keine Doppeleintraege -- der zweite wuerde stillschweigend gewinnen.
        let mut names: Vec<&str> = table.iter().map(|(n, _, _)| *n).collect();
        names.sort_unstable();
        let vorher = names.len();
        names.dedup();
        assert_eq!(names.len(), vorher, "doppelter Eintrag in der Modelltabelle");
    }

    // Katze, Ozelot und Schildkroete haben echte Modelle (frueher kamen sie als Box).
    #[test]
    fn katze_und_schildkroete_haben_modelle() {
        let table = model_table();
        for name in ["cat", "ocelot", "turtle"] {
            assert!(table.iter().any(|(n, _, _)| *n == name), "{name} fehlt");
        }
        assert_eq!(cat_texture_for("minecraft:all_black"), Some("entity/cat/all_black"));
        assert_eq!(cat_texture_for("meinpack:tiger"), None);
        assert_eq!(model_scale("cat"), 0.8);
        assert_eq!(model_scale("ocelot"), 1.0);
        // Sitzend kippt der Rumpf auf pi/4 -- also pi/4 - pi/2 auf die Grunddrehung.
        let sitzt = feline_pose(0.0, 0.0, 0.0, 0.0, true, false);
        assert!((sitzt.get("body").rotation[0] + std::f32::consts::FRAC_PI_4).abs() < 1e-6);
        assert_eq!(feline_pose(0.0, 0.0, 0.0, 0.0, false, false).get("body").rotation[0], 0.0);
    }

    // Die Spinnenbeine muessen ihre GRUNDdrehung behalten (sie stehen fest schraeg ab) und
    // nur zusaetzlich schwingen -- sonst klappen alle acht Beine in eine Ebene.
    #[test]
    fn spinnenbeine_behalten_ihre_grunddrehung() {
        let model = spider_model("entity/spider/spider");
        let leg = model.parts.iter().find(|p| p.name == "right_front_leg").unwrap();
        assert_ne!(leg.rotation[1], 0.0, "Grunddrehung der Gierung fehlt");
        assert_ne!(leg.rotation[2], 0.0, "Grunddrehung des Rollens fehlt");
        let pose = spider_pose(0.0, 0.0, 1.0, 1.0);
        assert_ne!(pose.get("right_front_leg").rotation[2], 0.0, "Schwingung fehlt");
        // Die Pose ersetzt nichts, sie addiert (s. emit_part).
        assert_eq!(pose.get("right_front_leg").rotation[1], 0.0);
    }
}

#[cfg(test)]
mod default_skin_tests {
    use super::*;

    // Hex-UUID -> Bytes, nur fuer die Testdaten.
    fn uuid(hex: &str) -> [u8; 16] {
        let hex: String = hex.chars().filter(|c| *c != '-').collect();
        let mut bytes = [0u8; 16];
        for (i, b) in bytes.iter_mut().enumerate() {
            *b = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap();
        }
        bytes
    }

    #[test]
    // Die Erwartungswerte stammen NICHT aus meinem Kopf, sondern aus einem Lauf von
    // java.util.UUID.hashCode() + Math.floorMod(hash, 18) mit genau diesen UUIDs -- der
    // Punkt der Uebersetzung ist, dass sie mit Javas Definition uebereinstimmt, und das
    // kann nur ein Vergleich mit Javas Ausgabe zeigen.
    fn hash_stimmt_mit_java_ueberein() {
        // "0000...0" und "ffff...f" ergeben beide Hash 0: die beiden Haelften sind gleich,
        // ihr XOR also 0. Ein Grenzfall, der ein falsches Vorzeichen sofort zeigen wuerde.
        assert_eq!(default_skin(&uuid("00000000-0000-0000-0000-000000000000")).0, DEFAULT_SKINS[0].0);
        assert_eq!(default_skin(&uuid("ffffffff-ffff-ffff-ffff-ffffffffffff")).0, DEFAULT_SKINS[0].0);
        // Hash -369792882 -- floorMod ergibt 0, Rusts `%` waere -6 und damit ein Panic beim
        // Indexzugriff. Genau der Fall, fuer den rem_euclid da ist.
        assert_eq!(default_skin(&uuid("069a79f4-44e9-4726-a5be-fca90e38aaf5")).0, DEFAULT_SKINS[0].0);
        // Hash 1946714239 -> 1, Hash 1548061430 -> 14 (die zweite Haelfte der Tabelle).
        assert_eq!(default_skin(&uuid("853c80ef-3c37-49fd-aa49-938b674adae6")).0, DEFAULT_SKINS[1].0);
        assert_eq!(default_skin(&uuid("b0d3d8a1-3aa1-42c2-bd1a-6c0e6b2d7c9b")).0, DEFAULT_SKINS[14].0);
    }

    #[test]
    // Die Tabelle ist indexbehaftet: die ersten neun Eintraege MUESSEN slim sein, die letzten
    // neun classic (DefaultSkinHelper.SKINS). Wer sie alphabetisch "aufraeumt", verschiebt
    // damit still jedem Spieler ohne Skin das Gesicht und die Armbreite.
    fn tabelle_ist_erst_slim_dann_classic() {
        assert_eq!(DEFAULT_SKINS.len(), 18);
        assert!(DEFAULT_SKINS[..9].iter().all(|(_, slim)| *slim), "erste Haelfte slim");
        assert!(DEFAULT_SKINS[9..].iter().all(|(_, slim)| !*slim), "zweite Haelfte classic");
        assert_eq!(DEFAULT_SKINS[6], ("entity/player/slim/steve", true));
        assert_eq!(DEFAULT_SKINS[15], ("entity/player/wide/steve", false));
    }

    #[test]
    // Der Standard-Skin bestimmt auch die Armbreite: ein Spieler, der auf einen Slim-Eintrag
    // faellt, muss das schmale Modell bekommen. Vorher war das hart classic/Steve.
    fn armbreite_kommt_aus_dem_eintrag() {
        let (texture, slim) = default_skin(&uuid("853c80ef-3c37-49fd-aa49-938b674adae6"));
        assert!(slim, "Index 1 ist ein Slim-Eintrag");
        assert!(texture.contains("/slim/"), "Pfad und Flag muessen zusammenpassen: {texture}");
        for (texture, slim) in DEFAULT_SKINS {
            assert_eq!(slim, texture.contains("/slim/"), "{texture} widerspricht seinem Flag");
        }
    }
}

#[cfg(test)]
mod cape_tests {
    use super::*;

    fn nahe(a: f32, b: f32, was: &str) {
        assert!((a - b).abs() < 1e-4, "{was}: {a} != {b}");
    }

    fn matrizen_gleich(a: Affine, b: Affine, was: &str) {
        for i in 0..3 {
            for j in 0..3 {
                nahe(a.m[i][j], b.m[i][j], &format!("{was} [{i}][{j}]"));
            }
        }
    }

    #[test]
    // Die Grunddrehung des Teils (180 Grad) und die erste Drehung im Quaternion (-180 Grad)
    // heben sich auf -- das ist die Begruendung dafuer, dass cape_model die Grunddrehung
    // weglaesst und cape_pose die komplette Matrix liefert. Waere die Annahme falsch, haenge
    // der Umhang seitenverkehrt vor dem Koerper.
    fn die_beiden_y_drehungen_heben_sich_auf() {
        let leer = Pose::new();
        let pose = cape_pose(&leer, 3.0, 40.0, -8.0);
        let matrix = pose.get("cape").rotation_matrix.expect("cape braucht eine Matrix");
        let ohne_die_zwei =
            rotation_x_deg(6.0 + 40.0 / 2.0 + 3.0).then(rotation_z_deg(-8.0 / 2.0)).then(Affine::rotation_y_deg(180.0 - -8.0 / 2.0));
        matrizen_gleich(matrix, ohne_die_zwei, "Y-Drehungen kuerzen sich");
    }

    #[test]
    // Der Umhang haengt am Rumpf: eine Rumpfdrehung aus der Pose (Schleichen) MUSS ihn
    // mitnehmen. cape_pose baut deshalb auf der Spieler-Pose auf statt eine leere zu
    // erzeugen -- genau das prueft dieser Test.
    fn uebernimmt_die_rumpfhaltung() {
        let schleichend = biped_pose(0.0, 0.0, 0.0, 0.0, true, 0.0);
        let pose = cape_pose(&schleichend, 0.0, 0.0, 0.0);
        let body = pose.get("body");
        nahe(body.rotation[0], 0.5, "Rumpfneigung beim Schleichen");
        nahe(body.pivot_offset[1], 3.2, "Rumpf-Pivot beim Schleichen");
    }

    #[test]
    // Stillstehen: die nachhinkende Position holt die echte ein, alle drei Winkelgroessen
    // gehen gegen 0 (bis auf die 6 Grad Grundneigung in cape_pose). Ein Umhang, der im
    // Stehen wegweht, waere der auffaelligste Fehler hier.
    fn im_stehen_haengt_der_umhang_herunter() {
        let mut state = CapeState::new();
        let pos = [100.0, 64.0, -200.0];
        for _ in 0..200 {
            state.tick(pos);
            state.tick_movement(0.0);
        }
        let (vertical, swing, side) = cape_angles(&state, pos, pos, 90.0, 1.0);
        nahe(vertical, 0.0, "keine Hoehendifferenz");
        nahe(swing, 0.0, "kein Wehen");
        nahe(side, 0.0, "kein Seitwaertsversatz");
    }

    #[test]
    // Der erste Tick muss hart setzen, nicht nachziehen: Vanilla startet die nachhinkende
    // Position bei 0, und die 10-Blocks-Regel ist genau dafuer da. Ohne sie wuerde der
    // Umhang beim Betreten der Welt aus dem Weltursprung herangezogen.
    fn erster_tick_setzt_hart() {
        let mut state = CapeState::new();
        state.tick([1000.0, 70.0, -1000.0]);
        let p = state.lerp_pos(1.0);
        assert_eq!(p, [1000.0, 70.0, -1000.0], "erster Tick muss springen, nicht kriechen");
        // Und der Sprung darf keine Bewegung vortaeuschen: last == aktuell.
        assert_eq!(state.lerp_pos(0.0), p, "kein Nachziehen ueber den Sprung hinweg");
    }

    #[test]
    // Vorwaerts laufen laesst den Umhang nach HINTEN wehen (swing > 0), rueckwaerts laufen
    // darf ihn NICHT nach vorne durch den Koerper klappen -- dafuer ist die Untergrenze 0.
    fn vorwaerts_weht_nach_hinten_rueckwaerts_klappt_nicht_um() {
        // Blickrichtung 0 Grad = nach Sueden (+Z) in Minecraft.
        let laufen = |dz: f64| {
            let mut state = CapeState::new();
            let mut pos = [0.0, 64.0, 0.0];
            state.tick(pos);
            let mut last = pos;
            for _ in 0..20 {
                last = pos;
                pos[2] += dz;
                state.tick(pos);
                state.tick_movement(0.1);
            }
            cape_angles(&state, pos, last, 0.0, 1.0)
        };
        let (_, swing_vor, _) = laufen(0.216);
        let (_, swing_zurueck, _) = laufen(-0.216);
        assert!(swing_vor > 5.0, "vorwaerts muss deutlich wehen, war {swing_vor}");
        assert_eq!(swing_zurueck, 0.0, "rueckwaerts darf nicht negativ werden, war {swing_zurueck}");
    }

    #[test]
    // Das Lauf-Schwingen haengt an add_distance_moved, das nur der eigene Spieler aufruft.
    // Ohne diese Aufrufe (fremde Spieler) muss der Term wegfallen -- sonst wackelt ein
    // fremder Umhang anders als in Vanilla.
    fn lauf_schwingen_nur_mit_zurueckgelegter_strecke() {
        let bauen = |mit_strecke: bool| {
            let mut state = CapeState::new();
            let mut pos = [0.0, 64.0, 0.0];
            state.tick(pos);
            for _ in 0..40 {
                pos[2] += 0.216;
                state.tick(pos);
                state.tick_movement(0.1);
                if mit_strecke {
                    state.add_distance_moved(0.216);
                }
            }
            state
        };
        let eigen = bauen(true);
        let fremd = bauen(false);
        assert_eq!(fremd.lerp_distance_moved(1.0), 0.0, "ohne Aufruf bleibt die Strecke 0");
        assert!(eigen.lerp_distance_moved(1.0) > 0.0, "der eigene Spieler summiert Strecke");
        // Die Laufgroesse (Amplitude des Schwingens) laeuft in beiden Faellen hoch -- der
        // Unterschied steckt allein in der Phase.
        nahe(eigen.lerp_movement(1.0), fremd.lerp_movement(1.0), "Laufgroesse identisch");
    }

    #[test]
    // Ein Teleport (mehr als 10 Blocks) setzt hart, statt den Umhang quer durch die Welt zu
    // ziehen. Genau dafuer ist die Schwelle da.
    fn teleport_setzt_hart_statt_zu_ziehen() {
        let mut state = CapeState::new();
        let start = [0.0, 64.0, 0.0];
        for _ in 0..50 {
            state.tick(start);
        }
        let ziel = [500.0, 64.0, 500.0];
        state.tick(ziel);
        assert_eq!(state.lerp_pos(1.0), ziel, "Teleport muss springen");
        let (_, swing, side) = cape_angles(&state, ziel, ziel, 0.0, 1.0);
        nahe(swing, 0.0, "nach dem Sprung kein Wehen");
        nahe(side, 0.0, "nach dem Sprung kein Versatz");
    }

    #[test]
    // Ein kleiner Schritt (unter der Schwelle) wird NACHGEZOGEN, und zwar um genau ein
    // Viertel der Differenz. Das ist die Zahl, die die gesamte Umhangbewegung bestimmt.
    fn kleiner_schritt_wird_um_ein_viertel_nachgezogen() {
        let mut state = CapeState::new();
        state.tick([0.0, 0.0, 0.0]);
        state.tick([0.0, 0.0, 4.0]);
        nahe(state.lerp_pos(1.0)[2] as f32, 1.0, "ein Viertel von 4");
        state.tick([0.0, 0.0, 4.0]);
        nahe(state.lerp_pos(1.0)[2] as f32, 1.75, "ein Viertel des Rests");
    }

    #[test]
    // Die Kastenmasse und die Texturgroesse des Umhangs. Vanilla baut ihn mit
    // textureScale (1.0, 0.5) auf eine 64x64-Vorlage -- was rechnerisch 64x32 ist, und
    // genau so gross sind die Cape-Bilder von Mojang (nachgeprueft an einem echten Abruf).
    fn umhang_masse_und_texturgroesse() {
        let model = cape_model();
        assert_eq!(model.tex_size, [64.0, 32.0]);
        let body = &model.parts[0];
        assert_eq!(body.name, "body");
        assert!(body.cuboids.is_empty(), "der Rumpf ist nur ein Gelenk, keine Geometrie");
        let cape = &body.children[0];
        assert_eq!(cape.name, "cape");
        assert_eq!(cape.pivot, [0.0, 0.0, 2.0]);
        assert_eq!(cape.cuboids[0].offset, [-5.0, 0.0, -1.0]);
        assert_eq!(cape.cuboids[0].size, [10.0, 16.0, 1.0]);
        assert_eq!(cape.cuboids[0].uv, [0.0, 0.0]);
    }
}

#[cfg(test)]
mod arm_sway_tests {
    use super::*;

    fn arme(age: f32) -> ([f32; 3], [f32; 3]) {
        let pose = biped_pose(0.0, 0.0, 0.0, 0.0, false, age);
        (pose.get("right_arm").rotation, pose.get("left_arm").rotation)
    }

    #[test]
    // Der Punkt: die Arme liegen NIE am Koerper an. Der Roll-Term ist
    // cos(...)*0.05 + 0.05, also 0..0.1 rad -- nie negativ. Ohne den Summanden 0.05 waere er
    // symmetrisch um Null und die Arme klebten die halbe Zeit am Rumpf.
    fn arme_zeigen_immer_nach_aussen() {
        // Ueber mehrere vollstaendige Perioden abtasten (Roll-Periode ~70 Ticks).
        for i in 0..2000 {
            let age = i as f32 * 0.25;
            let (rechts, links) = arme(age);
            assert!(rechts[2] >= -1e-6, "rechter Arm bei age={age} nach innen: {}", rechts[2]);
            assert!(links[2] <= 1e-6, "linker Arm bei age={age} nach innen: {}", links[2]);
        }
    }

    #[test]
    // Der Ausschlag: 0 bis 0.1 rad (5,7 Grad), und beide Grenzen werden tatsaechlich erreicht.
    // Ein zu kleiner Wert waere unsichtbar, ein zu grosser saehe nach Fluegeln aus.
    fn ausschlag_trifft_die_vanilla_grenzen() {
        let mut min = f32::MAX;
        let mut max = f32::MIN;
        for i in 0..2000 {
            let roll = arme(i as f32 * 0.25).0[2];
            min = min.min(roll);
            max = max.max(roll);
        }
        assert!(min.abs() < 1e-3, "Minimum muss 0 sein, war {min}");
        assert!((max - 0.1).abs() < 1e-3, "Maximum muss 0.1 rad sein, war {max}");
    }

    #[test]
    // Die Arme bewegen sich spiegelbildlich (sigma = +1 / -1). Waere das nicht so, ruderten
    // beide in dieselbe Richtung.
    fn beide_arme_sind_spiegelbildlich() {
        for i in 0..200 {
            let age = i as f32 * 0.5;
            let (rechts, links) = arme(age);
            assert!((rechts[2] + links[2]).abs() < 1e-6, "Roll nicht spiegelbildlich bei age={age}");
            assert!((rechts[0] + links[0]).abs() < 1e-6, "Pitch nicht spiegelbildlich bei age={age}");
        }
    }

    #[test]
    // Zwei VERSCHIEDENE Perioden (Roll 0.09/Tick, Pitch 0.067/Tick) -- daran haengt, dass die
    // Bewegung sich nie exakt wiederholt. Waeren sie gleich, saehe man ein mechanisches Auf
    // und Ab. Geprueft am Abstand der jeweiligen Maxima.
    fn roll_und_pitch_haben_verschiedene_perioden() {
        let periode = |werte: &dyn Fn(f32) -> f32| {
            // Erstes Maximum nach age=0 suchen (feine Abtastung, die Perioden sind ~70/94 Ticks).
            let mut bestes = (0.0f32, f32::MIN);
            for i in 1..4000 {
                let age = i as f32 * 0.05;
                let v = werte(age);
                if v > bestes.1 {
                    bestes = (age, v);
                }
            }
            bestes.0
        };
        // Roll ist bei age=0 schon maximal (cos(0)=1), also das ZWEITE Maximum: eine Periode.
        let roll_periode = periode(&|age| arme(age).0[2]);
        let pitch_max = periode(&|age| arme(age).0[0]);
        assert!((roll_periode - 69.8).abs() < 1.0, "Roll-Periode war {roll_periode}, erwartet ~69.8 Ticks");
        // Pitch ist ein Sinus: erstes Maximum nach einer Viertelperiode, ~23.4 Ticks.
        assert!((pitch_max - 23.4).abs() < 1.0, "Pitch-Maximum war {pitch_max}, erwartet ~23.4 Ticks");
    }

    #[test]
    // Die Ruhe-Bewegung ADDIERT auf alles andere, sie ersetzt es nicht: Schrittschwingen und
    // Schleichen muessen erhalten bleiben. In Vanilla stehen die swingArm-Aufrufe deshalb am
    // Ende von setAngles.
    fn sway_addiert_auf_schritt_und_schleichen() {
        // Laufen: das Schrittschwingen sitzt im Pitch und ist viel groesser als die Sway.
        let laufen = biped_pose(0.0, 0.0, 5.0, 1.0, false, 0.0);
        let stehen = biped_pose(0.0, 0.0, 0.0, 0.0, false, 0.0);
        assert!(
            (laufen.get("right_arm").rotation[0] - stehen.get("right_arm").rotation[0]).abs() > 0.1,
            "das Schrittschwingen darf von der Sway nicht verschluckt werden"
        );
        // Schleichen: +0.4 Pitch plus die Sway obendrauf, und der Pivot bleibt verschoben.
        let schleichen = biped_pose(0.0, 0.0, 0.0, 0.0, true, 0.0);
        let sway_pitch = stehen.get("right_arm").rotation[0];
        assert!((schleichen.get("right_arm").rotation[0] - (0.4 + sway_pitch)).abs() < 1e-5);
        assert_eq!(schleichen.get("right_arm").pivot_offset[1], 3.2, "Schleich-Pivot muss bleiben");
        // Und auch beim Schleichen zeigen die Arme nach aussen.
        assert!(schleichen.get("right_arm").rotation[2] >= 0.0);
    }
}

#[cfg(test)]
mod body_yaw_tests {
    use super::*;

    #[test]
    // Der kurze Weg beim Interpolieren: von 350 nach 10 Grad sind es 20 Grad ueber die Null,
    // nicht 340 Grad rueckwaerts. Ohne wrap_degrees drehte sich die Figur bei jedem
    // Nulldurchgang einmal komplett um.
    fn winkel_interpolation_nimmt_den_kurzen_weg() {
        let mitte = lerp_angle_degrees(0.5, 350.0, 10.0);
        assert!((wrap_degrees(mitte) - 0.0).abs() < 1e-4, "Mitte war {mitte}");
        assert_eq!(wrap_degrees(190.0), -170.0);
        assert_eq!(wrap_degrees(-190.0), 170.0);
        assert_eq!(wrap_degrees(180.0), -180.0);
    }

    #[test]
    // Das Kernverhalten, das der Nutzer beschrieben hat: im STAND folgt der Kopf der Maus
    // sofort, der Koerper bleibt stehen -- bis zur 50-Grad-Grenze.
    fn im_stand_dreht_sich_nur_der_kopf() {
        let mut b = BodyYaw::new(0.0);
        // 40 Grad umschauen, ohne sich zu bewegen.
        b.tick(40.0, 0.0, 0.0, false);
        assert_eq!(b.body_yaw(1.0), 0.0, "der Koerper darf sich nicht bewegen");
        assert!((b.relative_head_yaw(1.0) - 40.0).abs() < 1e-4, "der Kopf muss voll folgen");
    }

    #[test]
    // Ueber 50 Grad hinaus wird der Koerper MITGEZOGEN -- genau das fuehlt sich an wie
    // "traege": erst gibt der Koerper nicht nach, dann schleppt der Kopf ihn mit.
    fn ueber_funfzig_grad_zieht_der_kopf_den_koerper_mit() {
        let mut b = BodyYaw::new(0.0);
        b.tick(120.0, 0.0, 0.0, false);
        let relativ = b.relative_head_yaw(1.0);
        assert!((relativ.abs() - 50.0).abs() < 1e-3, "die Verdrehung muss auf 50 Grad begrenzt sein, war {relativ}");
        assert!((b.body_yaw(1.0) - 70.0).abs() < 1e-3, "der Koerper muss auf 70 Grad nachgezogen sein");
    }

    #[test]
    // Laufen: der Koerper dreht sich zur BEWEGUNGSRICHTUNG, nicht zur Blickrichtung. Wer
    // seitwaerts laeuft und nach vorn schaut, dreht den Koerper zur Laufrichtung.
    //
    // Minecraft-Konvention: yaw 0 = Sued (+Z), 90 = West (-X).
    fn beim_laufen_folgt_der_koerper_der_bewegungsrichtung() {
        let mut b = BodyYaw::new(0.0);
        // Nach Sueden laufen (+Z) und dabei nach Sueden schauen -> Ziel 0, bleibt 0.
        for _ in 0..20 {
            b.tick(0.0, 0.0, 0.2, false);
        }
        assert!(b.body_yaw(1.0).abs() < 1e-2, "nach Sueden laufen -> Koerper nach Sueden");

        // Nach Westen laufen (-X), weiter nach Sueden schauen. Bewegungsrichtung ist yaw 90,
        // aber die 50-Grad-Grenze laesst den Koerper nur bis 50 kommen.
        let mut b = BodyYaw::new(0.0);
        for _ in 0..40 {
            b.tick(0.0, -0.2, 0.0, false);
        }
        let koerper = b.body_yaw(1.0);
        assert!((koerper - 50.0).abs() < 1e-2, "Koerper muss an der 50-Grad-Grenze haengen, war {koerper}");
        assert!((b.relative_head_yaw(1.0) + 50.0).abs() < 1e-2, "der Kopf schaut 50 Grad zurueck");
    }

    #[test]
    // Rueckwaerts laufen: Blick und Bewegung mehr als 95 Grad auseinander -> der Koerper
    // dreht sich um 180 Grad, statt sich zu verrenken. Ohne diesen Zweig liefe die Figur
    // rueckwaerts mit verdrehtem Oberkoerper.
    fn rueckwaerts_laufen_dreht_den_koerper_um() {
        let mut b = BodyYaw::new(0.0);
        // Blick nach Sueden (0), Bewegung nach Norden (-Z) -> 180 Grad Unterschied.
        for _ in 0..40 {
            b.tick(0.0, 0.0, -0.2, false);
        }
        // Ziel waere 180-180 = 0, der Koerper bleibt also bei 0 und schaut mit dem Blick
        // in dieselbe Richtung -- man laeuft rueckwaerts.
        assert!(b.body_yaw(1.0).abs() < 1e-2, "Koerper bleibt in Blickrichtung, war {}", b.body_yaw(1.0));
        assert!(b.relative_head_yaw(1.0).abs() < 1e-2, "kein verdrehter Hals beim Rueckwaertslaufen");
    }

    #[test]
    // Die 30 % pro Tick: der Koerper springt nicht auf sein Ziel, sondern kriecht dorthin.
    // Das ist die eigentliche "Traegheit".
    fn koerper_holt_dreissig_prozent_pro_tick_auf() {
        let mut b = BodyYaw::new(0.0);
        // Zuschlagen setzt das Ziel auf die Blickrichtung -- so laesst sich der reine
        // Nachlauf ohne den Bewegungszweig pruefen. 40 Grad bleiben unter der 50-Grad-Grenze.
        b.tick(40.0, 0.0, 0.0, true);
        assert!((b.body_yaw(1.0) - 12.0).abs() < 1e-3, "erster Tick: 30 % von 40 = 12, war {}", b.body_yaw(1.0));
        b.tick(40.0, 0.0, 0.0, true);
        assert!((b.body_yaw(1.0) - 20.4).abs() < 1e-3, "zweiter Tick: +30 % vom Rest, war {}", b.body_yaw(1.0));
    }

    #[test]
    // Zwischentick-Interpolation: der Koerper gleitet, statt 20 mal pro Sekunde zu springen.
    fn koerper_gleitet_zwischen_den_ticks() {
        let mut b = BodyYaw::new(0.0);
        b.tick(40.0, 0.0, 0.0, true);
        let anfang = b.body_yaw(0.0);
        let mitte = b.body_yaw(0.5);
        let ende = b.body_yaw(1.0);
        assert!(anfang < mitte && mitte < ende, "{anfang} / {mitte} / {ende}");
        assert!((mitte - ende / 2.0).abs() < 1e-3, "die Mitte muss genau dazwischen liegen");
    }

    #[test]
    // Ein Nulldurchgang darf nicht zu einer Volldrehung fuehren. Blick von 350 auf 10 Grad,
    // der Koerper folgt -- und muss dabei ueber die Null gehen, nicht 340 Grad zurueck.
    fn nulldurchgang_dreht_nicht_komplett_herum() {
        let mut b = BodyYaw::new(350.0);
        b.tick(10.0, 0.0, 0.0, true);
        let bewegung = wrap_degrees(b.body_yaw(1.0) - 350.0);
        assert!(bewegung > 0.0 && bewegung < 20.0, "Koerper muss vorwaerts ueber die Null, war {bewegung}");
    }
}

#[cfg(test)]
mod diffuse_tests {
    use super::*;

    fn nahe(a: f32, b: f32, was: &str) {
        assert!((a - b).abs() < 1e-3, "{was}: {a} != {b}");
    }

    #[test]
    // Die sechs achsenparallelen Flaechen, nachgerechnet aus Vanillas Lichtvektoren und der
    // Formel aus light.glsl. Die Zahlen sind ausgerechnet, nicht gewaehlt.
    fn achsenflaechen_treffen_die_vanilla_werte() {
        nahe(diffuse_shade([0.0, 1.0, 0.0]), 1.0, "oben");
        nahe(diffuse_shade([0.0, -1.0, 0.0]), 0.4, "unten");
        nahe(diffuse_shade([0.0, 0.0, -1.0]), 0.7395, "nord");
        nahe(diffuse_shade([0.0, 0.0, 1.0]), 0.7395, "sued");
        nahe(diffuse_shade([1.0, 0.0, 0.0]), 0.4970, "ost");
        nahe(diffuse_shade([-1.0, 0.0, 0.0]), 0.4970, "west");
    }

    #[test]
    // Zwei Plausibilitaeten, an denen ich bei der GUI-Beleuchtung schon einmal gescheitert bin:
    // oben MUSS die hellste Flaeche sein und unten die dunkelste. Kommt es anders heraus, ist
    // ein Vorzeichen verdreht -- und das faellt am fertigen Bild kaum auf, weil es "auch
    // irgendwie schattiert" aussieht.
    fn oben_ist_am_hellsten_unten_am_dunkelsten() {
        let richtungen = [[0.0, 1.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, -1.0], [1.0, 0.0, 0.0], [-1.0, 0.0, 0.0]];
        let werte: Vec<f32> = richtungen.iter().map(|n| diffuse_shade(*n)).collect();
        let max = werte.iter().copied().fold(f32::MIN, f32::max);
        let min = werte.iter().copied().fold(f32::MAX, f32::min);
        nahe(diffuse_shade([0.0, 1.0, 0.0]), max, "oben ist die hellste Flaeche");
        nahe(diffuse_shade([0.0, -1.0, 0.0]), min, "unten ist die dunkelste Flaeche");
    }

    #[test]
    // Keine Flaeche wird ganz schwarz -- dafuer ist MINECRAFT_AMBIENT_LIGHT (0.4) da -- und
    // keine ueberstrahlt.
    fn helligkeit_bleibt_im_erlaubten_bereich() {
        for i in 0..64 {
            for j in 0..64 {
                let (a, b) = (i as f32 / 63.0 * std::f32::consts::TAU, j as f32 / 63.0 * std::f32::consts::PI);
                let n = [a.cos() * b.sin(), b.cos(), a.sin() * b.sin()];
                let shade = diffuse_shade(n);
                assert!(shade >= AMBIENT_LIGHT - 1e-6, "zu dunkel: {shade}");
                assert!(shade <= 1.0 + 1e-6, "zu hell: {shade}");
            }
        }
    }

    #[test]
    // Der Kern des Ganzen: dreht sich das Entity, WANDERT die Helligkeit einer Flaeche. Eine
    // fest je Modellflaeche eingebackene Tabelle (wie bei Bloecken) waere ueber die Drehung
    // konstant -- genau das soll dieser Test verhindern.
    fn helligkeit_wandert_beim_drehen() {
        let shade_bei = |grad: f32| {
            let base = Affine::rotation_y_deg(grad);
            diffuse_shade(base.apply_direction([0.0, 0.0, 1.0]))
        };
        nahe(shade_bei(0.0), 0.7395, "unverdreht");
        nahe(shade_bei(90.0), 0.4970, "eine Vierteldrehung");
        assert!(shade_bei(0.0) > shade_bei(60.0), "die Helligkeit muss sich beim Drehen aendern");
        // Und sie muss stetig verlaufen, nicht springen.
        let mut vorher = shade_bei(0.0);
        for grad in 1..=360 {
            let jetzt = shade_bei(grad as f32);
            assert!((jetzt - vorher).abs() < 0.02, "Sprung bei {grad} Grad: {vorher} -> {jetzt}");
            vorher = jetzt;
        }
    }

    #[test]
    // Die Einbettung dreht das Modell mit scale(-1,-1,1) (s. entity_transform). Die
    // Modellraum-Normale "unten" (0,-1,0) muss dadurch in der WELT nach oben zeigen und die
    // hellste Flaeche werden. Waere das falsch, waere die Figur von unten beleuchtet.
    fn modellraum_unten_wird_in_der_welt_oben() {
        let base = entity_transform([0.0, 64.0, 0.0], 0.0, 1.0, PLAYER_MODEL_SCALE);
        let welt = base.apply_direction([0.0, -1.0, 0.0]);
        nahe(welt[1], 1.0, "Modellraum-unten zeigt in der Welt nach oben");
        nahe(diffuse_shade(welt), 1.0, "und ist damit die hellste Flaeche");
    }
}

#[cfg(test)]
mod swim_tests {
    use super::*;

    const PI: f32 = std::f32::consts::PI;

    fn schwimmend(swing: f32, lean: f32) -> Pose {
        biped_pose_leaning(0.0, 0.0, swing, 1.0, false, lean, 0.0)
    }

    #[test]
    // Ohne Neigung darf die Schwimmbewegung NICHTS tun -- sonst wuerde sie an Land in die
    // Laufanimation hineinpfuschen.
    fn an_land_keine_wirkung() {
        let laufen = biped_pose_leaning(0.0, 0.0, 5.0, 1.0, false, 0.0, 0.0);
        let nur_laufen = biped_pose(0.0, 0.0, 5.0, 1.0, false, 0.0);
        assert_eq!(laufen.get("right_arm").rotation, nur_laufen.get("right_arm").rotation);
        assert_eq!(laufen.get("left_leg").rotation, nur_laufen.get("left_leg").rotation);
    }

    #[test]
    // Voll eingetaucht zeigen beide Arme nach HINTEN (yaw = PI). Das ist die Grundhaltung des
    // Kraulens -- ohne sie ruderte die Figur seitlich statt nach vorn.
    fn voll_eingetaucht_zeigen_die_arme_nach_hinten() {
        for swing in [0.0, 5.0, 13.9, 14.0, 20.0, 25.9] {
            let pose = schwimmend(swing, 1.0);
            let links = pose.get("left_arm").rotation[1];
            let rechts = pose.get("right_arm").rotation[1];
            assert!((wrap_radians(links - PI)).abs() < 1e-4, "linker Arm bei {swing}: yaw {links}");
            assert!((wrap_radians(rechts - PI)).abs() < 1e-4, "rechter Arm bei {swing}: yaw {rechts}");
        }
    }

    #[test]
    // Die drei Abschnitte des Zyklus, an ihren Grenzen geprueft. Der Zug (i < 14) haelt die
    // Arme flach (pitch 0), das Vorholen hebt sie bis PI/2, das Zuruecksetzen senkt sie wieder.
    fn die_drei_abschnitte_stimmen() {
        let pitch = |swing: f32| schwimmend(swing, 1.0).get("right_arm").rotation[0];
        assert!(pitch(0.0).abs() < 1e-4, "Zuganfang: Arme flach");
        assert!(pitch(13.9).abs() < 1e-4, "Zugende: immer noch flach");
        assert!(pitch(14.0).abs() < 1e-4, "Vorholen beginnt bei 0");
        assert!((pitch(18.0) - PI / 4.0).abs() < 1e-3, "Vorholen Mitte: PI/4, war {}", pitch(18.0));
        assert!((pitch(21.99) - PI / 2.0).abs() < 1e-2, "Vorholen Ende: fast PI/2");
        assert!((pitch(22.0) - PI / 2.0).abs() < 1e-4, "Zuruecksetzen beginnt oben");
        assert!(pitch(25.99).abs() < 1e-2, "Zuruecksetzen endet unten");
    }

    #[test]
    // Der Zyklus wiederholt sich alle 26 Einheiten -- und rem_euclid muss auch mit negativen
    // Phasen umgehen (die Schrittphase wird zwischen den Ticks zurueckgerechnet und kann kurz
    // unter null liegen, s. LimbAnimator::animation_progress).
    fn zyklus_wiederholt_sich_und_vertraegt_negative_phasen() {
        let a = schwimmend(3.0, 1.0).get("right_arm").rotation;
        let b = schwimmend(3.0 + 26.0, 1.0).get("right_arm").rotation;
        for i in 0..3 {
            assert!((a[i] - b[i]).abs() < 1e-4, "Achse {i}: {} != {}", a[i], b[i]);
        }
        // Darf nicht panicken und muss im gueltigen Zyklus landen.
        let negativ = schwimmend(-1.0, 1.0).get("right_arm").rotation;
        assert!(negativ.iter().all(|v| v.is_finite()));
    }

    #[test]
    // Halb eingetaucht muss die Haltung ZWISCHEN Laufen und Schwimmen liegen. Das ist der
    // weiche Uebergang beim Eintauchen -- ohne ihn wuerde die Figur umspringen.
    fn halbe_neigung_mischt_zwischen_laufen_und_schwimmen() {
        let land = schwimmend(5.0, 0.0).get("right_arm").rotation[1];
        let wasser = schwimmend(5.0, 1.0).get("right_arm").rotation[1];
        let halb = schwimmend(5.0, 0.5).get("right_arm").rotation[1];
        assert!((halb - f32::midpoint(land, wasser)).abs() < 1e-4, "halb war {halb}, erwartet zwischen {land} und {wasser}");
    }

    #[test]
    // Die Beine treten gegenphasig und deutlich langsamer als beim Laufen (Frequenz 1/3 statt
    // 0.6662, Ausschlag 0.3 statt 1.4). Beide Beine gleichphasig waere ein Delfinschlag statt
    // Kraulen.
    fn beine_treten_gegenphasig_und_flach() {
        let mut max_ausschlag: f32 = 0.0;
        for i in 0..200 {
            let swing = i as f32 * 0.25;
            let pose = schwimmend(swing, 1.0);
            let links = pose.get("left_leg").rotation[0];
            let rechts = pose.get("right_leg").rotation[0];
            assert!((links + rechts).abs() < 1e-4, "Beine muessen gegenphasig sein bei {swing}");
            max_ausschlag = max_ausschlag.max(rechts.abs());
        }
        assert!((max_ausschlag - 0.3).abs() < 1e-3, "Ausschlag muss 0.3 sein, war {max_ausschlag}");
    }

    #[test]
    // Der Kopf richtet sich beim Schwimmen nach vorn aus (-PI/4), sonst schaute die
    // waagerecht liegende Figur in den Boden. Das war schon vorher da -- der Test haelt fest,
    // dass die Schwimmbewegung es nicht ueberschreibt.
    fn kopf_bleibt_nach_vorn_gerichtet() {
        let pose = schwimmend(5.0, 1.0);
        assert!((pose.get("head").rotation[0] + PI / 4.0).abs() < 1e-4);
    }
}

#[cfg(test)]
mod first_person_tests {
    use super::*;

    // Wo der Arm in der Ego-Ansicht landet: die Kette auf den Mittelpunkt des Arm-Kastens
    // angewandt. Der Arm-Pivot liegt bei (-5, 2, 0) Modellpixeln, der Kasten reicht von dort
    // 12 Pixel nach unten.
    fn arm_position(right: bool, equip: f32, swing: f32) -> [f32; 3] {
        let base = first_person_arm_transform(right, equip, swing);
        // JEDER Arm hat seinen eigenen Pivot: der rechte bei x = -5, der linke bei x = +5
        // Modellpixeln. Der erste Anlauf dieses Tests hat fuer beide den rechten genommen und
        // deshalb faelschlich einen Spiegelfehler angezeigt.
        let pivot_x = if right { -5.0 / 16.0 } else { 5.0 / 16.0 };
        base.apply([pivot_x, 2.0 / 16.0, 0.0])
    }

    #[test]
    // Der rechte Arm muss rechts vom Auge und unterhalb sitzen, und VOR der Kamera (negatives
    // Z im Sichtraum -- dorthin schaut man). Das ist die Grundprobe: stimmt eines der
    // Vorzeichen in der Kette nicht, landet der Arm hinter dem Kopf und man sieht nichts.
    fn rechter_arm_sitzt_rechts_unten_vor_der_kamera() {
        let p = arm_position(true, 0.0, 0.0);
        assert!(p[0] > 0.0, "muss rechts der Mitte liegen, war x={}", p[0]);
        assert!(p[1] < 0.0, "muss unterhalb der Augenhoehe liegen, war y={}", p[1]);
        assert!(p[2] < 0.0, "muss VOR der Kamera liegen, war z={}", p[2]);
    }

    #[test]
    // Der linke Arm ist das Spiegelbild: gleiche Hoehe und Tiefe, x umgekehrt.
    fn linker_arm_ist_das_spiegelbild() {
        let r = arm_position(true, 0.0, 0.0);
        let l = arm_position(false, 0.0, 0.0);
        assert!((r[0] + l[0]).abs() < 1e-5, "x muss gespiegelt sein: {} / {}", r[0], l[0]);
        assert!((r[1] - l[1]).abs() < 1e-5, "y muss gleich sein");
        assert!((r[2] - l[2]).abs() < 1e-5, "z muss gleich sein");
    }

    #[test]
    // Die Wechsel-Animation senkt die Hand: bei equipProgress 1 liegt sie tiefer als bei 0.
    // Der Term ist `equipProgress * -0.6` auf der Y-Achse.
    fn wechsel_animation_senkt_die_hand() {
        let oben = arm_position(true, 0.0, 0.0);
        let unten = arm_position(true, 1.0, 0.0);
        assert!(unten[1] < oben[1], "bei laufendem Wechsel muss die Hand tiefer sein");
        assert!((oben[1] - unten[1] - 0.6).abs() < 1e-4, "der Versatz muss genau 0.6 sein");
    }

    #[test]
    // Waehrend eines Schlags bewegt sich die Hand tatsaechlich -- und kehrt am Ende des
    // Schwungs (Fortschritt 1) wieder in die Ruhelage zurueck.
    fn schlag_bewegt_die_hand_und_kehrt_zurueck() {
        let ruhe = arm_position(true, 0.0, 0.0);
        let mitte = arm_position(true, 0.0, 0.5);
        let ende = arm_position(true, 0.0, 1.0);
        let abstand = |a: [f32; 3], b: [f32; 3]| ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
        assert!(abstand(ruhe, mitte) > 0.1, "in der Schwungmitte muss sich die Hand deutlich bewegt haben");
        assert!(abstand(ruhe, ende) < 1e-4, "am Schwungende ist sie wieder in Ruhelage");
    }

    #[test]
    // Stetigkeit ohne willkuerliche Schwelle: verdoppelt man die Abtastdichte, muss sich der
    // groesste Einzelschritt etwa HALBIEREN. Das ist genau die Aussage "die Bewegung ist
    // stetig" -- an einem echten Sprung wuerde feineres Abtasten den groessten Schritt gar
    // nicht verkleinern.
    //
    // Ein erster Anlauf hat stattdessen "Schritt < 0.1 bei 100 Stuetzstellen" geprueft und
    // fehlgeschlagen: der groesste Schritt liegt dort bei rund 0.8. Die Hand bewegt sich
    // waehrend eines Schlags einfach schnell -- die Schwelle war geraten, nicht gemessen.
    fn schlagbewegung_ist_stetig() {
        let groesster_schritt = |stuetzstellen: usize| {
            let mut max = 0.0f32;
            let mut vorher = arm_position(true, 0.0, 0.0);
            for i in 1..=stuetzstellen {
                let jetzt = arm_position(true, 0.0, i as f32 / stuetzstellen as f32);
                let d = ((jetzt[0] - vorher[0]).powi(2) + (jetzt[1] - vorher[1]).powi(2) + (jetzt[2] - vorher[2]).powi(2)).sqrt();
                max = max.max(d);
                vorher = jetzt;
            }
            max
        };
        let grob = groesster_schritt(500);
        let fein = groesster_schritt(1000);
        let verhaeltnis = fein / grob;
        // Erwartet wird 1/sqrt(2) = 0.707, NICHT 1/2. Grund: der Schwung laeuft ueber
        // g = sqrt(swingProgress), und die Wurzel hat bei 0 eine unendliche Steigung -- die
        // Hand schnellt zu Beginn des Schlags sehr schnell heraus und laeuft zum Ende hin
        // aus. Der groesste Schritt liegt damit immer direkt am Anfang und schrumpft nur mit
        // der Wurzel der Abtastdichte.
        //
        // Die obere Schranke ist die eigentliche Pruefung: an einem echten SPRUNG bliebe der
        // groesste Schritt beim Verfeinern gleich (Verhaeltnis ~1.0). Die untere haelt fest,
        // dass es der Wurzel-Anlauf ist und keine glatte Bewegung.
        assert!(verhaeltnis < 0.8, "kein Sprung erlaubt: {grob} -> {fein} (Verhaeltnis {verhaeltnis})");
        assert!(verhaeltnis > 0.6, "der Wurzel-Anlauf muss erhalten bleiben: Verhaeltnis {verhaeltnis}");
    }

    #[test]
    // Das Arm-Modell enthaelt genau den einen Arm samt Aermel -- nicht den ganzen Spieler.
    fn arm_modell_enthaelt_nur_den_arm() {
        for slim in [false, true] {
            let model = player_arm_model(slim, true);
            assert_eq!(model.parts.len(), 1, "nur ein Teil");
            assert_eq!(model.parts[0].name, "right_arm");
            assert_eq!(model.parts[0].children.len(), 1, "der Aermel gehoert dazu");
            assert_eq!(model.parts[0].children[0].name, "right_sleeve");
            assert_eq!(model.tex_size, [64.0, 64.0], "Skin-Format");
        }
        assert_eq!(player_arm_model(false, false).parts[0].name, "left_arm");
        // Schmal: der Arm ist 3 statt 4 Pixel breit.
        assert_eq!(player_arm_model(true, true).parts[0].cuboids[0].size[0], 3.0);
        assert_eq!(player_arm_model(false, true).parts[0].cuboids[0].size[0], 4.0);
    }

    #[test]
    // Die Haltung: nur ein leichtes Rollen nach aussen, sonst Grundstellung.
    fn haltung_ist_grundstellung_mit_leichtem_rollen() {
        let rechts = first_person_arm_pose(true).get("right_arm").rotation;
        assert_eq!(rechts, [0.0, 0.0, 0.1]);
        let links = first_person_arm_pose(false).get("left_arm").rotation;
        assert_eq!(links, [0.0, 0.0, -0.1]);
    }
}

#[cfg(test)]
mod rotation_xyz_tests {
    use super::*;

    fn nah(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-5)
    }

    // Einzelachsen: eine 90-Grad-Drehung um X schickt +Y nach +Z (Rechte-Hand-Regel).
    #[test]
    fn einzelne_achsen_drehen_richtig() {
        assert!(nah(Affine::rotation_xyz_deg(90.0, 0.0, 0.0).apply([0.0, 1.0, 0.0]), [0.0, 0.0, 1.0]));
        assert!(nah(Affine::rotation_xyz_deg(0.0, 90.0, 0.0).apply([0.0, 0.0, 1.0]), [1.0, 0.0, 0.0]));
        assert!(nah(Affine::rotation_xyz_deg(0.0, 0.0, 90.0).apply([1.0, 0.0, 0.0]), [0.0, 1.0, 0.0]));
    }

    // Der springende Punkt gegenueber rotation_zyx: bei gemischten Winkeln kommt eine ANDERE
    // Drehung heraus. Waere die Reihenfolge egal, koennte man eine der beiden weglassen --
    // und das Item laege bei jedem Schwert schief in der Hand.
    #[test]
    fn xyz_ist_nicht_zyx() {
        let xyz = Affine::rotation_xyz_deg(0.0, -90.0, 55.0).apply([1.0, 0.0, 0.0]);
        let zyx = Affine::rotation_zyx(0.0, (-90.0f32).to_radians(), 55.0f32.to_radians()).apply([1.0, 0.0, 0.0]);
        assert!(!nah(xyz, zyx), "XYZ und ZYX duerfen nicht dasselbe liefern: {xyz:?} / {zyx:?}");
    }

    // Zusammengesetzt muss die Matrix genau Rx * Ry * Rz sein -- gegen die Einzelmatrizen
    // nachgerechnet, damit die ausmultiplizierte Fassung oben nicht still danebenliegt.
    #[test]
    fn entspricht_dem_produkt_der_einzeldrehungen() {
        let (x, y, z) = (23.0f32, -41.0, 67.0);
        let einzeln =
            Affine::rotation_xyz_deg(x, 0.0, 0.0).then(Affine::rotation_xyz_deg(0.0, y, 0.0)).then(Affine::rotation_xyz_deg(0.0, 0.0, z));
        let zusammen = Affine::rotation_xyz_deg(x, y, z);
        for probe in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [0.3, -0.7, 0.5]] {
            assert!(nah(einzeln.apply(probe), zusammen.apply(probe)), "Produkt weicht ab bei {probe:?}");
        }
    }
}

#[cfg(test)]
mod blocking_arm_pose_tests {
    use super::*;

    fn pose_mit_kopf(pitch: f32, yaw: f32) -> Pose {
        let mut pose = Pose::new();
        pose.rotate("head", [pitch, yaw, 0.0]);
        pose.rotate("right_arm", [0.0, 0.0, 0.0]);
        pose.rotate("left_arm", [0.0, 0.0, 0.0]);
        pose
    }

    #[test]
    // Der Arm geht deutlich HOCH (negative Neigung) und dreht zur Koerpermitte -- sonst
    // haenge der Schild weiter an der Seite, wie frueher.
    fn der_blockarm_geht_hoch_und_zur_mitte() {
        let mut pose = pose_mit_kopf(0.0, 0.0);
        apply_blocking_arm_pose(&mut pose, true);
        let rechts = pose.get("right_arm").rotation;
        assert!((rechts[0] + 0.9424779).abs() < 1e-5, "Neigung {}", rechts[0]);
        assert!((rechts[1] + 30.0 * std::f32::consts::PI / 180.0).abs() < 1e-5, "Gierung {}", rechts[1]);

        let mut pose = pose_mit_kopf(0.0, 0.0);
        apply_blocking_arm_pose(&mut pose, false);
        // Die linke Hand dreht spiegelbildlich, die Neigung bleibt dieselbe.
        assert!((pose.get("left_arm").rotation[1] - 30.0 * std::f32::consts::PI / 180.0).abs() < 1e-5);
    }

    #[test]
    // Der Schild folgt dem Blick -- aber nur bis zum Anschlag. Ohne die Deckel liefe der Arm
    // beim Blick senkrecht nach unten durch den Koerper.
    fn der_blockarm_folgt_dem_blick_bis_zum_anschlag() {
        let flach = {
            let mut p = pose_mit_kopf(0.0, 0.0);
            apply_blocking_arm_pose(&mut p, true);
            p.get("right_arm").rotation
        };
        let leicht_hoch = {
            let mut p = pose_mit_kopf(-0.3, 0.2);
            apply_blocking_arm_pose(&mut p, true);
            p.get("right_arm").rotation
        };
        assert!(leicht_hoch[0] < flach[0], "beim Blick nach oben muss der Arm mitgehen");
        assert!(leicht_hoch[1] > flach[1], "beim Blick nach rechts ebenso");

        // Weit ueber den Anschlag hinaus: dieselben Werte wie genau AM Anschlag.
        let am_anschlag = {
            let mut p = pose_mit_kopf(0.43633232, std::f32::consts::FRAC_PI_6);
            apply_blocking_arm_pose(&mut p, true);
            p.get("right_arm").rotation
        };
        let weit_darueber = {
            let mut p = pose_mit_kopf(1.5, 1.5);
            apply_blocking_arm_pose(&mut p, true);
            p.get("right_arm").rotation
        };
        assert!((am_anschlag[0] - weit_darueber[0]).abs() < 1e-6, "Neigung nicht gedeckelt");
        assert!((am_anschlag[1] - weit_darueber[1]).abs() < 1e-6, "Gierung nicht gedeckelt");
    }

    #[test]
    // BLOCK sticht ITEM aus: ein Schild in der Hand darf nicht BEIDE Haltungen bekommen.
    fn blocken_sticht_die_gewoehnliche_item_haltung_aus() {
        let mit_block = biped_pose_full(
            0.0,
            0.0,
            0.0,
            0.0,
            BipedExtras { item_left: true, block_left: true, ..Default::default() },
        );
        let nur_item = biped_pose_full(0.0, 0.0, 0.0, 0.0, BipedExtras { item_left: true, ..Default::default() });
        let hoch = mit_block.get("left_arm").rotation;
        let normal = nur_item.get("left_arm").rotation;
        assert!(hoch[0] < normal[0] - 0.5, "der blockende Arm muss deutlich hoeher sein: {hoch:?} gegen {normal:?}");
        assert!(hoch[1].abs() > 0.3, "und zur Mitte gedreht: {}", hoch[1]);
    }
}

#[cfg(test)]
mod item_arm_pose_tests {
    use super::*;

    // Der Arm wird ANGEHOBEN: die Neigung sinkt um PI/10 (im Modellraum zeigt X-Rotation
    // negativ nach vorn/oben). Aus dem Stand heraus (pitch 0) also genau -PI/10.
    #[test]
    fn item_hebt_den_arm() {
        let mut pose = biped_pose(0.0, 0.0, 0.0, 0.0, false, 0.0);
        let vorher = pose.get("right_arm").rotation[0];
        apply_item_arm_pose(&mut pose, true);
        let nachher = pose.get("right_arm").rotation[0];
        assert!(nachher < vorher, "der Arm muss sich heben, nicht senken");
        assert!((nachher - (vorher * 0.5 - std::f32::consts::PI / 10.0)).abs() < 1e-6);
    }

    // Die Laufanimation bleibt erhalten, nur halbiert -- genau das unterscheidet Vanillas
    // Formel von einem simplen "Winkel drauf". Bei vollem Ausschlag muss die Differenz
    // zwischen zwei Schwungphasen nach dem Anheben noch da, aber kleiner sein.
    #[test]
    fn schwung_bleibt_gedaempft_erhalten() {
        let messe = |swing: f32| {
            let mut pose = biped_pose(0.0, 0.0, swing, 1.0, false, 0.0);
            let roh = pose.get("right_arm").rotation[0];
            apply_item_arm_pose(&mut pose, true);
            (roh, pose.get("right_arm").rotation[0])
        };
        let (roh_a, mit_a) = messe(0.0);
        let (roh_b, mit_b) = messe(1.5);
        let roh_spanne = (roh_b - roh_a).abs();
        let mit_spanne = (mit_b - mit_a).abs();
        assert!(mit_spanne > 0.0, "der Arm darf beim Laufen nicht einfrieren");
        assert!(mit_spanne < roh_spanne, "der Ausschlag muss gedaempft sein");
        assert!((mit_spanne - roh_spanne * 0.5).abs() < 1e-5, "genau halbiert");
    }

    // Die linke Hand betrifft den linken Arm -- und NUR den.
    #[test]
    fn nebenhand_hebt_nur_den_linken_arm() {
        let mut pose = biped_pose(0.0, 0.0, 0.0, 0.0, false, 0.0);
        let rechts_vorher = pose.get("right_arm").rotation[0];
        apply_item_arm_pose(&mut pose, false);
        assert_eq!(pose.get("right_arm").rotation[0], rechts_vorher);
        assert!(pose.get("left_arm").rotation[0] < rechts_vorher);
    }
}

// Ruestung und Schild -- beides Modelle, die es vorher gar nicht gab.
#[cfg(test)]
mod ausruestungs_tests {
    use super::*;
    use crate::armor::ArmorSlot;

    fn teilnamen(model: &Model) -> Vec<&'static str> {
        model.parts.iter().map(|p| p.name).collect()
    }

    // ArmorFeatureRenderer.setVisible, Zeile fuer Zeile. Besonders die HOSE: sie zeichnet den
    // Rumpf MIT (das ist der Guertel), die Schuhe nicht.
    #[test]
    fn jedes_ruestungsteil_deckt_die_richtigen_koerperteile() {
        assert_eq!(teilnamen(&armor_model(ArmorSlot::Head)), ["head"]);
        assert_eq!(teilnamen(&armor_model(ArmorSlot::Chest)), ["body", "right_arm", "left_arm"]);
        assert_eq!(teilnamen(&armor_model(ArmorSlot::Legs)), ["body", "right_leg", "left_leg"]);
        assert_eq!(teilnamen(&armor_model(ArmorSlot::Feet)), ["right_leg", "left_leg"]);
        // Der Helm bringt die zweite Kopfebene ("hat") als Kind mit -- sie traegt in Vanilla
        // den ueberstehenden Teil des Helms.
        let head = &armor_model(ArmorSlot::Head).parts[0];
        assert_eq!(head.children.iter().map(|c| c.name).collect::<Vec<_>>(), ["hat"]);
    }

    // Die Hose ist die INNERE Ebene (Dilation 0.5), alles andere die aeussere (1.0). Ohne den
    // Unterschied steckten Brustpanzer und Hose an der Huefte ineinander.
    #[test]
    fn die_hose_ist_duenner_als_der_brustpanzer() {
        let dicke = |slot: ArmorSlot| armor_model(slot).parts.iter().find(|p| p.name == "body").unwrap().cuboids[0].dilation[0];
        assert_eq!(dicke(ArmorSlot::Legs), 0.5);
        assert_eq!(dicke(ArmorSlot::Chest), 1.0);
    }

    // Und die aeussere Ebene muss WIRKLICH aussen liegen: der Brustpanzer bei Dilation 1.0
    // steht ueberall einen Pixel weiter als der nackte Rumpf.
    #[test]
    fn die_ruestung_liegt_ausserhalb_des_koerpers() {
        let panzer = armor_model(ArmorSlot::Chest);
        let rumpf_panzer = panzer.parts.iter().find(|p| p.name == "body").unwrap();
        let nackt = player_model(false);
        let rumpf_nackt = nackt.parts.iter().find(|p| p.name == "body").unwrap();
        assert!(
            rumpf_panzer.cuboids[0].dilation > rumpf_nackt.cuboids[0].dilation,
            "der Brustpanzer muss weiter aussen liegen als der Rumpf"
        );
        // Gleiche Pivots -- sonst folgte die Ruestung der Pose nicht mehr.
        assert_eq!(rumpf_panzer.pivot, rumpf_nackt.pivot);
    }

    // ShieldEntityModel.getTexturedModelData: zwei Teile, feste Masse, Textur 64x64.
    #[test]
    fn das_schild_hat_platte_und_griff() {
        let model = shield_model();
        assert_eq!(model.tex_size, [64.0, 64.0]);
        assert_eq!(teilnamen(&model), ["plate", "handle"]);
        let plate = &model.parts[0].cuboids[0];
        assert_eq!(plate.uv, [0.0, 0.0]);
        assert_eq!(plate.offset, [-6.0, -11.0, -2.0]);
        assert_eq!(plate.size, [12.0, 22.0, 1.0]);
        let handle = &model.parts[1].cuboids[0];
        assert_eq!(handle.uv, [26.0, 0.0]);
        assert_eq!(handle.offset, [-1.0, -3.0, -1.0]);
        assert_eq!(handle.size, [2.0, 6.0, 6.0]);
    }

    // Das Schild steht UM seinen Ursprung, nicht in der 0..1-Zelle eines Item-Modells --
    // genau deshalb faellt bei ihm das `translate(-0.5)` weg (s. App::emit_shield). Waere es
    // in einer Zelle, laege der Schwerpunkt bei +0.5 statt bei 0.
    #[test]
    fn das_schild_steht_um_seinen_ursprung() {
        let mut out = Vec::new();
        emit(&shield_model(), &Pose::new(), Affine::IDENTITY, [0.0, 0.0, 1.0, 1.0], 1.0, &mut out);
        let (min_x, max_x) = out.iter().fold((f32::MAX, f32::MIN), |(a, b), v| (a.min(v.position[0]), b.max(v.position[0])));
        assert!(min_x < 0.0 && max_x > 0.0, "das Schild muss den Ursprung umschliessen, war {min_x}..{max_x}");
        // 12 Pixel breit, also -6/16 .. 6/16.
        assert!((min_x + 6.0 / 16.0).abs() < 1e-5, "linke Kante {min_x}");
        assert!((max_x - 6.0 / 16.0).abs() < 1e-5, "rechte Kante {max_x}");
    }

    // Die Faerbung darf NUR multiplizieren, nicht die Helligkeit ersetzen: Lederruestung
    // bekommt sie, alle anderen Materialien die neutrale Weissfaerbung, und beide muessen bei
    // gleicher Flaeche dasselbe Verhaeltnis behalten.
    #[test]
    fn die_faerbung_multipliziert_die_helligkeit() {
        let model = armor_model(ArmorSlot::Chest);
        let mut weiss = Vec::new();
        let mut braun = Vec::new();
        emit_tinted(&model, &Pose::new(), Affine::IDENTITY, [0.0, 0.0, 1.0, 1.0], [1.0, 1.0, 1.0], 1.0, &mut weiss);
        emit_tinted(&model, &Pose::new(), Affine::IDENTITY, [0.0, 0.0, 1.0, 1.0], crate::armor::UNDYED_LEATHER, 1.0, &mut braun);
        assert_eq!(weiss.len(), braun.len());
        for (w, b) in weiss.iter().zip(&braun) {
            for k in 0..3 {
                assert!((b.color[k] - w.color[k] * crate::armor::UNDYED_LEATHER[k]).abs() < 1e-5);
            }
        }
    }
}

#[cfg(test)]
mod end_crystal_tests {
    use super::*;

    #[test]
    // getYOffset pendelt zwischen -1.4 und -0.6 Bloecken.
    fn der_kristall_schwebt_im_vanilla_bereich() {
        for i in 0..400 {
            let y = end_crystal_y_offset(i as f32 * 0.37);
            assert!((-1.4 - 1e-4..=-0.6 + 1e-4).contains(&y), "{y}");
        }
    }

    #[test]
    // Bodenplatte: 12x4x12 Pixel ab y=0, mal 2 und um -0.5 verschoben -> 1.5 Bloecke breit,
    // von einem Block unter bis einen halben Block unter der Entity-Position.
    fn bodenplatte_sitzt_wie_in_vanilla() {
        let mut out = Vec::new();
        emit(&end_crystal_model(true), &Pose::new(), end_crystal_transform([0.0; 3]), [0.0, 0.0, 1.0, 1.0], 1.0, &mut out);
        let ohne = {
            let mut o = Vec::new();
            emit(&end_crystal_model(false), &Pose::new(), end_crystal_transform([0.0; 3]), [0.0, 0.0, 1.0, 1.0], 1.0, &mut o);
            o.len()
        };
        let platte = &out[ohne..];
        let (mut min, mut max) = ([f32::MAX; 3], [f32::MIN; 3]);
        for v in platte {
            for i in 0..3 {
                min[i] = min[i].min(v.position[i]);
                max[i] = max[i].max(v.position[i]);
            }
        }
        assert!((min[1] + 1.0).abs() < 1e-4 && (max[1] + 0.5).abs() < 1e-4, "{min:?} {max:?}");
        assert!((max[0] - min[0] - 1.5).abs() < 1e-4);
    }
}

#[cfg(test)]
mod pfeil_tests {
    use super::*;
    use std::f32::consts::PI;

    // Die Zahlen stehen so in ArrowModel.createBodyLayer. Sie von Hand nachzuziehen ist die
    // haeufigste Fehlerquelle bei einem nachgebauten Modell, deshalb hier festgenagelt --
    // inklusive der beiden Dinge, die man leicht uebersieht: die Lagen-Skalierung 0.9 geht
    // MULTIPLIKATIV auf die 0.8 des Federteils, und das Kreuz traegt eine Textur-Streckung.
    #[test]
    fn pfeilmodell_trifft_vanillas_masse() {
        let m = arrow_model(ARROW_TEXTURE);
        assert_eq!(m.tex_size, [32.0, 32.0]);
        let namen: Vec<&str> = m.parts.iter().map(|p| p.name).collect();
        assert_eq!(namen, vec!["back", "cross_1", "cross_2"]);

        let back = &m.parts[0];
        assert_eq!(back.pivot, [-11.0, 0.0, 0.0]);
        assert!((back.rotation[0] - PI / 4.0).abs() < 1e-6);
        assert!((back.scale[0] - 0.72).abs() < 1e-6, "0.8 aus withScale mal 0.9 der Lage, war {}", back.scale[0]);
        // Flach auf X: eine Ebene, kein Kasten.
        assert_eq!(back.cuboids[0].size, [0.0, 5.0, 5.0]);
        assert_eq!(back.cuboids[0].offset, [0.0, -2.5, -2.5]);

        for teil in &m.parts[1..] {
            assert_eq!(teil.pivot, [0.0; 3]);
            assert!((teil.scale[0] - 0.9).abs() < 1e-6);
            // Flach auf Z, 16 lang und 4 hoch -- der Schaft.
            assert_eq!(teil.cuboids[0].size, [16.0, 4.0, 0.0]);
            assert_eq!(teil.cuboids[0].offset, [-12.0, -2.0, 0.0]);
            assert_eq!(teil.cuboids[0].tex_scale, [1.0, 0.8], "texScale des Schafts");
        }
        // Die beiden Ebenen stehen um 90 Grad gegeneinander -- daraus wird das Kreuz.
        let d = m.parts[2].rotation[0] - m.parts[1].rotation[0];
        assert!((d - PI / 2.0).abs() < 1e-6, "Kreuzwinkel {d}");
    }

    // ArrowRenderer dreht nur -- keine Spiegelung, keine -1.501-Verschiebung. Die Probe:
    // bei Yaw 0 und Pitch 0 muss die Spitze (Modell-+X) nach +Z zeigen, Minecrafts Yaw-0-
    // Richtung. Ein vergessenes -90 faellt hier sofort auf.
    #[test]
    fn pfeil_zeigt_bei_yaw_null_nach_plus_z() {
        let t = arrow_transform([0.0, 64.0, 0.0], 0.0, 0.0);
        let spitze = t.apply_direction([1.0, 0.0, 0.0]);
        assert!(spitze[2] > 0.99, "Spitze zeigt nach {spitze:?}, erwartet +Z");
        assert!(spitze[0].abs() < 1e-6 && spitze[1].abs() < 1e-6, "{spitze:?}");
        // Und der Ursprung sitzt auf der Entity-Position, ohne Versatz.
        assert_eq!(t.apply([0.0, 0.0, 0.0]), [0.0, 64.0, 0.0]);
    }
}
