// Frames dekomprimieren, Upstream-Verbindung, Metadaten-Filter und Cookies.

use super::*;

// Wendet (falls aktiv) die Zlib-Dekompression auf einen rohen Frame an -- identisch zur
// frueheren recv_inner-Logik, nur vom Framing entkoppelt. Bewusst eine FREIE Funktion statt
// einer Methode: sie braucht von der Verbindung nur das Kompressions-Flag, und ohne `&self`
// laesst sich die Laengenpruefung (s. MAX_UNCOMPRESSED_LEN) ohne echten TCP-Socket testen.
pub(super) fn decompress_frame(compression: Option<i32>, raw: Vec<u8>) -> Result<Vec<u8>, NetError> {
    if compression.is_none() {
        return Ok(raw);
    }
    let (data_len, offset) = decode_varint(&raw, 0).map_err(|_| NetError::Protocol("ungueltiger VarInt".into()))?;
    let rest = &raw[offset..];
    if data_len == 0 {
        return Ok(rest.to_vec()); // Unter dem Threshold: unkomprimiert
    }
    // Vor der Allokation pruefen, nicht danach -- genau darin liegt der Angriff (s.
    // MAX_UNCOMPRESSED_LEN). Faengt negative Laengen gleich mit ab.
    if !(1..=MAX_UNCOMPRESSED_LEN).contains(&data_len) {
        return Err(NetError::Protocol(format!("unplausible entpackte Paketlaenge: {data_len}")));
    }
    let mut decoder = ZlibDecoder::new(rest);
    let mut out = vec![0u8; data_len as usize];
    io::Read::read_exact(&mut decoder, &mut out).map_err(NetError::Io)?;
    // read_exact fuellt genau data_len Bytes -- entpackt der Strom MEHR, war die
    // Laengenangabe gelogen. Stillschweigend abschneiden ergaebe ein halbes Paket, das der
    // Parser danach als Muell sieht; hier ist die Ursache noch benennbar.
    if io::Read::read(&mut decoder, &mut [0u8; 1]).map_err(NetError::Io)? != 0 {
        return Err(NetError::Protocol("entpacktes Paket laenger als angegeben".into()));
    }
    Ok(out)
}

// Liest die Frame-Laenge (VarInt) am Anfang von `buf`, ohne etwas zu verbrauchen. Ok(None) =
// der VarInt ist selbst noch nicht vollstaendig da (mehr Bytes noetig). Gibt (Laenge,
// VarInt-Bytelaenge) zurueck.
pub(super) fn peek_frame_len(buf: &[u8]) -> Result<Option<(usize, usize)>, NetError> {
    let mut value: u32 = 0;
    for i in 0..5 {
        let Some(&byte) = buf.get(i) else {
            return Ok(None);
        };
        value |= u32::from(byte & 0x7F) << (7 * i);
        if byte & 0x80 == 0 {
            let len = value as usize;
            if len > MAX_FRAME_LEN {
                return Err(NetError::Protocol(format!("Frame-Laenge {len} ueber dem Limit {MAX_FRAME_LEN}")));
            }
            return Ok(Some((len, i + 1)));
        }
    }
    Err(NetError::Protocol("Laengen-VarInt zu lang".into()))
}

pub(super) async fn send_handshake(conn: &mut Connection, host: &str, port: u16, intent: i32) -> Result<(), NetError> {
    let handshake = Handshake { protocol_version: packets::PROTOCOL_VERSION, server_address: host.to_string(), server_port: port, intent };
    conn.send(&handshake.to_payload()).await
}

// Server-Status-Ping (Meilenstein 1): Handshake -> Status Request -> Status Response.
pub async fn ping(address: &str) -> Result<String, NetError> {
    let (host, port) = resolve_address(address).await;
    let mut conn = Connection::connect(&host, port).await?;

    send_handshake(&mut conn, &host, port, packets::INTENT_STATUS).await?;
    conn.send(&packets::status_request_payload()).await?;

    let payload = conn.recv().await?;
    let response = StatusResponse::from_payload(&payload).map_err(|_| NetError::Protocol("ungueltige Status-Response".into()))?;
    Ok(response.json)
}

