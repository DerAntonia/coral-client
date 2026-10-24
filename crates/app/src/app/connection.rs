// Verbinden und Trennen, Microsoft-Login und Kontenverwaltung.

use crate::*;

impl App {
    // Verbindet; laeuft schon eine Verbindung, wird sie vorher getrennt (Reconnect).
    pub(crate) fn start_connect(&mut self) {
        let previous = self.conn_task.take();
        if previous.is_some() {
            self.push_log("== reconnect ==");
        }
        self.conn_status = ConnStatus::Connecting;
        self.conn_route = None;
        self.reset_world();
        self.server_packs.clear();
        self.sky_time_pending = true;
        self.lod_preview_saved();
        self.request_connect_view();
        self.connect_view_taken = false;
        self.connect_view_first_at = None;

        let address = self.server_address.clone();
        self.server_packs.set_address(&address);
        // Der Sofort-Ton-Haken (s. net::Events::with_immediate). Er laeuft im Netzwerk-Task,
        // also darf er NUR auf threadsichere Dinge zugreifen: den Mixer-Griff und die
        // veroeffentlichte Hand. Beides ist genau dafuer gebaut.
        let sink = self.audio.sink();
        let hand = std::sync::Arc::clone(&self.totem_hand_published);
        let plan = std::sync::Arc::clone(&self.own_pop_plan);
        let own_entity = std::sync::Arc::clone(&self.own_entity_id_published);
        // Liegt der Tab hinten, ist er stumm und schickt nur KeepAlive und Pong (s. app/tabs.rs).
        let hinten = std::sync::Arc::clone(&self.tabs[self.active_tab].background);
        let hinten_ton = std::sync::Arc::clone(&hinten);
        let events = net::Events::new(self.events_tx.clone()).with_immediate(std::sync::Arc::new(move |event: &net::ClientEvent| {
            let net::ClientEvent::EntityEvent { entity_id, status } = event else { return };
            if hinten_ton.load(std::sync::atomic::Ordering::Relaxed) {
                return;
            }
            if *status != protocol::packets::ENTITY_STATUS_USE_TOTEM {
                return;
            }
            if own_entity.load(std::sync::atomic::Ordering::Relaxed) != *entity_id {
                return;
            }
            // Die Hand hat der Hauptthread schon entschieden (s. publish_totem_hand) --
            // hier wird nur noch nachgesehen. Die Entscheidung zweimal zu rechnen waere
            // die eine Sache, die hier wirklich schiefgehen koennte: Bild und Ton wuerden
            // auseinanderlaufen.
            let hand = modules::TotemHand::from_published(hand.load(std::sync::atomic::Ordering::Relaxed));
            // Den Plan nur kurz ansehen und die Stimme herausnehmen -- gespielt wird erst
            // nach dem Loslassen, damit der Mixer-Lock nie im Plan-Lock steckt.
            let plan = match plan.lock() {
                Ok(plan) => plan.clone(),
                Err(_) => return,
            };
            sound_log::line(|| {
                let was = match &plan {
                    toteming::OwnPopPlan::Silent => "stumm",
                    toteming::OwnPopPlan::Tone => "Ton (Pop-Klaenge-Modul)",
                    toteming::OwnPopPlan::Samples { .. } => "Aufnahme",
                };
                format!("{:<10} {:<44} {:<9} {:<30}     -     {was}, {hand:?} (eigener Totem-Pop, sofort im Netzwerk-Task)", "client", "item.totem.use", "Players", "selbst")
            });
            match plan {
                toteming::OwnPopPlan::Silent => {}
                toteming::OwnPopPlan::Tone => sink.play(hand.sound_hz(), hand.sound_pan()),
                toteming::OwnPopPlan::Samples { main, off } => {
                    let stimme = if hand == modules::TotemHand::MainHand { main } else { off };
                    if let Some(v) = stimme {
                        sink.play_sample(v.sample, v.gain, 0.0, v.pitch);
                    }
                }
            }
        }))
        // Rohe Pakete fuer Addons (Hook::PacketIn), nur solange eines sie hoert.
        .with_raw(std::sync::Arc::clone(&self.addon_raw_in))
        .with_background(hinten);
        // Angemeldet -> echtes Konto (Online-Mode). Sonst Offline-Mode mit "Player" (wie
        // bisher, fuer den lokalen Test-Server mit online-mode=false).
        let credentials = self.credentials.clone();
        // Erneuerung des aktiven Kontos fuer join_refreshing: vor dem Verbinden, wenn das
        // Token schon abgelaufen ist, sonst erst, wenn der Session-Server es ablehnt.
        let abgelaufen = credentials.as_ref().and_then(|c| accounts::expiry(&c.access_token)).is_some_and(|exp| exp <= accounts::now_unix() + 60);
        let mut refresh = self.token_refresh();
        // Neuer Kanal pro Verbindung: der alte (falls vorhanden) faellt mit dem alten Task
        // weg, run_play_loop dort bemerkt das an outgoing.recv() == None und stoppt das
        // Senden sauber (s. connection.rs), statt in eine Busy-Loop zu laufen.
        let (outgoing_tx, outgoing_rx) = tokio::sync::mpsc::unbounded_channel();
        self.outgoing_tx = Some(addons::PacketTx::new(outgoing_tx, self.addon_hold_packets));
        let rotation = std::sync::Arc::clone(&self.shared_rotation);
        self.sync_lag_delay();
        // join() haelt die Verbindung dauerhaft offen (Keep-Alive etc.), daher als
        // Hintergrund-Task spawnen statt block_on -- sonst friert die Render-Loop ein.
        self.conn_task = Some(self.tokio_rt.spawn(async move {
            // Erst die alte Verbindung wirklich abbauen. abort() wirkt nicht sofort,
            // sondern beendet den Task erst an dessen naechstem await -- ohne das
            // await hier wuerden sich alte und neue Verbindung kurz ueberlappen und
            // der Server saehe zwei Logins desselben Spielers.
            if let Some(prev) = previous {
                prev.abort();
                let _ = prev.await;
            }
            let mut credentials = credentials;
            if abgelaufen {
                if let Some(erneuern) = refresh.take() {
                    let _ = events.send(net::ClientEvent::Log("Access-Token abgelaufen -- erneuere es ...".into()));
                    match erneuern.await {
                        Ok(c) => credentials = Some(c),
                        Err(e) => {
                            let _ = events.send(net::ClientEvent::Disconnected(format!("Token erneuern fehlgeschlagen: {e}")));
                            return;
                        }
                    }
                }
            }
            let _ = net::join_refreshing(&address, "Player", credentials, refresh, events, outgoing_rx, rotation).await;
        }));
    }

