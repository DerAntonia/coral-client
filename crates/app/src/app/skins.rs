// Skins, Umhaenge, Koepfe und Server-Ressourcenpakete.

use crate::*;

// Obergrenze fuer App::skin_pixels (s. apply_loaded_skin).
const SKIN_PIXELS_MAX: usize = 1024;

impl App {
    // Spielerliste ausgewertet: fuer jeden Eintrag mit Textur-Property den Skin anfordern.
    // Das Modell (slim/classic) steht schon in der Property und gilt sofort -- so traegt ein
    // Alex-Spieler von der ersten Sekunde an die richtigen Proportionen, auch bevor das Bild
    // da ist (er ist bis dahin schwarz-lila auf dem Slim-Koerper).
    pub(crate) fn handle_player_info(&mut self, entries: Vec<protocol::packets::PlayerInfoEntry>) {
        for entry in entries {
            // Tab-Liste (ClientPlayNetworkHandler.onPlayerList): ADD_PLAYER legt einen frischen
            // Eintrag an, alle anderen Aktionen aendern nur einen vorhandenen.
            if let Some(name) = &entry.name {
                self.tab_entries.insert(entry.uuid, TabEntry::new(name.clone()));
            }
            if let Some(tab) = self.tab_entries.get_mut(&entry.uuid) {
                if let Some(v) = entry.game_mode {
                    tab.game_mode = v;
                }
                if let Some(v) = entry.listed {
                    tab.listed = v;
                }
                if let Some(v) = entry.latency {
                    tab.latency = v;
                }
                if let Some(v) = &entry.display_name {
                    tab.display_name = v.clone();
                }
                if let Some(v) = entry.list_order {
                    tab.list_order = v;
                }
                if let Some(v) = entry.show_hat {
                    tab.show_hat = v;
                }
            }
            // Namen zuerst und unabhaengig von der Textur-Property mitschreiben: ein Spieler
            // ohne Skin (oder ein Eintrag, der nur eine Ping-Aktualisierung traegt) hat
            // trotzdem einen Namen, und ohne ihn faellt sein Schild aus.
            if let Some(name) = entry.name.clone() {
                self.player_names.insert(entry.uuid, name);
            }
            let Some(property) = entry.textures_property.as_deref() else { continue };
            let Some(skin) = auth::parse_textures_property(property) else { continue };
            // Hat dieser Eintrag einen ANDEREN Skin als zuletzt? Tab-Plugins aendern die Koepfe
            // ihrer Platzhalter-Eintraege genau so: dieselbe UUID, neue Textur. Der alte Skin
            // darf dann nicht als "schon geladen" gelten (sonst zeigt die Tab-Liste
            // die falschen Koepfe).
            let neue_url = self.skin_sources.get(&entry.uuid).is_some_and(|alt| alt.url != skin.url);
            let neuer_umhang = self.skin_sources.get(&entry.uuid).is_some_and(|alt| alt.cape_url != skin.cape_url);
            let existing = self.skins.get(&entry.uuid).copied();
            self.skins.insert(
                entry.uuid,
                PlayerSkin { slot: existing.and_then(|s| s.slot), slim: skin.slim, cape_slot: existing.and_then(|s| s.cape_slot) },
            );
            // Die Quelle merken, damit ein spaeter verdraengter Skin nachgeladen werden kann,
            // ohne dass der Server die Liste noch einmal schickt (s. refresh_missing_skins).
            self.skin_sources.insert(entry.uuid, skin.clone());
            // Umhang zuerst anfragen: er haengt an einer eigenen URL und wird unabhaengig vom
            // Skin geladen -- sonst verliert man ihn, sobald der Skin schon angefragt war.
            if let Some(cape_url) = skin.cape_url.clone() {
                if existing.and_then(|s| s.cape_slot).is_none() || neuer_umhang {
                    if self.requested_skins.insert(cape_url.clone()) {
                        self.request_cape(entry.uuid, cape_url);
                    } else {
                        self.wait_for_skin_url(cape_url, entry.uuid);
                    }
                }
            }
            // Schon im Atlas? Dann nichts tun -- Server schicken die Spielerliste oft neu.
            if existing.and_then(|s| s.slot).is_some() && !neue_url {
                continue;
            }
            // Laedt dieselbe URL schon (fuer diesen oder einen anderen Eintrag)? Dann mitwarten.
            if !self.requested_skins.insert(skin.url.clone()) {
                self.wait_for_skin_url(skin.url, entry.uuid);
                continue;
            }
            self.request_skin(entry.uuid, skin);
        }
    }

