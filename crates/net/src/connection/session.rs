// Token-Erneuerung, Ping und Wiedergabe.

use super::*;

// Vollstaendiger Join: Login -> Configuration -> Play, dann Verbindung offen halten.
// Laeuft bis zum Disconnect -- vom Aufrufer als Hintergrund-Task zu spawnen, nicht
// blockierend (sonst friert die Render-Loop ein). Status/Log geht per Channel an die App.
// `credentials` = echtes Microsoft-/Minecraft-Konto (fuer Online-Mode-Server). None = wie
// bisher Offline-Mode mit `offline_username`.
pub async fn join(
    address: &str,
    offline_username: &str,
    credentials: Option<auth::Credentials>,
    events: Events,
    outgoing: Outgoing,
    rotation: SharedRotation,
) -> Result<(), NetError> {
    join_refreshing(address, offline_username, credentials, None, events, outgoing, rotation).await
}

// Holt frische Credentials (Token erneuern), s. join_refreshing.
pub type TokenRefresh = std::pin::Pin<Box<dyn std::future::Future<Output = Result<auth::Credentials, String>> + Send>>;

// Wie join, aber lehnt der Session-Server das Token ab (meist abgelaufen), wird es einmal mit
// `refresh` erneuert und neu verbunden, statt gleich mit dem 403 zu trennen.
pub async fn join_refreshing(
    address: &str,
    offline_username: &str,
    mut credentials: Option<auth::Credentials>,
    mut refresh: Option<TokenRefresh>,
    events: Events,
    mut outgoing: Outgoing,
    rotation: SharedRotation,
) -> Result<(), NetError> {
    let result = loop {
        let result = join_inner(address, offline_username, credentials.as_ref(), &events, &mut outgoing, &rotation).await;
        let Err(NetError::SessionRejected(_)) = &result else { break result };
        let Some(erneuern) = refresh.take() else { break result };
        log(&events, "Session-Server lehnt das Token ab -- erneuere es ...");
        match erneuern.await {
            Ok(c) => credentials = Some(c),
            Err(e) => break Err(NetError::Protocol(format!("Token abgelehnt, Erneuern fehlgeschlagen: {e}"))),
        }
    };
    if let Err(e) = &result {
        let _ = events.send(ClientEvent::Disconnected(e.to_string()));
    }
    result
}

pub(super) async fn join_inner(
    address: &str,
    offline_username: &str,
    credentials: Option<&auth::Credentials>,
    events: &Events,
    outgoing: &mut Outgoing,
    rotation: &SharedRotation,
) -> Result<(), NetError> {
    let (typed_host, typed_port) = parse_address(address);
    let (mut host, mut port) = resolve_address(address).await;
    // Der Name, den man eingegeben hat (nach einem Transfer das Ziel): weicht der verbundene
    // Host davon ab, kam er aus einem SRV-Eintrag.
    let mut eingegeben = typed_host.clone();
    // Die Weiterleitung im Log sichtbar machen: sonst wundert man sich bei einem Fehler
    // ueber einen Port/Host, den man nie eingegeben hat.
    if (host.as_str(), port) != (typed_host.as_str(), typed_port) {
        log(events, format!("SRV-Eintrag fuer {typed_host}: -> {host}:{port}"));
    }
    let mut intent = packets::INTENT_LOGIN;
    // Die Cookies wandern bei einem Transfer mit (Vanillas TransferState) -- genau darueber
    // erkennt der Zielserver, dass wir vom Netzwerk geschickt wurden.
    let mut cookies = std::collections::HashMap::new();

    // Jeder Durchlauf ist eine Verbindung. Ein Transfer beendet sie mit Ok(Ziel), dann geht
    // es mit einer neuen weiter; alles andere beendet die Sitzung.
    loop {
        // Auch hinter SRV-Eintrag und Transfer pruefen: sonst koennte ein lokaler Server
        // einen Offline-Client einfach an einen fremden weiterreichen.
        if credentials.is_none() && !is_local_address(&host) {
            return Err(NetError::Protocol(format!("{host} ist kein Server auf diesem Rechner -- ohne Anmeldung nicht erlaubt")));
        }
        log(events, format!("Verbinde zu {host}:{port} ..."));
        let mut conn = Connection::connect(&host, port).await?;
        conn.cookies = std::mem::take(&mut cookies);
        if let Some(gegenueber) = conn.peer {
            let ziel = if host == eingegeben { gegenueber.to_string() } else { format!("{host}/{gegenueber}") };
            let _ = events.send(ClientEvent::Route(ziel));
        }

        // Der Mitschnitt wird NUR bei einem Abbruch mit Fehler geschrieben. Ein normales
        // Trennen durch den Nutzer endet mit Ok und soll kein /tmp vollschreiben. Der Pfad
        // geht ins Client-Log, sonst muesste man raten, wo die Datei gelandet ist.
        let result = join_session(&mut conn, &host, port, intent, offline_username, credentials, events, outgoing, rotation).await;
        let (ziel_host, ziel_port) = match result {
            Ok(ziel) => ziel,
            Err(e) => {
                match conn.log.dump(&e.to_string()) {
                    Some(path) => log(events, format!("Paket-Mitschnitt der letzten Pakete: {path}")),
                    None => log(events, "Kein Paket-Mitschnitt (nichts aufgezeichnet)"),
                }
                return Err(e);
            }
        };
        log(events, format!("Server schickt uns weiter: {ziel_host}:{ziel_port}"));
        cookies = std::mem::take(&mut conn.cookies);
        // Erst die alte Verbindung schliessen, dann melden: der Zielserver erwartet uns nicht
        // doppelt angemeldet.
        drop(conn);
        let _ = events.send(ClientEvent::Transferring { host: ziel_host.clone(), port: ziel_port });
        // Wie Vanilla (ConnectScreen ueber ServerAddress): auch das Transfer-Ziel geht durch
        // die SRV-Aufloesung, und der Handshake traegt die aufgeloeste Adresse.
        eingegeben = ziel_host.clone();
        (host, port) = resolve_srv(&ziel_host, ziel_port).await.unwrap_or((ziel_host, ziel_port));
        intent = packets::INTENT_TRANSFER;
    }
}