    // Holt dem angemeldeten Konto ein frisches Token ueber sein Refresh-Token und meldet es
    // wie eine Kontopruefung zurueck (Liste und aktive Anmeldung bekommen es). None ohne
    // Refresh-Token, dann laesst sich nichts erneuern.
    pub(crate) fn token_refresh(&self) -> Option<net::TokenRefresh> {
        let c = self.credentials.as_ref()?;
        let (refresh_token, client_id) = match self.accounts.find(&c.profile_id).map(|i| &self.accounts.list[i]) {
            Some(a) => (a.refresh_token.clone(), a.client_id.trim().to_string()),
            None => (self.settings.msa_refresh_token.clone(), String::new()),
        };
        if refresh_token.trim().is_empty() {
            return None;
        }
        let client_id = if client_id.is_empty() { AZURE_CLIENT_ID.to_string() } else { client_id };
        let tx = self.auth_tx.clone();
        Some(Box::pin(async move {
            let msa = auth::refresh_msa(&client_id, refresh_token.trim()).await.map_err(|e| e.to_string())?;
            let c = auth::msa_to_minecraft(&msa.access_token).await.map_err(|e| e.to_string())?;
            let _ = tx.send(AuthEvent::AccountChecked { key: accounts::key(&c.profile_id), result: Ok((c.clone(), Some((msa.refresh_token, client_id)))) });
            Ok(c)
        }))
    }