    // Skins nachladen, die aus dem Atlas verdraengt wurden, deren Spieler aber (wieder) im
    // Bild ist. Ohne das bleibt so ein Spieler bis zum Verbindungsende schwarz-lila:
    // die Spielerliste kommt bei vielen Servern nur einmal beim Betreten.
    //
    // Die Bremse gegen ein Nachlade-Karussell steht gleich am Anfang: nachgeladen wird NUR,
    // wenn ueberhaupt ein Platz frei zu bekommen ist, ohne einem anderen SICHTBAREN Spieler
    // seinen wegzunehmen. Sind mehr Spieler in Sicht als der Atlas Plaetze hat, wuerde jedes
    // Nachladen sonst das naechste ausloesen -- ein Dauerlauf an Downloads, der nie zur Ruhe
    // kaeme und am Ende doch nicht alle zeigen koennte.
    pub(crate) fn refresh_missing_skins(&mut self) {
        // Ist ueberhaupt ein Platz zu bekommen, ohne einem SICHTBAREN Spieler seinen
        // wegzunehmen? Skins und Umhaenge haben getrennte Platzreihen, also auch getrennte
        // Antworten.
        let platz_frei = |order: &std::collections::VecDeque<([u8; 16], usize)>, kapazitaet: usize, app: &Self| {
            order.len() < kapazitaet || order.iter().any(|(u, _)| !app.spieler_sichtbar(u))
        };
        let skin_platz = platz_frei(&self.skin_slot_order, self.entity_atlas.skin_slot_count(), self);
        let umhang_platz = platz_frei(&self.cape_slot_order, self.entity_atlas.cape_slot_count(), self);
        if !skin_platz && !umhang_platz {
            return;
        }

        // Nur fuer Spieler, die tatsaechlich im Bild sind -- und fuer den eigenen Koerper,
        // den man in der 3. Person sieht.
        let mut sichtbar: Vec<[u8; 16]> =
            self.entities.iter().filter(|(_, e)| world::is_player(e.entity_type)).map(|(_, e)| e.uuid).chain(self.own_uuid).collect();
        sichtbar.sort_unstable();
        sichtbar.dedup();

        for uuid in sichtbar {
            let Some(skin) = self.skin_sources.get(&uuid).cloned() else { continue };
            let eintrag = self.skins.get(&uuid).copied();
            // `requested_skins` ist die "laeuft gerade"-Sperre: sie verhindert, dass hier in
            // jedem Frame eine neue Anfrage fuer denselben Spieler rausgeht.
            if skin_platz && eintrag.and_then(|s| s.slot).is_none() && self.requested_skins.insert(skin.url.clone()) {
                self.request_skin(uuid, skin.clone());
            }
            if !umhang_platz || eintrag.and_then(|s| s.cape_slot).is_some() {
                continue;
            }
            if let Some(cape_url) = skin.cape_url.clone() {
                if self.requested_skins.insert(cape_url.clone()) {
                    self.request_cape(uuid, cape_url);
                }
            }
        }
    }

    // Textur eines Schaedel-Items fuers Icon (s. ItemIcons::push_skull): Atlas-Rechteck und ob
    // es eine 64x64-Textur mit Hutlage ist. Der Spielerkopf zeigt den Skin aus seiner
    // profile-Komponente (s. request_head_skins), solange der nicht geladen ist schwarz-lila.
    // Drachen- und Piglinkopf haben eigene Modelle -- die bleiben beim Notbehelf.
    pub(crate) fn skull_texture(&self, name: &str, stack: &protocol::item::ItemStack) -> Option<([f32; 4], bool)> {
        self.skull_texture_for(name, stack.profile.as_ref().and_then(head_skin_key))
    }

    // Dasselbe fuer einen Kopf in einer Hand oder auf einem Kopf (s. world::HeldItem::head_skin).
    pub(crate) fn held_skull_texture(&self, item: &world::HeldItem) -> Option<([f32; 4], bool)> {
        let name = world::item_registry::item_name(item.model_id)?;
        self.skull_texture_for(name, item.head_skin)
    }

    pub(crate) fn skull_texture_for(&self, name: &str, schluessel: Option<[u8; 16]>) -> Option<([f32; 4], bool)> {
        let (textur, hut) = match name {
            "skeleton_skull" => ("entity/skeleton/skeleton", false),
            "wither_skeleton_skull" => ("entity/skeleton/wither_skeleton", false),
            "creeper_head" => ("entity/creeper/creeper", false),
            "zombie_head" => ("entity/zombie/zombie", true),
            "player_head" => {
                if let Some(rect) = schluessel
                    .and_then(|k| self.skins.get(&k))
                    .and_then(|s| s.slot)
                    .and_then(|slot| self.entity_atlas.skin_slot_rect(slot))
                {
                    return Some((rect, true));
                }
                // Kein Skin im Atlas (unterwegs, verdraengt, nicht gefunden oder gar keiner):
                // schwarz-lila statt eines Standard-Gesichts.
                (assets::MISSING_SKIN, true)
            }
            _ => return None,
        };
        self.entity_atlas.rect(textur).map(|r| (r, hut))
    }

