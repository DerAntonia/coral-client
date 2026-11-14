// Schild-Markierungen, Namensfarben, Teams und Hitbox-Vertices.

use super::*;

impl App {
    // Hitbox-Anzeige (Modul, s. Settings::module_hitbox_*): Drahtgitter-Box um jedes
    // angreifbare Fremd-Entity, wahlweise an seiner GEGLAETTETEN Bildposition (render_pos,
    // dieselbe, die build_entity_vertices auch fuers Modell benutzt) und/oder an seiner
    // ROHEN, zuletzt vom Server geschickten Position (target_pos) -- Letztere zeigt, wie weit
    // die Bild-Glaettung dem Server-Stand gerade hinterherhaengt, Erstere ist die Position,
    // die man tatsaechlich vor sich sieht.
    //
    // Welche Entities eine Box bekommen und in welcher Farbe, regeln die eigenen Arten (s.
    // hitbox_kinds); was keine eigene Art hat, nur wenn es angreifbar ist (Mobs, s.
    // world::is_attackable). Dazu Spinnennetze und Wasserquellen als Bloecke.
    //
    // Hier entstehen nur die KANTEN als Endpunkt-Paare; ihre Breite bekommt die Anzeige erst
    // im Vertex-Shader nach der Projektion (s. push_hitbox_edge und block.wgsl::vs_hitbox).
    // Die eingestellte Pixelbreite und die Deckkraft reisen deshalb nur als Werte je Vertex
    // mit (s. render::HitboxVertex) -- der Blockumriss im selben Puffer hat eigene.
    //
    // Die gestrichelte Darstellung teilt die Kanten dagegen SCHON HIER auf: dafuer braucht es
    // eine Weltlaenge, und die haengt (anders als die Breite) sinnvollerweise an der
    // Entfernung des jeweiligen Segments -- ein Strichmuster, das im Bildschirmraum konstant
    // waere, saehe an einer schraeg wegfuehrenden Kante falsch aus.
    // Schild-Modul: welche Markierung (Cooldown vor Hochhalten -- wer im Cooldown steckt, kann
    // ohnehin nicht blocken) dieses Entity in der Darstellung `art` bekommt, mit ihrer Farbe.
    pub(crate) fn shield_mark_color(&self, id: i32, e: &world::Entity, art: ShieldMark) -> Option<[f32; 4]> {
        let set = &self.settings;
        if !set.module_shields || !world::is_player(e.entity_type) {
            return None;
        }
        if set.shield_cd_players && set.shield_mark(ShieldColor::Cooldown) == art && self.shield_cooldowns.contains_key(&id) {
            return Some(set.shield_color_rgba(ShieldColor::Cooldown));
        }
        if set.shield_hold_players && set.shield_mark(ShieldColor::Hold) == art && !e.invisible {
            let (rechts, links) = self.entity_blocking_arms(e);
            if rechts || links {
                return Some(set.shield_color_rgba(ShieldColor::Hold));
            }
        }
        None
    }