    // Startet den Microsoft-Device-Code-Login als Hintergrund-Task. Der Ablauf schickt seine
    // Zwischenstaende (Code zum Anzeigen, fertige Anmeldung, Fehler) per auth_tx zurueck, die
    // drain_auth_events() im Hauptthread verarbeitet.
    // Zuerst der Browser-Login (Kontoauswahl, Microsoft leitet auf einen lokalen Empfaenger
    // zurueck); nur wenn dafuer kein lokaler Port frei ist, der Device-Code.
    pub(crate) fn start_microsoft_login(&mut self) {
        // Ein erneuter Klick faengt neu an -- die alte Anmeldung wird im Browser oft gar nicht
        // mehr abgeschlossen (Fenster zu, falsches Konto).
        self.cancel_device_login();
        if self.login_busy {
            return;
        }
        let client_id = AZURE_CLIENT_ID.to_string();
        // Die Seite nach der Rueckleitung in den Farben des Themes.
        let seite = {
            use theme::{srgb, Slot};
            auth::PageTheme {
                background: srgb(Slot::BgApp),
                panel: srgb(Slot::BgPanel),
                border: srgb(Slot::Border1),
                text: srgb(Slot::Text1),
                text_2: srgb(Slot::Text2),
                success: srgb(Slot::Success),
                danger: srgb(Slot::Danger),
            }
        };
        self.login_busy = true;
        self.push_log("Microsoft-Anmeldung wird vorbereitet ...");
        let tx = self.auth_tx.clone();
        self.device_login = Some(self.tokio_rt.spawn(async move {
            let msa = match auth::start_browser_login(&client_id).await {
                Ok(login) => {
                    let _ = tx.send(AuthEvent::BrowserLogin { url: login.url.clone() });
                    auth::finish_browser_login(login, seite).await
                }
                Err(e) => {
                    let _ = tx.send(AuthEvent::Status(format!("{e} -- weiter mit der Code-Anmeldung")));
                    match auth::request_device_code(&client_id).await {
                        Ok(dc) => {
                            let _ = tx.send(AuthEvent::DeviceCode { user_code: dc.user_code.clone(), verification_uri: dc.verification_uri.clone() });
                            auth::poll_for_msa(&client_id, &dc).await
                        }
                        Err(e) => Err(e),
                    }
                }
            };
            let msa = match msa {
                Ok(m) => m,
                Err(e) => {
                    let _ = tx.send(AuthEvent::Failed(e.to_string()));
                    return;
                }
            };
            let _ = tx.send(AuthEvent::Status("Angemeldet -- hole Minecraft-Profil ...".into()));
            match auth::msa_to_minecraft(&msa.access_token).await {
                Ok(creds) => {
                    let _ = tx.send(AuthEvent::LoggedIn { credentials: creds, msa: Some((msa.refresh_token, client_id.clone())), source: accounts::Source::Microsoft });
                }
                Err(e) => {
                    let _ = tx.send(AuthEvent::Failed(e.to_string()));
                }
            }
        }));
    }

    // Einen noch wartenden Microsoft-Login abbrechen: sonst hielte login_busy jede andere
    // Anmeldung bis zum Ablauf des Codes auf. Nur auf einen neuen Anmeldeversuch hin, nicht
    // bei der lautlosen Erneuerung (try_refresh_login).
    pub(crate) fn cancel_device_login(&mut self) {
        let Some(task) = self.device_login.take() else { return };
        if !task.is_finished() {
            task.abort();
            self.login_busy = false;
            self.push_log("Vorherige Microsoft-Anmeldung abgebrochen.");
        }
    }

    // Nutzt ein manuell eingegebenes Minecraft-Access-Token (Token-Feld) direkt -- holt nur
    // noch Name + UUID dazu. Kein Refresh-Token (das gibt es nur ueber den Microsoft-Login).
    pub(crate) fn submit_token_login(&mut self, token: String, source: accounts::Source) {
        let token = token.trim().to_string();
        if token.is_empty() {
            return;
        }
        self.cancel_device_login();
        self.login_busy = true;
        self.push_log("Token wird geprüft ...");
        let tx = self.auth_tx.clone();
        self.tokio_rt.spawn(async move {
            match auth::credentials_from_token(&token).await {
                Ok(creds) => {
                    let _ = tx.send(AuthEvent::LoggedIn { credentials: creds, msa: None, source });
                }
                Err(e) => {
                    let _ = tx.send(AuthEvent::Failed(e.to_string()));
                }
            }
        });
    }