// Fuehrt Handshake + Login Start + Login-Loop aus, bis Login Success erreicht ist. Mit
// `credentials` (echtes Konto) laeuft der Online-Mode-Handshake inkl. Verschluesselung; ohne
// (None) der bisherige Offline-Mode mit frei waehlbarem Namen + Zufalls-UUID.
// --- Eingeloggte Upstream-Verbindung fuer den Diagnose-Proxy ----------------------------
//
// Gegen einen Online-Mode-Server ist ein reiner Byte-Weiterleiter blind: ab der
// Encryption-Response laeuft der ganze Strom durch AES-128-CFB8, der Proxy saehe nur
// Chiffrat. Damit der Proxy trotzdem mitlesen kann, meldet er sich SELBST an (mit denselben
// Zugangsdaten wie der Client) und spricht zum echten Vanilla-Client unverschluesselt --
// das ist genau die Aufteilung, die pktproxy im MITM-Modus braucht.
//
// Nach aussen gibt es hier nur Payloads: entschluesselt und entpackt. Framing, Kompression
// und Verschluesselung bleiben in Connection.
pub struct UpstreamLink {
    pub(super) conn: Connection,
    pub username: String,
    pub uuid: protocol::uuid::Uuid,
    // Der Schwellwert, den der Server uns gesetzt hat. Der Proxy reicht GENAU denselben an
    // seinen Client weiter, dann sind beide Seiten formatgleich und er muss nichts umpacken.
    pub compression: Option<i32>,
}

impl UpstreamLink {
    // Handshake, Login Start, Verschluesselung und Session-Join -- bis EINSCHLIESSLICH Login
    // Success. Das Login Acknowledged wird bewusst NICHT geschickt: das reicht der Proxy vom
    // echten Client durch, damit die Reihenfolge unangetastet bleibt.
    pub async fn login(address: &str, credentials: &auth::Credentials) -> Result<Self, NetError> {
        let (host, port) = resolve_address(address).await;
        let mut conn = Connection::connect(&host, port).await?;
        let login = do_login(&mut conn, &host, port, packets::INTENT_LOGIN, &credentials.username, Some(credentials)).await?;
        let compression = conn.compression;
        Ok(Self { conn, username: login.username, uuid: login.uuid, compression })
    }

    pub async fn send(&mut self, payload: &[u8]) -> Result<(), NetError> {
        self.conn.send(payload).await
    }

    pub async fn recv(&mut self) -> Result<Vec<u8>, NetError> {
        self.conn.recv().await
    }
}

pub(super) async fn do_login(
    conn: &mut Connection,
    host: &str,
    port: u16,
    // INTENT_LOGIN beim normalen Verbinden, INTENT_TRANSFER nach einem Transfer-Paket.
    intent: i32,
    offline_username: &str,
    credentials: Option<&auth::Credentials>,
) -> Result<LoginResult, NetError> {
    send_handshake(conn, host, port, intent).await?;

    // Online: echter Name + echte Konto-UUID. Offline: frei waehlbarer Name + Zufalls-UUID
    // (der Server leitet die UUID dort ohnehin deterministisch aus dem Namen ab).
    let (name, uuid) = match credentials {
        Some(c) => {
            let uuid =
                Uuid::from_hex(&c.profile_id).ok_or_else(|| NetError::Protocol("ungueltige Profil-UUID in den Credentials".into()))?;
            (c.username.clone(), uuid)
        }
        None => (offline_username.to_string(), Uuid::random_v4()),
    };
    let login_start = LoginStart { name, uuid };
    conn.send(&login_start.to_payload()).await?;

    loop {
        let payload = conn.recv().await?;
        let id = packets::peek_packet_id(&payload).map_err(|_| NetError::Protocol("leeres Packet".into()))?;
        match id {
            0x00 => {
                let d = LoginDisconnect::from_payload(&payload).map_err(|_| NetError::Protocol("ungueltiges Disconnect-Packet".into()))?;
                // Der Login-State schickt JSON, kein NBT -- ausgepackt steht hier die
                // Meldung des Servers statt einer rohen Zeichenkette voller Stil-Felder
                // (etwa "You are already online!", das sonst als JSON im Log stand). Die
                // Farben reisen als Paragraphen-Codes mit (s. read_disconnect_reason).
                let spans = protocol::nbt::json_text_to_spans(&d.reason_json);
                let leer = spans.iter().all(|s| s.text.trim().is_empty());
                let grund = if leer { d.reason_json } else { protocol::nbt::spans_to_legacy(&spans) };
                return Err(NetError::Protocol(format!("Server hat Login abgelehnt: {grund}")));
            }
            0x01 => {
                // EncryptionRequest -- der Server ist im Online-Mode. Ohne Credentials koennen
                // wir den Session-Join nicht durchfuehren; klarer Hinweis statt kryptischem Fehler.
                let creds = credentials.ok_or_else(|| {
                    NetError::Protocol(
                        "Dieser Server ist im Online-Mode -- bitte zuerst mit Microsoft anmelden (oder Token eingeben)".into(),
                    )
                })?;
                do_encryption_handshake(conn, &payload, creds).await?;
            }
            0x02 => {
                let s = LoginSuccess::from_payload(&payload).map_err(|_| NetError::Protocol("ungueltiges Login-Success-Packet".into()))?;
                return Ok(LoginResult { uuid: s.uuid, username: s.username });
            }
            0x03 => {
                let c =
                    SetCompression::from_payload(&payload).map_err(|_| NetError::Protocol("ungueltiges SetCompression-Packet".into()))?;
                conn.compression = (c.threshold >= 0).then_some(c.threshold);
            }
            packets::CB_LOGIN_COOKIE_REQUEST => {
                answer_cookie_request(conn, &payload, packets::SB_LOGIN_COOKIE_RESPONSE).await?;
            }
            other => return Err(NetError::Protocol(format!("unerwartetes Login-Packet: id=0x{other:02x}"))),
        }
    }
}