    // Farbe des Namensschilds eines Spielers (0xRRGGBB) und woher sie stammt. Der Reihe nach:
    //   1. die Farbe seines Scoreboard-Teams (so faerbt Vanilla den Namen),
    //   2. der Name in der Tab-Liste (Anzeigename aus dem Spielerlisten-Paket): die Farbe
    //      des Abschnitts, der den Spielernamen enthaelt,
    //   3. die letzte Farbe im Team-Prefix (Server, die den Namen nur ueber den Prefix
    //      faerben und die Teamfarbe leer lassen),
    //   4. ein eigenes Namensschild des Servers: ein Text-Display als Passagier, dort der
    //      Abschnitt mit dem Namen, sonst der erste farbige.
    // None = nichts davon.
    pub(crate) fn name_tag_color(&self, uuid: &[u8; 16], entity_id: Option<i32>) -> (Option<u32>, &'static str) {
        let name = self.player_names.get(uuid);
        let team = name.and_then(|n| self.sb_entry_team.get(n)).and_then(|t| self.sb_teams.get(t));
        if let Some(c) = team.and_then(|t| protocol::nbt::formatting_color(t.color)) {
            return (Some(c), "Teamfarbe");
        }
        let tab = self.tab_entries.get(uuid).and_then(|t| t.display_name.as_ref());
        if let Some(c) = tab.zip(name).and_then(|(spans, n)| spans.iter().find(|s| s.text.contains(n.as_str())).and_then(|s| s.color)) {
            return (Some(c), "Tab-Liste");
        }
        if let Some(c) = team.and_then(|t| protocol::nbt::legacy_to_spans(&t.prefix, None).iter().rev().find_map(|s| s.color)) {
            return (Some(c), "Prefix");
        }
        // Was tatsaechlich ueber dem Kopf steht (s. head_labels): der Abschnitt mit dem Namen
        // zaehlt, sonst der erste farbige.
        let Some(name) = name else { return (None, "keine") };
        let mut ersatz = None;
        for spans in self.head_labels(uuid, entity_id) {
            if let Some(c) = spans.iter().find(|s| s.text.contains(name.as_str())).and_then(|s| s.color) {
                return (Some(c), "Beschriftung");
            }
            ersatz = ersatz.or_else(|| spans.iter().find_map(|s| s.color.filter(|c| *c != 0xFFFFFF)));
        }
        if let Some(c) = ersatz {
            return (Some(c), "Beschriftung (Rest)");
        }
        // Die Seitenleiste: Team-Modi listen dort oft die Spieler in ihrer Teamfarbe -- fuer den
        // EIGENEN Namen meist die einzige Quelle, denn ueber dem eigenen Kopf zeigt der Server
        // kein Namensschild.
        if let Some((_, zeilen)) = self.scoreboard_lines() {
            for zeile in zeilen {
                let text: String = zeile.iter().map(|s| s.text.as_str()).collect();
                if !text.contains(name.as_str()) {
                    continue;
                }
                let farbe = zeile
                    .iter()
                    .find(|s| s.text.contains(name.as_str()))
                    .and_then(|s| s.color)
                    .or_else(|| zeile.iter().find_map(|s| s.color.filter(|c| *c != 0xFFFFFF)));
                if let Some(c) = farbe {
                    return (Some(c), "Seitenleiste");
                }
            }
        }
        (None, "keine")
    }

    // Jede Beschriftung ueber dem Kopf eines Spielers, die seinen Namen enthaelt: Spielername,
    // Text-Display, Custom Name eines Ruestungsstaenders ... -- egal woher, was zaehlt, ist was
    // dort steht. Health-Anzeigen ("19 ❤") enthalten den Namen nicht und fallen heraus.
    pub(crate) fn head_labels(&self, uuid: &[u8; 16], entity_id: Option<i32>) -> Vec<Vec<protocol::nbt::TextSpan>> {
        let Some(name) = self.player_names.get(uuid) else { return Vec::new() };
        // Die eigene Figur steht nicht in der Entity-Liste: ihr Fusspunkt kommt aus der Physik.
        let fuss = if Some(*uuid) == self.own_uuid {
            Some((self.physics.pos.x, self.physics.pos.y, self.physics.pos.z))
        } else {
            entity_id.and_then(|id| self.entities.get(id)).map(|e| e.pos)
        };
        let Some(fuss) = fuss else { return Vec::new() };
        let mut out = Vec::new();
        for (lid, le) in self.entities.iter() {
            let Some(label) = self.world_label(*lid, le, 0.0) else { continue };
            let a = label.anchor;
            if (a.x - fuss.0).abs() > 0.8 || (a.z - fuss.2).abs() > 0.8 || a.y < fuss.1 || a.y > fuss.1 + 4.5 {
                continue;
            }
            if label.spans.iter().map(|s| s.text.as_str()).collect::<String>().contains(name.as_str()) {
                out.push(label.spans.into_owned());
            }
        }
        out
    }