    // Skins der Spielerkoepfe im eigenen Inventar und im offenen Container nachladen -- ueber
    // denselben Weg wie die Skins der Spieler (s. apply_loaded_skin), mit einem Schluessel aus
    // der profile-Komponente (s. head_skin_key). Nur solange der Atlas freie Plaetze hat: ein
    // Kopf soll keinem Spieler seinen Skin wegnehmen.
    pub(crate) fn request_head_skins(&mut self) {
        let mut wuensche: Vec<([u8; 16], auth::SkinRef)> = Vec::new();
        let profile: Vec<protocol::item::HeadProfile> = self
            .inventory
            .player
            .iter()
            .chain(self.inventory.open.iter().flat_map(|c| c.slots.iter()))
            .chain(std::iter::once(&self.inventory.cursor))
            .flatten()
            .filter_map(|stack| stack.profile.clone())
            .collect();
        for p in &profile {
            self.note_head_profile(p);
        }
        let profile = profile.iter().filter_map(head_skin_key);
        // Dazu die Koepfe in Haenden und auf Koepfen der Entities im Bild (s. head_textures).
        let gesehen = self
            .entities
            .iter()
            .flat_map(|(_, e)| entity_head_items(e))
            .flatten()
            .filter_map(|h| h.head_skin)
            // Und die platzierten Koepfe in der Naehe.
            .chain(self.placed_heads.iter().filter(|(p, _)| self.near_eye(**p, 64.0)).map(|(_, k)| *k))
            .collect::<Vec<_>>();
        for key in profile.chain(gesehen) {
            if self.skins.get(&key).is_some_and(|s| s.slot.is_some()) || wuensche.iter().any(|(k, _)| *k == key) {
                continue;
            }
            // Die Quelle: die Textur des Kopfes (mitgeschickt oder nachgeschlagen), sonst der
            // Skin des gleichnamigen Spielers aus der Tab-Liste (Kopf mit dessen UUID).
            let quelle = self.head_textures.get(&key).cloned().flatten().or_else(|| self.skin_sources.get(&key).cloned());
            if let Some(skin) = quelle {
                wuensche.push((key, skin));
            }
        }
        // Ein Platz ist frei, oder einer gehoert jemandem, der gerade nicht zu sehen ist --
        // weder als Spieler im Bild noch als Kopf im Inventar oder an einem Entity. Vorher
        // blieben Koepfe auf einem vollen Server (mehr als 128 Skins) grundsaetzlich
        // schwarz-lila, auch bei Spielerkoepfen im Inventar.
        let geschuetzt = self.skins_in_view();
        let mut frei = self.entity_atlas.skin_slot_count().saturating_sub(self.skin_slot_order.len())
            + self.skin_slot_order.iter().filter(|(u, _)| !geschuetzt.contains(u)).count();
        for (key, skin) in wuensche {
            if frei == 0 {
                return;
            }
            frei -= 1;
            self.skins.entry(key).or_insert(PlayerSkin { slot: None, slim: skin.slim, cape_slot: None });
            self.skin_sources.insert(key, skin.clone());
            // Schon einmal geladen: gleich in den Atlas, noch vor dem Zeichnen dieses Bildes.
            // Ueber den Kanal (request_skin) kaeme der Skin erst im naechsten Bild an, und so
            // lange stuende der Kopf schwarz-lila da.
            if let Some(rgba) = self.skin_pixels.get(&skin.url).cloned() {
                self.store_skin(key, &rgba, skin.slim);
            } else if self.requested_skins.insert(skin.url.clone()) {
                self.request_skin(key, skin);
            } else {
                self.wait_for_skin_url(skin.url, key);
            }
        }
    }