// Beantwortet EncryptionRequest: Shared Secret wuerfeln, Mojang-Session-Join (damit der Server
// uns beim Nachfragen akzeptiert), Shared Secret + Verify Token RSA-verschluesselt zurueckschicken
// und danach den Bytestrom auf AES-128-CFB8 umschalten. Reihenfolge ist kritisch: Der
// Session-Join MUSS vor der Encryption-Response passieren, und die Verschluesselung wird erst
// NACH dem (noch unverschluesselten) Senden der Response aktiv.
pub(super) async fn do_encryption_handshake(conn: &mut Connection, request_payload: &[u8], credentials: &auth::Credentials) -> Result<(), NetError> {
    let req = packets::EncryptionRequest::from_payload(request_payload)
        .map_err(|_| NetError::Protocol("ungueltiges EncryptionRequest-Packet".into()))?;

    let secret = crate::crypto::random_shared_secret();
    let hash = crate::crypto::server_hash(&req.server_id, &secret, &req.public_key);

    // Bei Mojang anmelden, dass wir diesem Server beitreten (nur wenn der Server es verlangt --
    // in der Praxis immer, wenn er ueberhaupt einen EncryptionRequest schickt).
    if req.should_authenticate {
        auth::mojang_join(&credentials.access_token, &credentials.profile_id, &hash).await.map_err(|e| {
            if e.join_forbidden() {
                NetError::SessionRejected(e.to_string())
            } else {
                NetError::Protocol(e.to_string())
            }
        })?;
    }

    let enc_secret = crate::crypto::rsa_encrypt(&req.public_key, &secret).map_err(NetError::Protocol)?;
    let enc_verify = crate::crypto::rsa_encrypt(&req.public_key, &req.verify_token).map_err(NetError::Protocol)?;

    conn.send(&packets::encryption_response_payload(&enc_secret, &enc_verify)).await?;
    // Ab hier ist der komplette Strom verschluesselt.
    conn.enable_encryption(&secret);
    Ok(())
}