    // Alles, woran das Teams-Modul das Team eines Spielers erkennt (s. teams::side_of).
    pub(crate) fn team_facts(&self, uuid: &[u8; 16], entity_id: Option<i32>) -> (teams::Facts, &'static str) {
        let name = self.player_names.get(uuid);
        let team = name.and_then(|n| self.sb_entry_team.get(n)).cloned();
        let (color, quelle) = self.name_tag_color(uuid, entity_id);
        // Der angezeigte Name als Klartext: Team-Prefix + Name, sonst die Tab-Liste, sonst
        // die Beschriftung ueber dem Kopf.
        let klartext = |spans: &[protocol::nbt::TextSpan]| spans.iter().map(|s| s.text.as_str()).collect::<String>();
        let text = team
            .as_ref()
            .and_then(|t| self.sb_teams.get(t))
            .map(|t| klartext(&protocol::nbt::legacy_to_spans(&format!("{}{}", t.prefix, name.map_or("", |n| n.as_str())), None)))
            .or_else(|| self.tab_entries.get(uuid).and_then(|t| t.display_name.as_deref()).map(klartext))
            .or_else(|| self.head_labels(uuid, entity_id).first().map(|l| klartext(l)));
        let prefix = text.zip(name).and_then(|(t, n)| teams::prefix_before(&t, n));
        let ruestung = if Some(*uuid) == self.own_uuid {
            self.own_armor()
        } else {
            entity_id.and_then(|id| self.entities.get(id)).map_or([None; 4], |e| e.armor)
        };
        let armor = ruestung.map(|r| r.and_then(|r| r.dyed));
        (teams::Facts { team, color, prefix, armor }, quelle)
    }

    // Die eigene Figur als moegliches Fahrzeug an die Entity-Liste melden (s.
    // Entities::set_own_body): Namensschild-Displays auf dem eigenen Kopf laufen sonst nicht mit.
    pub(crate) fn sync_own_body(&mut self) {
        let Some(id) = self.own_entity_id else { return };
        let p = self.physics.pos;
        self.entities.set_own_body(id, (p.x, p.y, p.z), self.camera.yaw, self.physics.current_height());
    }

    // Vorschlaege fuer die eigene Teamfarbe: die Namensfarben der anderen Spieler, haeufigste
    // zuerst, ohne Weiss. Eine schon gewaehlte bleibt dabei, auch wenn sie gerade niemand traegt.
    pub(crate) fn team_color_suggestions(&self) -> Vec<u32> {
        let mut zaehler: Vec<(u32, usize)> = Vec::new();
        for (uuid, f) in &self.team_facts {
            let Some(c) = f.color.filter(|c| *c != 0xFFFFFF) else { continue };
            if Some(*uuid) == self.own_uuid {
                continue;
            }
            match zaehler.iter_mut().find(|(z, _)| *z == c) {
                Some(z) => z.1 += 1,
                None => zaehler.push((c, 1)),
            }
        }
        zaehler.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let mut out: Vec<u32> = zaehler.into_iter().map(|(c, _)| c).collect();
        if let Some(c) = self.settings.team_own_color.filter(|c| !out.contains(c)) {
            out.insert(0, c);
        }
        out.truncate(COLOR_PICK_MAX);
        out
    }

    // Teams-Modul: die Merkmale aller Spieler einmal pro Tick (das Zeichnen liest nur
    // team_facts), und jede Aenderung ins Log -- woran die Erkennung haengt, laesst sich nur
    // am echten Server pruefen.
    pub(crate) fn update_team_facts(&mut self) {
        self.team_facts.clear();
        if !self.settings.module_teams {
            return;
        }
        let eigene = match self.own_uuid {
            Some(u) => vec![(u, self.own_entity_id)],
            None => Vec::new(),
        };
        let spieler: Vec<([u8; 16], Option<i32>)> = eigene
            .into_iter()
            .chain(self.entities.iter().filter(|(_, e)| world::is_player(e.entity_type)).map(|(id, e)| (e.uuid, Some(*id))))
            .collect();
        for (uuid, id) in spieler {
            let (mut fakten, mut quelle) = self.team_facts(&uuid, id);
            if let (true, Some(c)) = (Some(uuid) == self.own_uuid, self.settings.team_own_color) {
                fakten.color = Some(c);
                quelle = "eingestellt";
            }
            match fakten.color {
                Some(c) => {
                    self.team_color_memory.insert(uuid, c);
                }
                None => {
                    if let Some(&c) = self.team_color_memory.get(&uuid) {
                        fakten.color = Some(c);
                        quelle = "gemerkt";
                    }
                }
            }
            let name = self.player_names.get(&uuid).cloned().unwrap_or_else(|| "?".to_string());
            let eigen = if Some(uuid) == self.own_uuid { " (du)" } else { "" };
            let hex = |c: Option<u32>| c.map_or("-".to_string(), |c| format!("#{c:06X}"));
            let zeile = format!(
                "[teams] {name}{eigen}: Name {} ({quelle}) | Team {} | Prefix {} | Ruestung {}",
                hex(fakten.color),
                fakten.team.as_deref().unwrap_or("-"),
                fakten.prefix.as_deref().unwrap_or("-"),
                fakten.armor.map(hex).join(" ")
            );
            if self.team_color_log.get(&uuid) != Some(&zeile) {
                eprintln!("{zeile}");
                self.team_color_log.insert(uuid, zeile);
            }
            self.team_facts.insert(uuid, fakten);
        }
    }