    // Koepfe der Kontenliste oben rechts und im Acc-Manager: derselbe Weg wie bei
    // Spielerkoepfen -- Profil einmal bei Mojang nachschlagen (s. note_head_profile), dann den
    // Skin in den Atlas laden. Beim Oeffnen der Liste und wenn ein Profil ankommt; nur in
    // freie Plaetze, ein Konto in der Liste nimmt keinem Spieler seinen Skin weg.
    pub(crate) fn request_account_heads(&mut self) {
        let schluessel: Vec<[u8; 16]> = self.accounts.list.iter().filter_map(|a| uuid_bytes_from_hex(&a.uuid)).collect();
        for key in schluessel {
            if self.skins.get(&key).is_some_and(|s| s.slot.is_some()) {
                continue;
            }
            // Schon einmal geladen: sofort aus dem Speicher, ohne Abruf.
            if let Some((rgba, slim)) = self.account_skins.get(&key).cloned() {
                self.store_skin(key, &rgba, slim);
                continue;
            }
            let Some(skin) = self.head_textures.get(&key).cloned().flatten().or_else(|| self.skin_sources.get(&key).cloned()) else {
                if self.account_head_retry.get(&key).is_some_and(|t| Instant::now() < *t) {
                    continue;
                }
                self.note_head_profile(&protocol::item::HeadProfile { uuid: Some(key), name: None, textures: None });
                continue;
            };
            if self.entity_atlas.skin_slot_count() <= self.skin_slot_order.len() {
                return;
            }
            self.skins.entry(key).or_insert(PlayerSkin { slot: None, slim: skin.slim, cape_slot: None });
            self.skin_sources.insert(key, skin.clone());
            // Schon einmal geladen: gleich in den Atlas, noch vor dem Zeichnen dieses Bildes.
            // Ueber den Kanal (request_skin) kaeme der Skin erst im naechsten Bild an, und so
            // lange stuende der Kopf schwarz-lila da.
            if let Some(rgba) = self.skin_pixels.get(&skin.url).cloned() {
                self.store_skin(key, &rgba, skin.slim);
            } else if self.requested_skins.insert(skin.url.clone()) {
                self.request_skin(key, skin);
            } else {
                self.wait_for_skin_url(skin.url, key);
            }
        }
    }

    // Wessen Skin gerade gebraucht wird, mit Abstand zum Auge: Spieler im Bild und Koepfe an
    // Entities nach ihrer Entfernung, platzierte Koepfe in der Naehe ebenso; der eigene
    // Koerper, die Koepfe im Inventar und die Konten der Kontenliste mit 0. Muss trotzdem ein
    // Gebrauchter weichen, dann der entfernteste (s. claim_atlas_slot_near).
    pub(crate) fn skin_distances(&self) -> std::collections::HashMap<[u8; 16], f32> {
        let auge = self.camera.position;
        let weite = |p: (f64, f64, f64)| (DVec3::new(p.0, p.1, p.2) - auge).length() as f32;
        let mut abstand: std::collections::HashMap<[u8; 16], f32> = std::collections::HashMap::new();
        let mut merke = |k: [u8; 16], d: f32| {
            let e = abstand.entry(k).or_insert(d);
            *e = e.min(d);
        };
        for (_, e) in self.entities.iter() {
            let d = weite(e.pos);
            if world::is_player(e.entity_type) {
                merke(e.uuid, d);
            }
            for h in entity_head_items(e).into_iter().flatten().filter_map(|h| h.head_skin) {
                merke(h, d);
            }
        }
        for (p, k) in &self.placed_heads {
            if self.near_eye(*p, 64.0) {
                merke(*k, weite((p.0 as f64 + 0.5, p.1 as f64 + 0.5, p.2 as f64 + 0.5)));
            }
        }
        for k in self.skins_anywhere() {
            merke(k, 0.0);
        }
        abstand
    }

    // Wessen Skin gerade gebraucht wird: Spieler im Bild und Koepfe an Entities, dazu alles aus
    // skins_anywhere. Diese Plaetze werden nur im Notfall verdraengt (s. skin_distances).
    pub(crate) fn skins_in_view(&self) -> std::collections::HashSet<[u8; 16]> {
        let spieler = self.entities.iter().filter(|(_, e)| world::is_player(e.entity_type)).map(|(_, e)| e.uuid);
        let getragen = self
            .entities
            .iter()
            .flat_map(|(_, e)| entity_head_items(e))
            .flatten()
            .filter_map(|h| h.head_skin);
        spieler.chain(getragen).chain(self.skins_anywhere()).collect()
    }

    // Was unabhaengig vom Ort gebraucht wird: der eigene Koerper, die Koepfe im Inventar, im
    // offenen Container und am Zeiger und die Konten der Kontenliste.
    pub(crate) fn skins_anywhere(&self) -> Vec<[u8; 16]> {
        let inventar = self
            .inventory
            .player
            .iter()
            .chain(self.inventory.open.iter().flat_map(|c| c.slots.iter()))
            .chain(std::iter::once(&self.inventory.cursor))
            .flatten()
            .filter_map(|stack| stack.profile.as_ref().and_then(head_skin_key));
        let konten = self.accounts.list.iter().filter_map(|a| uuid_bytes_from_hex(&a.uuid));
        self.own_uuid.into_iter().chain(inventar).chain(konten).collect()
    }