// Steht in diesem Metadaten-Strom ueberhaupt etwas, das die DARSTELLUNG angeht?
//
// Der Filter ist kein Luxus: Positions- und Gesundheits-Updates schicken ihren Strom pro Tick
// durch die Warteschlange, ohne dass sich daran je etwas aendert. Er ist aber genau deshalb
// auch die Stelle, an der ein neu ausgelesenes Feld leicht haengenbleibt -- als eigene
// Funktion ist sie pruefbar, ohne eine Verbindung aufzubauen.
pub(super) const fn meta_betrifft_anzeige(update: &packets::EntityDataUpdate) -> bool {
    update.flags.is_some()
        // Das Benutzen-Flag (Schild hoch, Bogen gespannt, am Essen) aendert sich fuer sich
        // allein -- fehlt es hier, sieht man fremde Schilde nie hochgerissen.
        || update.living_flags.is_some()
        // Die goldenen Herzen des eigenen Spielers.
        || update.absorption.is_some()
        || update.custom_name.is_some()
        || update.name_visible.is_some()
        || update.display_text.is_some()
        || update.display_item.is_some()
        || update.display_block_state.is_some()
        || update.display_translation.is_some()
        || update.display_scale.is_some()
        || update.display_left_rotation.is_some()
        || update.display_right_rotation.is_some()
        // Bodenplatte des End-Kristalls -- fehlte, deshalb stand sie auch bei gesetzten
        // Kristallen da.
        || update.end_crystal_show_bottom.is_some()
        || update.display_billboard.is_some()
        || update.orb_value.is_some()
        || update.display_view_range.is_some()
        || update.text_background.is_some()
        || update.text_opacity.is_some()
        || update.text_flags.is_some()
        // Der Stack liegender Items und Wurfgeschosse -- fehlte, deshalb blieben liegende
        // Items unsichtbar.
        || update.item_stack.is_some()
        // Schuetze einer Feuerwerksrakete: daran haengt der Elytra-Schub.
        || update.firework_shooter.is_some()
        // Steckt der Pfeil? -- fehlte, deshalb fielen steckende Pfeile aus der Wand.
        || update.arrow_in_ground.is_some()
        // Groesse der interaction-Klickflaeche -- fehlte, deshalb liess sich das Tor auf
        // mcpvp.club nicht schlagen.
        || update.interaction_width.is_some()
        || update.interaction_height.is_some()
        // Inhalt, Drehung und Wand eines Gegenstandsrahmens -- fehlten, deshalb blieben Karten
        // und Items in sichtbaren Rahmen leer (dort steht oft nur der Inhalt im Paket).
        || update.frame_item.is_some()
        || update.frame_rotation.is_some()
        || update.hanging_direction.is_some()
}

// Beantwortet einen Cookie Request -- in JEDEM Zustand, nur mit der passenden Antwort-ID.
//
// Wichtig ist das "immer antworten": kennen wir das Cookie nicht, geht trotzdem eine Antwort
// raus, nur eben eine leere. Vanilla macht es genauso, und ein Server, der auf die Antwort
// wartet, haengt sonst bis zu seinem Timeout und trennt die Verbindung -- fuer den Spieler
// ein grundloser Kick, fuer den Server ein Client, der sich totstellt.
pub(super) async fn answer_cookie_request(conn: &mut Connection, payload: &[u8], response_id: i32) -> Result<(), NetError> {
    let key = packets::read_cookie_key(payload).map_err(|_| NetError::Protocol("Cookie Request".into()))?;
    let value = conn.cookies.get(&key).cloned();
    conn.send(&packets::cookie_response_payload(response_id, &key, value.as_deref())).await
}

// Legt ein vom Server geschicktes Cookie ab. Zu grosse verwirft read_store_cookie, wie Vanilla.
pub(super) fn store_cookie(conn: &mut Connection, payload: &[u8]) {
    if let Ok(Some((key, value))) = packets::read_store_cookie(payload) {
        conn.cookies.insert(key, value);
    }
}

#[cfg(test)]
mod meta_filter_neu_tests {
    use super::*;

    #[test]
    fn pfeil_und_interaction_kommen_durch() {
        let leer = packets::EntityDataUpdate::default();
        assert!(!meta_betrifft_anzeige(&leer));
        assert!(meta_betrifft_anzeige(&packets::EntityDataUpdate { arrow_in_ground: Some(true), ..Default::default() }));
        assert!(meta_betrifft_anzeige(&packets::EntityDataUpdate { interaction_width: Some(3.0), ..Default::default() }));
        assert!(meta_betrifft_anzeige(&packets::EntityDataUpdate { interaction_height: Some(3.0), ..Default::default() }));
    }

    #[test]
    fn rahmen_inhalt_kommt_durch() {
        assert!(meta_betrifft_anzeige(&packets::EntityDataUpdate { frame_item: Some(None), ..Default::default() }));
        assert!(meta_betrifft_anzeige(&packets::EntityDataUpdate { frame_rotation: Some(2), ..Default::default() }));
        assert!(meta_betrifft_anzeige(&packets::EntityDataUpdate { hanging_direction: Some(3), ..Default::default() }));
    }
}