    // Login mit einem eingefuegten Microsoft-Refresh-Token: derselbe Tausch wie beim gespeicherten
    // Login (try_refresh_login), nur mit dem eingegebenen Token. Microsoft nimmt ein Refresh-Token
    // ausschliesslich zusammen mit der Client-ID an, fuer die es ausgestellt wurde -- es muss
    // also von Coral Client stammen (z.B. "Refresh-Token exportieren"). Das neue Refresh-Token
    // aus der Antwort wird gespeichert, die Anmeldung erneuert sich danach selbst.
    pub(crate) fn submit_refresh_token_login(&mut self, token: String) {
        let refresh_token = token.trim().to_string();
        if refresh_token.is_empty() {
            return;
        }
        self.cancel_device_login();
        if self.login_busy {
            return;
        }
        self.redeem_refresh_token(refresh_token, AZURE_CLIENT_ID.to_string());
    }

    // Refresh-Token gegen frische Tokens tauschen und damit anmelden (eingefuegtes Token oder
    // ein gespeichertes Microsoft-Konto aus der Liste, dann mit dessen Client-ID).
    pub(crate) fn redeem_refresh_token(&mut self, refresh_token: String, client_id: String) {
        if self.login_busy {
            return;
        }
        self.login_busy = true;
        self.push_log("Refresh-Token wird eingelöst ...");
        let tx = self.auth_tx.clone();
        self.tokio_rt.spawn(async move {
            let msa = match auth::refresh_msa(&client_id, &refresh_token).await {
                Ok(m) => m,
                Err(e) => {
                    let _ = tx.send(AuthEvent::Failed(format!("{e} -- stammt das Token von Coral Client?")));
                    return;
                }
            };
            let _ = tx.send(AuthEvent::Status("Token gültig -- hole Minecraft-Profil ...".into()));
            match auth::msa_to_minecraft(&msa.access_token).await {
                Ok(creds) => {
                    let _ = tx.send(AuthEvent::LoggedIn { credentials: creds, msa: Some((msa.refresh_token, client_id.clone())), source: accounts::Source::Microsoft });
                }
                Err(e) => {
                    let _ = tx.send(AuthEvent::Failed(e.to_string()));
                }
            }
        });
    }

    // Beim Betreten des Connect-Screens: falls ein gespeichertes Refresh-Token vorliegt und
    // wir noch nicht angemeldet sind, im Hintergrund lautlos erneuern -- so bleibt man ueber
    // Neustarts eingeloggt, ohne erneut den Device-Code eingeben zu muessen.
    pub(crate) fn try_refresh_login(&mut self) {
        if self.login_busy || self.credentials.is_some() {
            return;
        }
        // Das Refresh-Token gilt nur mit der Client-ID, fuer die es ausgestellt wurde.
        let client_id = self
            .accounts
            .find(&self.settings.account_uuid)
            .map(|i| self.accounts.list[i].client_id.trim().to_string())
            .filter(|c| !c.is_empty())
            .unwrap_or_else(|| AZURE_CLIENT_ID.to_string());
        let refresh_token = self.settings.msa_refresh_token.trim().to_string();
        if refresh_token.is_empty() {
            return;
        }
        self.login_busy = true;
        let tx = self.auth_tx.clone();
        self.tokio_rt.spawn(async move {
            let msa = match auth::refresh_msa(&client_id, &refresh_token).await {
                Ok(m) => m,
                Err(e) => {
                    let _ = tx.send(AuthEvent::Failed(e.to_string()));
                    return;
                }
            };
            match auth::msa_to_minecraft(&msa.access_token).await {
                Ok(creds) => {
                    let _ = tx.send(AuthEvent::LoggedIn { credentials: creds, msa: Some((msa.refresh_token, client_id.clone())), source: accounts::Source::Microsoft });
                }
                Err(e) => {
                    let _ = tx.send(AuthEvent::Failed(e.to_string()));
                }
            }
        });
    }