    // Ein Kopf-Profil vormerken: mit Textur-Property sofort, ohne (nur Name oder UUID, Vanillas
    // ResolvableProfile.Dynamic) wird sie einmal bei Mojang nachgeschlagen -- genau das tut
    // Vanillas ProfileResolver, bevor der Kopf seinen Skin zeigt.
    pub(crate) fn note_head_profile(&mut self, profil: &protocol::item::HeadProfile) {
        let Some(key) = head_skin_key(profil) else { return };
        if let Some(t) = &profil.textures {
            self.head_textures.entry(key).or_insert_with(|| auth::parse_textures_property(t));
            return;
        }
        if self.head_textures.contains_key(&key) {
            return;
        }
        // Schon frueher nachgeschlagen (auch auf einem anderen Server): ohne neuen Abruf.
        if let Some(textures) = self.head_profile_cache.get(&key) {
            let wert = textures.as_deref().and_then(auth::parse_textures_property);
            self.head_textures.insert(key, wert);
            return;
        }
        if !self.head_lookups.insert(key) {
            return;
        }
        // Ein Spieler, der gerade in der Tab-Liste steht, braucht keinen Abruf: sein Skin ist
        // schon da (derselbe Schluessel, die UUID).
        if self.skin_sources.contains_key(&key) {
            return;
        }
        let tx = self.auth_tx.clone();
        let (uuid, name) = (profil.uuid.filter(|u| *u != [0; 16]), profil.name.clone());
        self.tokio_rt.spawn(async move {
            match auth::resolve_profile_textures(uuid, name).await {
                Ok(textures) => {
                    let _ = tx.send(AuthEvent::HeadProfile { key, textures });
                }
                Err(e) => {
                    let _ = tx.send(AuthEvent::HeadProfileFailed { key, meldung: format!("Kopf-Profil nicht geladen: {e}") });
                }
            }
        });
    }

    pub(crate) fn wait_for_skin_url(&mut self, url: String, uuid: [u8; 16]) {
        let wartende = self.skin_waiters.entry(url).or_default();
        if !wartende.contains(&uuid) {
            wartende.push(uuid);
        }
    }

    // Ist dieser Spieler gerade im Bild (bzw. der eigene Koerper)? Massstab fuer die Frage,
    // wessen Atlas-Platz gefahrlos neu belegt werden darf.
    pub(crate) fn spieler_sichtbar(&self, uuid: &[u8; 16]) -> bool {
        Some(*uuid) == self.own_uuid || self.entities.iter().any(|(_, e)| e.uuid == *uuid && world::is_player(e.entity_type))
    }

    // Ein Ladeversuch ist gescheitert. Endgueltige Fehler (PNG nicht dekodierbar) bleiben in
    // der Anfrage-Sperre stehen und werden nie wieder versucht; vorruebergehende (Netz) sind
    // begrenzt oft erlaubt, danach ebenfalls endgueltig.
    pub(crate) fn note_skin_failure(&mut self, url: String, permanent: bool, meldung: String) {
        self.skin_waiters.remove(&url);
        // Ein voller Server hat viele Skins -- jeder Fehlschlag EINMAL im Log, nicht bei
        // jedem Wiederholungsversuch erneut.
        let versuche = self.skin_retries.entry(url.clone()).or_insert(0);
        *versuche = versuche.saturating_add(1);
        let erster = *versuche == 1;
        let aufgeben = permanent || *versuche >= SKIN_MAX_RETRIES;
        if !aufgeben {
            // Sperre loesen -- refresh_missing_skins darf es noch einmal versuchen.
            self.requested_skins.remove(&url);
        }
        if erster || aufgeben {
            self.push_log(meldung);
        }
    }