// Der eigentliche Sitzungsablauf ab bestehender TCP-Verbindung. Getrennt von join_inner nur,
// damit dort ein `?` nicht am Mitschnitt-Dump vorbeispringt -- jeder Fehler von hier muss
// durch die Fehlerbehandlung des Aufrufers. Ok heisst: der Server hat uns weitergeschickt.
#[allow(clippy::too_many_arguments)]
pub(super) async fn join_session(
    conn: &mut Connection,
    host: &str,
    port: u16,
    intent: i32,
    offline_username: &str,
    credentials: Option<&auth::Credentials>,
    events: &Events,
    outgoing: &mut Outgoing,
    rotation: &SharedRotation,
) -> Result<TransferTarget, NetError> {
    let login = do_login(conn, host, port, intent, offline_username, credentials).await?;
    log(events, format!("Login erfolgreich: {}", login.username));
    let _ = events.send(ClientEvent::OwnUuid(login.uuid.to_bytes()));

    conn.send(&packets::login_acknowledged_payload()).await?;
    // coral-record ab hier: Login und Verschluesselungs-Handshake bleiben draussen, die
    // Configuration (Registries, Tags) braucht die Wiedergabe.
    let kopf = record::Header { protocol: packets::PROTOCOL_VERSION, client: String::new(), server: format!("{host}:{port}"), wall_ms: 0.0, uuid: login.uuid.to_bytes(), name: login.username.clone(), from_join: true };
    let melder = events.clone();
    let mitte = Box::new(move |id| {
        let _ = melder.send(ClientEvent::RecordingStarted(id));
    });
    let mut rec = record::PacketRecorder::new(kopf, events.record.clone()).on_mid_start(mitte);
    if let Some(link) = events.clip.clone() {
        let melder = events.clone();
        rec = rec.with_clip(
            link,
            Box::new(move |e| {
                let _ = melder.send(match e {
                    record::ClipEvent::Started { session, from_join, t_ms } => ClientEvent::ClipStarted { session, from_join, t_ms },
                    record::ClipEvent::Checkpoint { session, seq, t_ms } => ClientEvent::ClipCheckpoint { session, seq, t_ms },
                });
            }),
        );
    }
    conn.rec = Some(rec);
    log(events, "Konfiguration ...");
    if let Some(ziel) = do_configuration(conn, events, outgoing).await? {
        return Ok(ziel);
    }
    // Bewegungspakete kommen erst im Play-State zum Zug -- waehrend Login/Configuration
    // haelt run_play_loop den outgoing-Kanal noch nicht offen, die App schickt aber ohnehin
    // erst etwas, sobald sie den Play-Zustand ueber ClientEvent::Connected erfaehrt.
    run_play_loop(conn, events, outgoing, rotation, Vec::new()).await
}

// Spielt eine Aufnahme (crates/record) ab: dieselbe Configuration und derselbe Play-Loop wie
// live, nur kommen die Pakete aus der Datei zur aufgezeichneten Zeit (s. Connection::replay).
// Die Uhr teilt die App, sie pausiert und spult damit. Laeuft, bis die App den Task abbricht.
pub async fn replay(path: std::path::PathBuf, clock: std::sync::Arc<record::ReplayClock>, events: Events, mut outgoing: Outgoing, rotation: SharedRotation) -> Result<(), NetError> {
    let ergebnis = replay_inner(&path, clock, &events, &mut outgoing, &rotation).await;
    if let Err(e) = &ergebnis {
        let _ = events.send(ClientEvent::Disconnected(e.to_string()));
    }
    ergebnis
}

pub(super) async fn replay_inner(path: &std::path::Path, clock: std::sync::Arc<record::ReplayClock>, events: &Events, outgoing: &mut Outgoing, rotation: &SharedRotation) -> Result<(), NetError> {
    let mut quelle = record::PacketSource::open(path, clock).map_err(|e| NetError::Protocol(format!("Aufnahme nicht lesbar: {e}")))?;
    let kopf = quelle.header().clone();
    let snapshot = quelle.take_snapshot();
    if !kopf.from_join && snapshot.is_empty() {
        return Err(NetError::Protocol("Diese Aufnahme beginnt mitten im Spiel und hat keinen Anfangszustand".into()));
    }
    log(events, format!("Wiedergabe: {} ({}, {})", path.display(), kopf.server, kopf.name));
    let mut conn = Connection::replay(quelle);
    let _ = events.send(ClientEvent::OwnUuid(kopf.uuid));
    // Ab der Mitte steht die Configuration im Snapshot, die Pakete beginnen im Play-State.
    if kopf.from_join && do_configuration(&mut conn, events, outgoing).await?.is_some() {
        return Ok(());
    }
    run_play_loop(&mut conn, events, outgoing, rotation, snapshot).await.map(|_| ())
}