    // Angemeldetes Konto setzen und fuer den naechsten Start merken. Das Refresh-Token kommt
    // aus der Kontenliste: ein Konto ohne eigenes darf nicht das des vorigen behalten, sonst
    // meldete try_refresh_login einen beim naechsten Start wieder beim vorigen an.
    pub(crate) fn set_active_account(&mut self, credentials: auth::Credentials) {
        self.settings.account_name = credentials.username.clone();
        self.settings.account_uuid = credentials.profile_id.clone();
        self.settings.mc_access_token = credentials.access_token.clone();
        self.settings.msa_refresh_token = self.accounts.find(&credentials.profile_id).map(|i| self.accounts.list[i].refresh_token.clone()).unwrap_or_default();
        self.credentials = Some(credentials);
        self.persist_settings();
        // Ein neu angelegtes Konto hat noch keinen Kopf geladen.
        self.request_account_heads();
    }

    pub(crate) fn persist_accounts(&mut self) {
        if let Err(e) = accounts::save(&self.accounts) {
            self.push_log(format!("Kontenliste konnte nicht gespeichert werden: {e}"));
        }
    }

    pub(crate) fn account_check(&self, a: &accounts::Account) -> accounts::Check {
        self.account_checks.get(&accounts::key(&a.uuid)).map_or(accounts::Check::Unknown, |(c, _)| c.clone())
    }

    pub(crate) fn is_active_account(&self, a: &accounts::Account) -> bool {
        self.credentials.as_ref().is_some_and(|c| accounts::key(&c.profile_id) == accounts::key(&a.uuid))
    }

    // Als gewaehlt zeigen: waehrend eines Wechsels das neue Konto, sonst das angemeldete.
    pub(crate) fn shown_as_active(&self, a: &accounts::Account) -> bool {
        match &self.account_switching {
            Some((k, _)) => *k == accounts::key(&a.uuid),
            None => self.is_active_account(a),
        }
    }

    // Das Konto, auf das gerade gewechselt wird.
    pub(crate) fn switching_account(&self) -> Option<&accounts::Account> {
        let (k, _) = self.account_switching.as_ref()?;
        self.accounts.list.iter().find(|a| accounts::key(&a.uuid) == *k)
    }

    // Beim Oeffnen der Kontenliste und des Acc-Managers: Koepfe laden und alle Konten pruefen.
    pub(crate) fn refresh_account_list(&mut self) {
        self.request_account_heads();
        self.check_accounts();
    }

    // Alle gespeicherten Konten pruefen: das Token beim Profil-Endpunkt vorzeigen; ist das
    // des aktiven Kontos abgelaufen, wird es erneuert. Fremde Konten mit abgelaufenem Token
    // erneuert erst der Wechsel (use_account) -- jede Erneuerung laeuft ueber Mojangs knapp
    // begrenzte Anmeldung (login_with_xbox), und alle auf einmal reissen das Limit. Darum auch
    // nacheinander, das aktive zuerst. Ein Konto hoechstens einmal je ACCOUNT_RECHECK.
    pub(crate) fn check_accounts(&mut self) {
        let jetzt = accounts::now_unix();
        let mut reihe = Vec::new();
        for a in self.accounts.list.clone() {
            let key = accounts::key(&a.uuid);
            if let Some((check, wann)) = self.account_checks.get(&key) {
                if *check == accounts::Check::Checking || wann.elapsed() < ACCOUNT_RECHECK {
                    continue;
                }
            }
            let abgelaufen = accounts::expiry(&a.access_token).is_some_and(|exp| exp <= jetzt + 60);
            // Abgelaufen und nichts zum Erneuern: das steht auch ohne Netz fest.
            if abgelaufen && a.refresh_token.is_empty() {
                self.account_checks.insert(key, (accounts::Check::Invalid("abgelaufen".into()), Instant::now()));
                continue;
            }
            let aktiv = self.is_active_account(&a);
            if abgelaufen && !aktiv {
                continue;
            }
            self.account_checks.insert(key, (accounts::Check::Checking, Instant::now()));
            if aktiv {
                reihe.insert(0, a);
            } else {
                reihe.push(a);
            }
        }
        if reihe.is_empty() {
            return;
        }
        let tx = self.auth_tx.clone();
        self.tokio_rt.spawn(async move {
            for a in reihe {
                let result = async {
                    // Ein Token kurz vor dem Ablauf gleich erneuern statt es noch vorzuzeigen.
                    if accounts::expiry(&a.access_token).is_none_or(|exp| exp > jetzt + 60) {
                        match auth::credentials_from_token(&a.access_token).await {
                            Ok(c) => return Ok((c, None)),
                            Err(e) if a.refresh_token.is_empty() => return Err(e),
                            Err(_) => {}
                        }
                    }
                    let msa = auth::refresh_msa(&a.client_id, &a.refresh_token).await?;
                    let c = auth::msa_to_minecraft(&msa.access_token).await?;
                    Ok((c, Some((msa.refresh_token, a.client_id.clone()))))
                }
                .await;
                let _ = tx.send(AuthEvent::AccountChecked { key: accounts::key(&a.uuid), result });
            }
        });
    }