    // Server-Resourcepack angeboten: annehmen und laden (oder aus dem Cache nehmen).
    pub(crate) fn server_pack_offered(&mut self, push: protocol::packets::ResourcePackPush) {
        use protocol::packets::ResourcePackStatus as S;
        let id = push.id;
        self.push_log(format!("Server-Resourcepack angeboten: {}{}", push.url, if push.required { " (Pflicht)" } else { "" }));
        if !server_packs::valid_url(&push.url) {
            self.send_pack_status(id, S::InvalidUrl);
            return;
        }
        self.send_pack_status(id, S::Accepted);
        if let Some(pfad) = server_packs::cached(&push.hash) {
            self.send_pack_status(id, S::Downloaded);
            self.server_packs.push(id, server_packs::State::Ready(pfad));
            return;
        }
        self.server_packs.push(id, server_packs::State::Downloading);
        let tx = self.auth_tx.clone();
        let name = self.credentials.as_ref().map_or_else(|| "Player".to_string(), |c| c.username.clone());
        let uuid = self.own_uuid.map(|u| u.iter().map(|b| format!("{b:02x}")).collect::<String>()).unwrap_or_default();
        let (url, erwartet) = (push.url, push.hash.to_ascii_lowercase());
        self.tokio_rt.spawn(async move {
            let result = async {
                let (bytes, hash) = auth::fetch_resource_pack(&url, &name, &uuid, MC_VERSION, protocol::packets::PROTOCOL_VERSION, assets::RESOURCE_PACK_FORMAT)
                    .await
                    .map_err(|e| (S::FailedDownload, format!("Server-Resourcepack nicht geladen: {e}")))?;
                if !erwartet.is_empty() && erwartet != hash {
                    return Err((S::FailedDownload, format!("Server-Resourcepack: SHA-1 passt nicht ({hash} statt {erwartet})")));
                }
                let dir = server_packs::cache_dir().ok_or((S::FailedDownload, "kein Home-Ordner".to_string()))?;
                let pfad = dir.join(format!("{hash}.zip"));
                std::fs::create_dir_all(&dir)
                    .and_then(|_| std::fs::write(&pfad, &bytes))
                    .map_err(|e| (S::FailedDownload, format!("Server-Resourcepack nicht gespeichert: {e}")))?;
                Ok(pfad)
            }
            .await;
            let _ = tx.send(AuthEvent::ServerPackDownloaded { id, result });
        });
    }

    pub(crate) fn server_pack_downloaded(&mut self, id: [u8; 16], result: Result<std::path::PathBuf, (protocol::packets::ResourcePackStatus, String)>) {
        match result {
            Ok(pfad) => {
                if self.server_packs.finish(id, server_packs::State::Ready(pfad)) {
                    self.send_pack_status(id, protocol::packets::ResourcePackStatus::Downloaded);
                }
            }
            Err((status, meldung)) => {
                if self.server_packs.finish(id, server_packs::State::Failed) {
                    self.send_pack_status(id, status);
                    self.notify(notifications::Notification::error("Server-Resourcepack", meldung.clone()));
                }
                self.push_log(meldung);
            }
        }
    }

    pub(crate) fn send_pack_status(&self, id: [u8; 16], status: protocol::packets::ResourcePackStatus) {
        if let Some(tx) = &self.outgoing_tx {
            let _ = tx.send(protocol::packets::resource_pack_status_payload(id, status));
        }
    }

    // Umhang im Hintergrund laden -- derselbe Weg wie beim Skin (samt Host-Pruefung und
    // Groessenbremse in auth::fetch_skin_png), nur ein anderer Dekodierer: Capes werden von
    // Vanilla NICHT umgeschrieben und sind fest 64x32 (s. assets::decode_cape_png).
    pub(crate) fn request_cape(&self, uuid: [u8; 16], url: String) {
        let tx = self.auth_tx.clone();
        self.tokio_rt.spawn(async move {
            match auth::fetch_skin_png(&url).await {
                Ok(png) => match assets::decode_cape_png(&png) {
                    Some(rgba) => {
                        let _ = tx.send(AuthEvent::CapeLoaded { uuid, url: url.clone(), rgba });
                    }
                    None => {
                        let meldung = format!("Umhang nicht dekodierbar (kein 64x32?): {url}");
                        let _ = tx.send(AuthEvent::SkinFailed { url, permanent: true, meldung });
                    }
                },
                Err(e) => {
                    let meldung = format!("Umhang nicht geladen: {e}");
                    let _ = tx.send(AuthEvent::SkinFailed { url, permanent: false, meldung });
                }
            }
        });
    }