    // Wie das Teams-Modul die Box dieses Spielers zeichnet: Farbe, Deckkraft (0..1), Dichte
    // in Pixeln und gepunktet -- oder None, wenn seine Seite nicht ueberschrieben wird.
    pub(crate) fn team_hitbox_style(&self, e: &world::Entity) -> Option<([f32; 3], f32, u32, bool)> {
        if !world::is_player(e.entity_type) || Some(e.uuid) == self.own_uuid {
            return None;
        }
        let leer = teams::Facts::default();
        let eigene = self.own_uuid.and_then(|u| self.team_facts.get(&u)).unwrap_or(&leer);
        let fremde = self.team_facts.get(&e.uuid).unwrap_or(&leer);
        self.team_style_of(teams::side_of(&self.settings.team_match, eigene, fremde), fremde.display_color())
    }

    // Stil einer Seite fuer einen Spieler mit der Namensfarbe `name_color` (ohne Farbe weiss,
    // wie ein ungefaerbter Name). None = Modul aus oder die Seite wird nicht ueberschrieben.
    pub(crate) fn team_style_of(&self, seite: teams::Side, name_color: Option<u32>) -> Option<([f32; 3], f32, u32, bool)> {
        let set = &self.settings;
        let st = set.team_hitboxes[seite.index()];
        if !set.module_teams || !st.on {
            return None;
        }
        let farbe = if st.nametag {
            let c = name_color.unwrap_or(0xFFFFFF);
            [((c >> 16) & 0xFF) as f32 / 255.0, ((c >> 8) & 0xFF) as f32 / 255.0, (c & 0xFF) as f32 / 255.0]
        } else {
            hsv_to_rgb(st.hue as f32, st.saturation as f32 / 100.0, st.value as f32 / 100.0)
        };
        Some((farbe, st.opacity as f32 / 100.0, st.density_px, st.dotted))
    }