    // Klick auf ein Konto: damit anmelden. Ein geprueftes, noch laufendes Token wird direkt
    // genommen; sonst erst erneuern (Microsoft) bzw. pruefen.
    pub(crate) fn use_account(&mut self, i: usize) {
        let Some(a) = self.accounts.list.get(i).cloned() else { return };
        if self.is_active_account(&a) {
            return;
        }
        self.cancel_device_login();
        if self.login_busy {
            return;
        }
        let laeuft = accounts::expiry(&a.access_token).is_none_or(|exp| exp > accounts::now_unix() + 60);
        // Ein laufendes Token gleich nehmen, auch waehrend die Pruefung noch laeuft (s.
        // check_accounts) -- sonst ging jeder Klick kurz nach dem Oeffnen der Liste den langen
        // Weg ueber Microsoft. Stellt die Pruefung es danach als ungueltig heraus, zeigt die
        // Liste das wie sonst auch.
        let ungueltig = matches!(self.account_check(&a), accounts::Check::Invalid(_));
        if !ungueltig && laeuft {
            self.push_log(format!("Angemeldet als {}", a.name));
            self.notify(notifications::Notification::success("Angemeldet", format!("als {}", a.name)));
            self.set_active_account(a.credentials());
        } else {
            // Sofort als gewaehlt zeigen, die Anmeldung laeuft im Hintergrund (s. account_switching).
            self.account_switching = Some((accounts::key(&a.uuid), Instant::now()));
            if !a.refresh_token.is_empty() {
                self.redeem_refresh_token(a.refresh_token, a.client_id);
            } else {
                self.submit_token_login(a.access_token, a.source);
            }
        }
    }

    // Rechtsklick-Menue "Account vergessen": aus der Liste nehmen; war es angemeldet, auch
    // abmelden.
    pub(crate) fn forget_account(&mut self, i: usize) {
        let aktiv = self.accounts.list.get(i).is_some_and(|a| self.is_active_account(a));
        let Some(a) = self.accounts.forget(i) else { return };
        self.account_checks.remove(&accounts::key(&a.uuid));
        self.persist_accounts();
        if aktiv {
            self.logout();
        }
        self.push_log(format!("Konto {} vergessen.", a.name));
    }

    // Meldet ab: verwirft Credentials und das gespeicherte Refresh-Token (naechster Connect
    // laeuft wieder Offline-Mode). Das Konto bleibt in der Liste.
    pub(crate) fn logout(&mut self) {
        self.credentials = None;
        self.settings.msa_refresh_token.clear();
        self.settings.account_name.clear();
        self.settings.mc_access_token.clear();
        self.settings.account_uuid.clear();
        self.persist_settings();
        self.push_log("Abgemeldet.");
    }