    // Skin im Hintergrund laden und dekodieren. Das Dekodieren passiert im Task, nicht im
    // Hauptthread: ein PNG zu entpacken dauert Millisekunden, und die will man nicht im
    // Frame haben.
    pub(crate) fn request_skin(&self, uuid: [u8; 16], skin: auth::SkinRef) {
        let tx = self.auth_tx.clone();
        let url = skin.url.clone();
        let slim = skin.slim;
        // Schon einmal geladen: dieselbe Meldung sofort, ohne Download (s. skin_pixels).
        if let Some(rgba) = self.skin_pixels.get(&url) {
            let _ = tx.send(AuthEvent::SkinLoaded { uuid, url, rgba: rgba.clone(), slim });
            return;
        }
        self.tokio_rt.spawn(async move {
            match auth::fetch_skin_png(&url).await {
                Ok(png) => match assets::decode_skin_png(&png) {
                    Some(rgba) => {
                        let _ = tx.send(AuthEvent::SkinLoaded { uuid, url: url.clone(), rgba, slim });
                    }
                    None => {
                        let meldung = format!("Skin nicht dekodierbar: {url}");
                        let _ = tx.send(AuthEvent::SkinFailed { url, permanent: true, meldung });
                    }
                },
                Err(e) => {
                    let meldung = format!("Skin nicht geladen: {e}");
                    let _ = tx.send(AuthEvent::SkinFailed { url, permanent: false, meldung });
                }
            }
        });
    }

    // Geladenen Skin in einen Atlas-Platz schreiben. Sind alle Plaetze belegt, wird der
    // aelteste wiederverwendet -- dessen Spieler ist wieder schwarz-lila.
    pub(crate) fn apply_loaded_skin(&mut self, uuid: [u8; 16], url: String, rgba: Vec<u8>, slim: bool) {
        // Die Anfrage ist durch: aus der "laeuft gerade"-Sperre austragen, damit derselbe
        // Skin nach einer spaeteren Verdraengung erneut geholt werden darf.
        self.requested_skins.remove(&url);
        // Obergrenze nur gegen endloses Wachsen auf grossen Servern (ein Skin sind 16 KiB).
        if self.skin_pixels.len() >= SKIN_PIXELS_MAX && !self.skin_pixels.contains_key(&url) {
            self.skin_pixels.clear();
        }
        self.skin_pixels.insert(url.clone(), rgba.clone());
        let wartende = self.skin_waiters.remove(&url).unwrap_or_default();
        // Skins der Konten fuer spaeter merken (s. account_skins).
        for k in std::iter::once(uuid).chain(wartende.iter().copied()) {
            if self.accounts.list.iter().any(|a| uuid_bytes_from_hex(&a.uuid) == Some(k)) {
                let slim = if k == uuid { slim } else { self.skins.get(&k).map_or(slim, |s| s.slim) };
                self.account_skins.insert(k, (rgba.clone(), slim));
            }
        }
        self.store_skin(uuid, &rgba, slim);
        // Dieselbe Textur fuer alle, die auf diese URL gewartet haben -- mit ihrer eigenen
        // Armbreite aus der Property.
        // Auch wer schon einen Platz hat, bekommt die Textur: er wartet ja gerade deshalb, weil
        // sein Eintrag einen NEUEN Skin bekommen hat (s. handle_player_info).
        for w in wartende {
            let Some(eintrag) = self.skins.get(&w).copied() else { continue };
            if w != uuid {
                self.store_skin(w, &rgba, eintrag.slim);
            }
        }
    }

    pub(crate) fn store_skin(&mut self, uuid: [u8; 16], rgba: &[u8], slim: bool) {
        self.store_tab_head(uuid, rgba);
        let Some(gpu) = self.gpu.as_ref() else { return };
        let current = self.skins.get(&uuid).and_then(|s| s.slot);
        // Wer gerade im Bild steht, darf nicht verdraengt werden -- der Neue schon gar nicht
        // gegen sich selbst. Die Liste wird VOR dem Aufruf gezogen, weil claim_atlas_slot
        // `self.skin_slot_order` exklusiv leiht.
        let abstand = self.skin_distances();
        let Some((slot, evicted)) =
            claim_atlas_slot_near(&mut self.skin_slot_order, self.entity_atlas.skin_slot_count(), uuid, current, |u| abstand.get(u).copied())
        else {
            return;
        };
        if let Some(old) = evicted.and_then(|e| self.skins.get_mut(&e)) {
            old.slot = None;
        }
        let Some(origin) = self.entity_atlas.skin_slot_origin(slot) else { return };
        gpu.block_renderer.upload_skin(&gpu.queue, origin, assets::SKIN_SIZE, rgba);
        let cape_slot = self.skins.get(&uuid).and_then(|s| s.cape_slot);
        self.skins.insert(uuid, PlayerSkin { slot: Some(slot), slim, cape_slot });
        // Nur der eigene Skin wird geloggt: bei fremden Spielern waere das auf einem vollen
        // Server eine Log-Flut, und "sieht mein eigener Skin richtig aus" ist die Frage, die
        // man ueberhaupt pruefen will (Fehlschlaege melden sich schon per AuthEvent::Status).
        if self.own_uuid == Some(uuid) {
            self.push_log(format!("eigener Skin geladen ({}).", if slim { "slim" } else { "classic" }));
        }
    }