    pub(crate) fn build_hitbox_vertices(&self, screen_height: f32) -> Vec<render::HitboxVertex> {
        use hitbox_kinds::HitboxKind;
        let set = &self.settings;
        let stil = |art: HitboxKind| set.module_hitbox_styles[art.index()];
        let farbe = |st: hitbox_kinds::HitboxStyle| hsv_to_rgb(st.hue as f32, st.saturation as f32 / 100.0, st.value as f32 / 100.0);
        let modul_an = set.module_hitbox_enabled;
        // Die Bloecke haben keine geglaettete oder rohe Lage, sie haengen nur am Modul.
        let netz = set.module_hitbox_enabled && stil(HitboxKind::Cobweb).on;
        let wasser = set.module_hitbox_enabled && stil(HitboxKind::Water).on;
        // Ohne Hitbox-Modul zeichnet nur der Block-Teil (Netz, Wasser) -- Schild- und Teams-
        // Markierungen faerben nur Boxen, die das Modul ohnehin zeigt.
        let spieler_roh = set.module_hitbox_player_raw;
        if !(modul_an || netz || wasser) {
            return Vec::new();
        }
        let eye = self.camera.position;
        // Wie OUTLINE_PULL: Boxen ein Stueck zum Auge gezogen, damit die Unterkante auf dem
        // Boden nicht im Tiefentest gegen die Blockflaeche verliert.
        let nah = |p: DVec3| -> Vec3 { ((p - eye) * (1.0 - OUTLINE_PULL)).as_vec3() };
        // Hauptfarbe: die Farbe jeder Art ohne "Farbe ueberschreiben".
        let smoothed_color =
            hsv_to_rgb(set.module_hitbox_smoothed_hue as f32, set.module_hitbox_smoothed_saturation as f32 / 100.0, set.module_hitbox_smoothed_value as f32 / 100.0);
        let raw_color = hsv_to_rgb(set.module_hitbox_raw_hue as f32, set.module_hitbox_raw_saturation as f32 / 100.0, set.module_hitbox_raw_value as f32 / 100.0);
        // Halbe Bildhoehe in Sichtraum-Einheiten bei d=1 -- dieselbe Groesse, die die
        // Projektion selbst benutzt (s. block_renderer::CameraUniform.params.y).
        let half_fov_tan = (self.camera.fov_degrees.to_radians() * 0.5).tan();
        let progress = self.tick_progress();
        // Nur Schild-Markierungen: voll deckend, sie sollen auffallen.
        let alpha = if set.module_hitbox_enabled { set.module_hitbox_opacity as f32 / 100.0 } else { 1.0 };
        // Farbe und Deckkraft einer Art: die eigene mit Ueberschreiben, sonst Hauptfarbe.
        let art_farbe = |st: hitbox_kinds::HitboxStyle| if st.custom { (farbe(st), alpha * st.alpha as f32 / 100.0) } else { (smoothed_color, alpha) };
        let mut out = Vec::new();
        let linien = if set.module_hitbox_enabled { set.module_hitbox_lines } else { 0 };
        // Eine Box aus ihren Ecken (augenrelativ) als zwoelf Kanten, mit Dichte in Pixeln und
        // ob gepunktet (das Teams-Modul hat dafuer eigene Werte).
        let mut push_box = |min: Vec3, max: Vec3, color: [f32; 4], density_px: u32, dotted: bool| {
            let half_width = density_px as f32 / 2.0;
            let corner = |sel: [bool; 3]| Vec3::new(if sel[0] { max.x } else { min.x }, if sel[1] { max.y } else { min.y }, if sel[2] { max.z } else { min.z });
            const EDGES: [([bool; 3], [bool; 3]); 12] = [
                ([false, false, false], [true, false, false]),
                ([false, true, false], [true, true, false]),
                ([false, false, true], [true, false, true]),
                ([false, true, true], [true, true, true]),
                ([false, false, false], [false, true, false]),
                ([true, false, false], [true, true, false]),
                ([false, false, true], [false, true, true]),
                ([true, false, true], [true, true, true]),
                ([false, false, false], [false, false, true]),
                ([true, false, false], [true, false, true]),
                ([false, true, false], [false, true, true]),
                ([true, true, false], [true, true, true]),
            ];
            // Nur sichtbare Kanten: eine Kante verdeckt die Box selbst, wenn beide Flaechen,
            // an denen sie liegt, vom Auge weg zeigen. Das Auge ist der Ursprung, eine Flaeche
            // zeigt zu ihm, wenn es auf ihrer Aussenseite liegt. Steckt das Auge in der Box,
            // zeigt keine Flaeche zu ihm -- dann alle Kanten, man sieht sie von innen.
            // Nur Umriss: genau eine der beiden Flaechen zeigt zum Auge -- die Kante trennt
            // Vorder- von Rueckseite, liegt also auf dem Rand der Box im Bild.
            let zum_auge = |achse: usize, oben: bool| if oben { max[achse] < 0.0 } else { min[achse] > 0.0 };
            let innen = (0..3).all(|i| !zum_auge(i, false) && !zum_auge(i, true));
            for (a_sel, b_sel) in EDGES {
                if linien != 0 && !innen {
                    // Die zwei Achsen, auf denen die Kante fest an einer Seite liegt.
                    let vorne = (0..3).filter(|&i| a_sel[i] == b_sel[i]).filter(|&i| zum_auge(i, a_sel[i])).count();
                    let zeigen = if linien == HITBOX_LINES_OUTLINE { vorne == 1 } else { vorne >= 1 };
                    if !zeigen {
                        continue;
                    }
                }
                let a = corner(a_sel);
                let b = corner(b_sel);
                // Strich+Luecke-Laenge dieser KANTE, aus ihrer EIGENEN Entfernung (nicht
                // der der ganzen Box): eine nahe Kante braucht mehr Weltlaenge fuer
                // dasselbe Bildschirmmuster als eine ferne.
                let dash_len = dotted.then(|| {
                    let dist = ((a + b) * 0.5).length().max(0.05);
                    let px_per_world = ((screen_height * 0.5) / (dist * half_fov_tan)).max(1e-6);
                    // Das Strichmuster ist bewusst laenger als die Linie breit ist:
                    // Striche in Breite der Linie waeren Punkte, keine Striche.
                    density_px as f32 * HITBOX_DASH_PER_DENSITY / px_per_world
                });
                match dash_len {
                    Some(len) if len > 0.001 => {
                        let total = (b - a).length();
                        let dir = (b - a) / total.max(1e-6);
                        let mut s = 0.0;
                        let mut on = true;
                        while s < total {
                            let end = (s + len).min(total);
                            if on {
                                push_hitbox_edge(&mut out, a + dir * s, a + dir * end, color, half_width);
                            }
                            s = end;
                            on = !on;
                        }
                    }
                    _ => push_hitbox_edge(&mut out, a, b, color, half_width),
                }
            }
        };
        let (dichte, gepunktet) = (set.module_hitbox_density_px, set.module_hitbox_dotted);
        for (id, e) in self.entities.iter() {
            if !self.entity_in_render_distance(e) {
                continue;
            }
            // Display-Entities sind nicht anvisierbar (s. Entities::pick) und haetten hier
            // eine Box, auf die man gar nicht zielen kann.
            if e.display.is_some() {
                continue;
            }
            // Schild-Modul: Cooldown bzw. gehobenes Schild faerbt die Box (Anzeige "Hitbox").
            // Sonst die eigene Art mit ihrer Farbe und Deckkraft, und alles andere
            // Angreifbare (Boote, Loren, Ruestungsstaender) in der Farbe von "Geglättet".
            let art = HitboxKind::of_entity(e.entity_type);
            // Teams-Modul: eigene Farbe, Deckkraft, Dichte und Strichelung fuer Verbuendete
            // bzw. Gegner. Es zeichnet selbst keine Box, sondern faerbt nur eine, die schon da
            // ist (s. unten) -- Dichte und Strichelung gelten dann fuer diese Box.
            let team_stil = self.team_hitbox_style(e);
            let (dichte, gepunktet) = team_stil.map_or((dichte, gepunktet), |(_, _, d, p)| (d, p));
            let schild_farbe = self.shield_mark_color(*id, e, ShieldMark::Hitbox);
            let art = match art {
                Some(art) => art,
                None if world::is_attackable(e.entity_type) => HitboxKind::Other,
                None => continue,
            };
            if !modul_an || !stil(art).on {
                continue;
            }
            let (smoothed_color, a) = art_farbe(stil(art));
            // Schild-Markierung und Teams-Modul faerben nur die Box, die schon gezeichnet wird.
            // Die Schild-Farbe hat Vorrang vor der Teams-Farbe.
            let (smoothed_color, raw_color, a) = match (schild_farbe, team_stil) {
                (Some([r, g, b, _]), _) => ([r, g, b], [r, g, b], alpha),
                (None, Some((farbe, deckkraft, _, _))) => (farbe, farbe, deckkraft),
                _ => (smoothed_color, raw_color, a),
            };
            let roh = art == HitboxKind::Player && spieler_roh;
            let (width, height) = e.hit_size();
            let half_w = width as f32 / 2.0;
            let height = height as f32;
            // Unterkante der Box, wie hit_pos (Windkugeln haengen tiefer).
            let unten = e.box_y_offset();
            let mut fuesse = |pos: DVec3, [r, g, b]: [f32; 3]| {
                let feet = nah(pos);
                push_box(feet + Vec3::new(-half_w, 0.0, -half_w), feet + Vec3::new(half_w, height, half_w), [r, g, b, a], dichte, gepunktet);
            };
            if roh {
                let (x, y, z) = e.target_pos();
                fuesse(DVec3::new(x, y + unten, z), raw_color);
            } else {
                let (x, y, z) = e.render_pos(progress);
                fuesse(DVec3::new(x, y + unten, z), smoothed_color);
            }
        }
        // Der eigene Spieler, nur in der 3. Person (wie Vanillas F3+B): in der Ich-Sicht sitzt
        // die Kamera in der Box. Er steht nicht in self.entities, Farbe wie die Art Spieler.
        // Mit dem Teams-Modul gehoert sie zu den Verbuendeten -- man ist im eigenen Team.
        let team_eigen = self.team_style_of(teams::Side::Friendly, self.own_uuid.and_then(|u| self.team_facts.get(&u)).and_then(super::super::super::teams::Facts::display_color));
        let eigene = match (team_eigen, stil(HitboxKind::Player)) {
            (Some((f, a, _, _)), _) => Some((f, a)),
            (None, st) => st.on.then(|| art_farbe(st)),
        };
        let (eigen_dichte, eigen_gepunktet) = team_eigen.map_or((dichte, gepunktet), |(_, _, d, p)| (d, p));
        let eigen_roh = team_eigen.map_or(raw_color, |(f, ..)| f);
        // Abschaltbar im Modul ("Eigene Hitbox").
        if let (true, true, false, Some((farbe_geglaettet, a))) = (modul_an, set.module_hitbox_self, self.perspective.is_first_person(), eigene) {
            let half_w = self.physics.width() as f32 / 2.0;
            let height = self.physics.current_height() as f32;
            let mut fuesse = |pos: DVec3, [r, g, b]: [f32; 3]| {
                let feet = nah(pos);
                push_box(feet + Vec3::new(-half_w, 0.0, -half_w), feet + Vec3::new(half_w, height, half_w), [r, g, b, a], eigen_dichte, eigen_gepunktet);
            };
            if spieler_roh {
                fuesse(self.physics.pos, eigen_roh);
            } else {
                fuesse(self.physics_render_prev.lerp(self.physics.pos, f64::from(progress)), farbe_geglaettet);
            }
        }
        // Spinnennetze im Umkreis als ganzer Block (so gross ist ihre Trefferflaeche, auch
        // wenn das Modell nur ein Kreuz ist), Wasserquellen nur in Reichweite des Eimers.
        // Ihre Kanten liegen genau in Flaechen, die schon gezeichnet sind: der Wasseroberflaeche
        // und dem Rahmen, den PvP-Packs ins Netz-Modell malen. Wie der Blockumriss ein kleines
        // Stueck zum Auge gezogen (s. OUTLINE_PULL), sonst gewinnt dort die Flaeche den
        // Tiefentest.
        if netz || wasser {
            let auge = self.physics.eye_pos();
            let reichweite = self.attr_block_range;
            let r = if netz { HITBOX_COBWEB_RADIUS } else { reichweite.ceil() as i32 + 1 };
            let (cx, cy, cz) = (auge.x.floor() as i32, auge.y.floor() as i32, auge.z.floor() as i32);
            let (st_netz, st_wasser) = (stil(HitboxKind::Cobweb), stil(HitboxKind::Water));
            let mit_alpha = |st: hitbox_kinds::HitboxStyle| {
                let ([r, g, b], a) = art_farbe(st);
                [r, g, b, a]
            };
            for x in cx - r..=cx + r {
                for y in cy - r..=cy + r {
                    for z in cz - r..=cz + r {
                        let state = self.world.block_at(x, y, z);
                        if state == world::STATE_AIR {
                            continue;
                        }
                        let ecke = DVec3::new(f64::from(x), f64::from(y), f64::from(z));
                        if netz && world::block_name(state) == Some("cobweb") {
                            push_box(nah(ecke), nah(ecke + DVec3::ONE), mit_alpha(st_netz), dichte, gepunktet);
                        } else if wasser && hitbox_kinds::bucket_source(state) {
                            let Some(h) = world::fluid_height_at(&self.world, x, y, z) else { continue };
                            let max = ecke + DVec3::new(1.0, h, 1.0);
                            // Naechster Punkt der Box zum Auge: liegt er ausserhalb der
                            // Reichweite, trifft der Eimer diese Quelle nicht.
                            if auge.clamp(ecke, max).distance(auge) > reichweite {
                                continue;
                            }
                            push_box(nah(ecke), nah(max), mit_alpha(st_wasser), dichte, gepunktet);
                        }
                    }
                }
            }
        }
        out
    }
}