    // Auth-Ereignisse aus den Hintergrund-Tasks verarbeiten (analog drain_events).
    pub(crate) fn drain_auth_events(&mut self) {
        while let Ok(event) = self.auth_rx.try_recv() {
            match event {
                AuthEvent::DeviceCode { user_code, verification_uri } => {
                    // Code in die Zwischenablage legen und den Browser oeffnen -- dann muss der
                    // Nutzer den Code nur noch einfuegen.
                    if let Ok(mut cb) = arboard::Clipboard::new() {
                        let _ = cb.set_text(user_code.clone());
                    }
                    let _ = open_external_command(&verification_uri).spawn();
                    self.push_log(format!("Im Browser anmelden: {verification_uri}"));
                    self.push_log(format!("Code (schon kopiert): {user_code}"));
                }
                AuthEvent::BrowserLogin { url } => match open_external_command(&url).spawn() {
                    Ok(_) => self.push_log("Im Browser das Konto auswählen ..."),
                    Err(e) => self.push_log(format!("Browser ließ sich nicht öffnen ({e}). Diese Seite öffnen: {url}")),
                },
                AuthEvent::Status(msg) => self.push_log(msg),
                AuthEvent::SkinLoaded { uuid, url, rgba, slim } => self.apply_loaded_skin(uuid, url, rgba, slim),
                AuthEvent::HeadProfile { key, textures } => {
                    self.head_profile_cache.insert(key, textures.clone());
                    if let Some(t) = textures {
                        self.head_textures.insert(key, auth::parse_textures_property(&t));
                    }
                    // Das Profil eines gespeicherten Kontos: jetzt seinen Skin laden -- auch bei
                    // geschlossener Liste, der Kopf oben in der Leiste braucht ihn ebenso.
                    if self.accounts.list.iter().any(|a| uuid_bytes_from_hex(&a.uuid) == Some(key)) {
                        self.request_account_heads();
                    }
                }
                // Bei einem Konto die Sperre loesen: sonst bliebe sein Kopf bis zum Neustart
                // schwarz-lila. request_account_heads fragt nach einer Pause erneut (es laeuft
                // jedes Bild, ohne Pause gaebe das eine Flut).
                // Fremde Koepfe bleiben gesperrt -- die fragen jedes Bild, das gaebe eine Flut.
                AuthEvent::HeadProfileFailed { key, meldung } => {
                    self.push_log(meldung);
                    if self.accounts.list.iter().any(|a| uuid_bytes_from_hex(&a.uuid) == Some(key)) {
                        self.head_lookups.remove(&key);
                        self.account_head_retry.insert(key, Instant::now() + ACCOUNT_HEAD_RETRY);
                    }
                }
                AuthEvent::CapeLoaded { uuid, url, rgba } => self.apply_loaded_cape(uuid, url, rgba),
                AuthEvent::ServerPackDownloaded { id, result } => self.server_pack_downloaded(id, result),
                AuthEvent::SkinFailed { url, permanent, meldung } => self.note_skin_failure(url, permanent, meldung),
                AuthEvent::ProfileKey(key) => self.start_chat_session(key),
                AuthEvent::Failed(msg) => {
                    self.login_busy = false;
                    self.account_switching = None;
                    self.push_log(format!("Login fehlgeschlagen: {msg}"));
                    self.notify(notifications::Notification::error("Login fehlgeschlagen", msg));
                }
                AuthEvent::LoggedIn { credentials, msa, source } => {
                    self.login_busy = false;
                    self.account_switching = None;
                    self.input_field = None;
                    self.push_log(format!("Angemeldet als {}", credentials.username));
                    self.notify(notifications::Notification::success("Angemeldet", format!("als {}", credentials.username)));
                    self.accounts.upsert(&credentials, msa, source);
                    self.account_checks.insert(accounts::key(&credentials.profile_id), (accounts::Check::Valid, Instant::now()));
                    self.persist_accounts();
                    self.set_active_account(credentials);
                }
                AuthEvent::AccountChecked { key, result } => {
                    let check = match result {
                        Ok((creds, msa)) => {
                            // Inzwischen vergessen? Dann nicht wieder anlegen.
                            if let Some(i) = self.accounts.find(&creds.profile_id) {
                                let source = self.accounts.list[i].source;
                                self.accounts.upsert(&creds, msa, source);
                                self.persist_accounts();
                                if self.credentials.as_ref().is_some_and(|c| accounts::key(&c.profile_id) == key) {
                                    self.set_active_account(creds);
                                }
                            }
                            accounts::Check::Valid
                        }
                        // Am Anmeldelimit sagt das nichts ueber das Konto: ungeprueft lassen.
                        Err(e) if e.rate_limited() => {
                            self.push_log(e.to_string());
                            self.account_checks.remove(&key);
                            continue;
                        }
                        Err(e) => accounts::Check::Invalid(e.to_string()),
                    };
                    self.account_checks.insert(key, (check, Instant::now()));
                }
            }
        }
    }