    // Gesicht (8,8) und Hutlage (40,8) des Skins in einen eigenen Kopf-Platz (s.
    // assets::HEAD_SLOTS). Verdraengt wird nur, wer nicht mehr in der Tab-Liste steht.
    pub(crate) fn store_tab_head(&mut self, uuid: [u8; 16], rgba: &[u8]) {
        if rgba.len() < (assets::SKIN_SIZE * assets::SKIN_SIZE * 4) as usize {
            return;
        }
        let mut pixel = vec![0u8; (assets::HEAD_SLOT_WIDTH * assets::HEAD_SLOT_HEIGHT * 4) as usize];
        for y in 0..8usize {
            for (ziel_x, quelle_x) in [(0usize, 8usize), (8, 40)] {
                let quelle = ((8 + y) * 64 + quelle_x) * 4;
                let ziel = (y * 16 + ziel_x) * 4;
                pixel[ziel..ziel + 32].copy_from_slice(&rgba[quelle..quelle + 32]);
            }
        }
        let current = self.tab_heads.get(&uuid).map(|(slot, _)| *slot);
        let in_liste: std::collections::HashSet<[u8; 16]> = self.tab_entries.keys().copied().collect();
        let Some((slot, evicted)) =
            claim_atlas_slot(&mut self.tab_head_order, self.entity_atlas.head_slot_count(), uuid, current, |u| in_liste.contains(u))
        else {
            return;
        };
        if let Some(alt) = evicted {
            self.tab_heads.remove(&alt);
        }
        if let (Some(gpu), Some(origin)) = (self.gpu.as_ref(), self.entity_atlas.head_slot_origin(slot)) {
            gpu.block_renderer.upload_item(&gpu.queue, origin, assets::HEAD_SLOT_WIDTH, assets::HEAD_SLOT_HEIGHT, &pixel);
        }
        self.tab_heads.insert(uuid, (slot, pixel));
    }

    // Dasselbe fuer den Umhang. Eigene Platzreihe, eigene Reihenfolge -- ein Spieler, dessen
    // SKIN aus dem Atlas verdraengt wurde, soll nicht auch seinen Umhang verlieren (und
    // umgekehrt), denn beide werden unabhaengig voneinander geladen.
    pub(crate) fn apply_loaded_cape(&mut self, uuid: [u8; 16], url: String, rgba: Vec<u8>) {
        self.requested_skins.remove(&url);
        let wartende = self.skin_waiters.remove(&url).unwrap_or_default();
        self.store_cape(uuid, &rgba);
        for w in wartende {
            if w != uuid && self.skins.get(&w).is_none_or(|s| s.cape_slot.is_none()) {
                self.store_cape(w, &rgba);
            }
        }
    }

    pub(crate) fn store_cape(&mut self, uuid: [u8; 16], rgba: &[u8]) {
        let Some(gpu) = self.gpu.as_ref() else { return };
        let current = self.skins.get(&uuid).and_then(|s| s.cape_slot);
        let sichtbare: std::collections::HashSet<[u8; 16]> =
            self.entities.iter().filter(|(_, e)| world::is_player(e.entity_type)).map(|(_, e)| e.uuid).chain(self.own_uuid).collect();
        let Some((slot, evicted)) =
            claim_atlas_slot(&mut self.cape_slot_order, self.entity_atlas.cape_slot_count(), uuid, current, |u| sichtbare.contains(u))
        else {
            return;
        };
        if let Some(old) = evicted.and_then(|e| self.skins.get_mut(&e)) {
            old.cape_slot = None;
        }
        let Some(origin) = self.entity_atlas.cape_slot_origin(slot) else { return };
        gpu.block_renderer.upload_cape(&gpu.queue, origin, assets::CAPE_WIDTH, assets::CAPE_HEIGHT, rgba);
        // Ein Umhang kann VOR dem Skin ankommen (zwei unabhaengige Downloads). Dann gibt es
        // noch keinen Eintrag, und der wird hier angelegt -- ohne Skin-Platz, aber mit
        // Umhang. Die Armbreite ist zu diesem Zeitpunkt schon aus der Property bekannt
        // (handle_player_info legt den Eintrag an), im Zweifel classic.
        let existing = self.skins.get(&uuid).copied();
        self.skins.insert(
            uuid,
            PlayerSkin { slot: existing.and_then(|s| s.slot), slim: existing.map(|s| s.slim).unwrap_or(false), cape_slot: Some(slot) },
        );
        if self.own_uuid == Some(uuid) {
            self.push_log("eigener Umhang geladen.".to_string());
        }
    }
}
