// Entity-Ereignisse, Server-Rotation, Paketluecken, Lagback und Rubberband.

use super::*;

impl App {
    pub(crate) fn handle_entity_event(&mut self, entity_id: i32, status: i8) {
        if status != protocol::packets::ENTITY_STATUS_USE_TOTEM || self.own_entity_id != Some(entity_id) {
            return;
        }
        // Ohne Totem-Modul: Vanillas eigene Pop-Animation, das grosse Totem vor dem Bildschirm
        // (ClientPlayNetworkHandler.onEntityStatus -> showFloatingItem). Fehlte frueher
        // ganz. Mit Modul ersetzt dessen Anzeige sie (sonst
        // liefen zwei Animationen uebereinander).
        if !self.settings.module_totem_indicator {
            // Pop-Optik: das Flug-Totem abschaltbar (showFloatingItem in der Mod).
            if self.settings.toteming.visuals && self.settings.toteming.hide_own_floating {
                return;
            }
            let zufall = |salz: u64| {
                let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
                let h = (t ^ salz).wrapping_mul(0x9E37_79B9_7F4A_7C15);
                (h >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0
            };
            self.floating_totem = Some((modules::TOTEM_ANIMATION_TICKS, zufall(0x51), zufall(0xA7)));
            return;
        }
        // DEN TON HAT DER NETZWERK-TASK SCHON GESPIELT (s. start_connect, Sofort-Haken) --
        // hier laeuft nur noch, was ohnehin auf ein Frame warten muss: Anzeige und Log.
        // Der Ton darf NICHT auch hier stehen, sonst klingt er zweimal.
        //
        // Und die Hand wird hier nicht neu entschieden, sondern aus derselben
        // veroeffentlichten Quelle gelesen wie der Ton. Zweimal rechnen hiesse: Bild und Ton
        // koennten verschiedene Haende meinen (Gold sehen, Zweithand-Ton hoeren).
        let hand = modules::TotemHand::from_published(self.totem_hand_published.load(std::sync::atomic::Ordering::Relaxed));
        self.totem_animation = Some(modules::TotemPopAnimation::start(hand));
        self.push_log(format!("Totem gepoppt: {}", hand.label()));
    }

    // Blickneigung in VANILLA-Konvention (positiv = nach unten). Unsere Kamera zaehlt
    // umgekehrt (positiv = nach oben, s. camera.rs), deshalb das gedrehte Vorzeichen.
    //
    // Eine Quelle fuer alles, was diesen Wert braucht: das Bewegungspaket UND das eigene
    // Spielermodell. ECHTER BUG gewesen (oben und unten waren fuer die eigene Figur
    // vertauscht): das Paket drehte das Vorzeichen, das Modell nicht -- in der
    // 3.-Person-Ansicht senkte die Figur also den Kopf, wenn man nach oben schaute.
    pub(crate) fn look_pitch_deg(&self) -> f32 {
        -self.camera.pitch
    }

    // Vom Server gesetzte Blickrichtung (Protokoll-Konvention: Pitch nach unten positiv).
    // Sonstiges > No rotate set: bei Ignore/Reset bleibt die Kamera. Die Bestaetigung an den
    // Server traegt seine Richtung trotzdem (s. connection.rs) -- bei Reset tut der naechste
    // Tick so, als sei zuletzt diese gesendet, und schickt damit die eigene hinterher.
    pub(crate) fn apply_server_rotation(&mut self, yaw_deg: f32, pitch_deg: f32) {
        match self.settings.no_rotate_set {
            SERVER_SET_FOLLOW => {
                self.camera.yaw = yaw_deg;
                self.camera.pitch = (-pitch_deg).clamp(-MAX_PITCH, MAX_PITCH);
            }
            SERVER_SET_RESET => {
                self.last_sent_yaw_deg = yaw_deg;
                self.last_sent_pitch_deg = pitch_deg;
            }
            _ => {}
        }
    }

    // Ein Ereignis vom Server ist da: eine stehende "Keine Pakete"-Karte weicht der Meldung,
    // wie lange es still war.
    pub(crate) fn server_event_seen(&mut self) {
        let jetzt = Instant::now();
        let stille = jetzt.duration_since(self.last_server_event);
        self.last_server_event = jetzt;
        if let Some(id) = self.packet_gap_note.take() {
            self.notifications.dismiss(id, jetzt);
            self.notify(notifications::Notification::info("Server wieder da", format!("{:.1} s lang kamen keine Pakete.", stille.as_secs_f32())));
        }
    }

    // Seit PACKET_GAP_WARN nichts vom Server: einmal warnen, bis wieder etwas kommt. Nur im
    // Spiel -- beim Verbinden ist Warten normal.
    pub(crate) fn check_packet_gap(&mut self) {
        if self.conn_status != ConnStatus::Connected || self.chunks_received == 0 || self.packet_gap_note.is_some() {
            return;
        }
        if self.last_server_event.elapsed() >= PACKET_GAP_WARN {
            let karte = notifications::Notification::warning("Keine Pakete vom Server", format!("Seit {} s kam nichts mehr an.", PACKET_GAP_WARN.as_secs()))
                .with_lifetime(PACKET_GAP_NOTE_LIFETIME);
            self.packet_gap_note = Some(self.notify(karte));
        }
    }

    // Der Server hat uns versetzt, `strecke` Bloecke weit. Nah dran heisst Lag-back (Flag,
    // Korrektur) -- weiter weg ist ein echter Teleport (/tp, Warp) und keine Meldung wert.
    // Folgen weitere kurz danach, zaehlt die stehende Karte mit, statt zu stapeln.
    pub(crate) fn note_lagback(&mut self, strecke: f64) {
        if !self.settings.module_connection_debug || self.replaying_backlog || !(LAGBACK_MIN_BLOCKS..=LAGBACK_MAX_BLOCKS).contains(&strecke) {
            return;
        }
        let jetzt = Instant::now();
        if let Some((id, anzahl, zuletzt)) = self.lagback_note {
            let anzahl = anzahl + 1;
            if jetzt.duration_since(zuletzt) < LAGBACK_GROUP
                && self.notifications.renew(id, format!("Lag-back ×{anzahl}"), format!("Zuletzt {strecke:.1} Blöcke zurückgesetzt."))
            {
                self.lagback_note = Some((id, anzahl, jetzt));
                return;
            }
        }
        let id = self.notify(notifications::Notification::warning("Lag-back", format!("{strecke:.1} Blöcke zurückgesetzt.")));
        self.lagback_note = Some((id, 1, jetzt));
    }

    // Stockung im Paketfluss (s. net::ClientEvent::PacketStall). Solange die Karte steht,
    // zaehlt sie weitere mit.
    pub(crate) fn note_rubberband(&mut self, ausgehend: bool, millis: u32) {
        if !self.settings.module_connection_debug || self.replaying_backlog {
            return;
        }
        let text = if ausgehend {
            format!("O: {millis} ms lang ging nichts an den Server.")
        } else {
            format!("I: {millis} ms lang kam nichts, dann alles auf einmal.")
        };
        if let Some((id, anzahl)) = self.rubberband_note {
            let anzahl = anzahl + 1;
            if self.notifications.renew(id, format!("Starkes Rubberbanding ×{anzahl}"), text.clone()) {
                self.rubberband_note = Some((id, anzahl));
                return;
            }
        }
        let id = self.notify(notifications::Notification::warning("Starkes Rubberbanding", text));
        self.rubberband_note = Some((id, 1));
    }
}