    // "Los" auf dem Connect-Screen: Verbindung starten UND sofort in die 3D-Ansicht
    // springen. Man wartet damit nicht mehr im Menue, bis die Verbindung steht -- das
    // Verbindungs-Overlay im World-Modus (build_hud -> build_connecting_overlay) zeigt die
    // Connect-Phase, bis der erste Chunk da ist. screen=Main, damit Esc danach das normale
    // Pausemenue oeffnet (und der Connected-Event nicht mehr umschalten muss).
    pub(crate) fn go_connect(&mut self) {
        // Ohne Konto nur der eigene Rechner (s. net::is_local_address). Hier abfangen, damit
        // man im Menue bleibt und gleich sieht, was fehlt; net::join prueft es noch einmal.
        if self.credentials.is_none() && !net::is_local_address(&self.server_address) {
            self.push_log("Ohne Anmeldung geht nur ein Server auf diesem Rechner (localhost).");
            self.notify(notifications::Notification::error("Nicht angemeldet", "Bitte zuerst mit Microsoft anmelden."));
            return;
        }
        self.column_history.clear();
        self.world_reset_debug = None;
        self.start_connect();
        self.mode = AppMode::World;
        self.screen = MenuScreen::Main;
        self.restore_held_controls();
    }

    // Pausemenue "Reconnect" und die gehaltene Reconnect-Taste: dieselbe Adresse neu
    // verbinden, start_connect baut die alte Verbindung vorher ab.
    pub(crate) fn reconnect(&mut self) {
        if self.server_address.is_empty() {
            return;
        }
        self.push_log("== reconnect (vom Nutzer) ==");
        self.go_connect();
    }

    // Reconnect-Taste: nach der Bestaetigungsdauer neu verbinden. Danach erst wieder, wenn die
    // Taste losgelassen wurde.
    pub(crate) fn tick_reconnect_hold(&mut self) {
        if self.reconnect_needs_release && !self.settings.keybinds.binds(KeyBindAction::Reconnect).any(|b| self.binds_down.contains(&b)) {
            self.reconnect_needs_release = false;
        }
        if self.reconnect_progress().is_some_and(|f| f >= 1.0) {
            self.reconnect_hold = None;
            self.reconnect_needs_release = true;
            self.reconnect();
        }
    }

    // Wie weit die Reconnect-Taste gehalten ist, 0..1 (1 = loest aus).
    pub(crate) fn reconnect_progress(&self) -> Option<f32> {
        let seit = self.reconnect_hold?.elapsed().as_secs_f32();
        let ms = match self.settings.reconnect_hold_ms {
            m if m < 0 => self.settings.hold_confirm_ms,
            m => m as u32,
        };
        Some(if ms == 0 { 1.0 } else { (seit * 1000.0 / ms as f32).min(1.0) })
    }

    // Das Protokoll kennt kein serverbound Disconnect-Packet: ein Client verlaesst den
    // Server, indem er die TCP-Verbindung schliesst. Genau das passiert hier -- abort()
    // bricht den Task an seinem naechsten await ab (er haengt fast immer im recv()),
    // wodurch der TcpStream fallengelassen und die Verbindung geschlossen wird.
    pub(crate) fn disconnect(&mut self) {
        let Some(task) = self.conn_task.take() else {
            return;
        };
        task.abort();
        self.keep_last_view();
        // Der abgebrochene Task kommt nicht mehr dazu, ein Disconnected-Event zu senden --
        // den Status setzen wir daher hier selbst.
        self.conn_status = ConnStatus::Disconnected;
        self.reset_world();
        // Server-Resourcepacks gehoeren zur Verbindung (s. server_packs).
        self.server_packs.clear();
        // Zurueck auf die Server-Liste -- egal, ob man aus dem Pausemenue oder dem
        // Gestorben-Screen getrennt hat.
        self.mode = AppMode::Menu;
        self.screen = MenuScreen::Connect;
        self.push_log("== getrennt (vom Nutzer) ==");
    }
}
