use super::*;

#[test]
// Ein Paket, das NUR das Benutzen-Flag traegt, muss durchkommen -- genau das schickt der
// Server, wenn jemand sein Schild hebt.
fn ein_reines_benutzen_flag_gilt_als_anzeigerelevant() {
    let nur_flag = packets::EntityDataUpdate { living_flags: Some(1), ..Default::default() };
    assert!(meta_betrifft_anzeige(&nur_flag));
    // Gegenprobe: ein Strom ohne jedes Anzeigefeld wird weiterhin verworfen.
    let nur_luft = packets::EntityDataUpdate { air: Some(300), ..Default::default() };
    assert!(!meta_betrifft_anzeige(&nur_luft));
}

// Liegende Items tragen ihren Stack allein im Paket -- ohne ihn bleiben sie unsichtbar.
// Dasselbe fuer den Schuetzen einer Feuerwerksrakete (Elytra-Schub).
#[test]
fn item_stapel_und_raketenschuetze_kommen_durch() {
    let stapel = packets::EntityDataUpdate { item_stack: Some(None), ..Default::default() };
    assert!(meta_betrifft_anzeige(&stapel));
    let schuetze = packets::EntityDataUpdate { firework_shooter: Some(Some(7)), ..Default::default() };
    assert!(meta_betrifft_anzeige(&schuetze));
}

#[test]
fn unbekannte_ids_werden_einmal_und_gedeckelt_gemerkt() {
    let mut s = PlayStats::default();
    assert!(s.note_unknown_id(0x77), "erste Sichtung ist neu");
    assert!(!s.note_unknown_id(0x77), "zweite Sichtung derselben ID nicht noch einmal");
    for id in 0..100 {
        s.note_unknown_id(id);
    }
    assert_eq!(s.unknown_ids.len(), PlayStats::MAX_UNKNOWN_IDS, "Liste darf nicht unbegrenzt wachsen");
}

#[test]
fn stall_bericht_nennt_die_entscheidenden_zahlen() {
    // Der Bericht ist die einzige Spur, die ein Haenger hinterlaesst -- er muss ohne
    // Nachfragen unterscheidbar machen, ob der Server schweigt, leere Stapel schickt
    // oder wir die Chunk-Pakete nicht erkennen.
    let mut s = PlayStats { packets: 412, batches: 1, last_batch_size: 2, acks_sent: 3, ..Default::default() };
    s.note_unknown_id(0x2C);
    let bericht = s.chunk_stall_report(std::time::Duration::from_secs(6));
    assert!(bericht.contains("412 Pakete"), "{bericht}");
    assert!(bericht.contains("1 Stapel (zuletzt 2 Chunks)"), "{bericht}");
    assert!(bericht.contains("3 Bestaetigungen"), "{bericht}");
    assert!(bericht.contains("0x2C"), "{bericht}");
}

#[test]
fn stall_bericht_ohne_unbekannte_ids_sagt_das_ausdruecklich() {
    let bericht = PlayStats::default().chunk_stall_report(std::time::Duration::from_secs(6));
    assert!(bericht.contains("unbekannte IDs: keine"), "{bericht}");
}

#[test]
fn parse_address_ergaenzt_den_standardport() {
    assert_eq!(parse_address("derantonia.de"), ("derantonia.de".to_string(), 25565));
    assert_eq!(parse_address("derantonia.de:25566"), ("derantonia.de".to_string(), 25566));
    // Kein gueltiger Port hinter dem Doppelpunkt -> alles ist der Host (so wie bisher).
    assert_eq!(parse_address("beispiel:abc"), ("beispiel:abc".to_string(), 25565));
}

#[test]
fn ohne_anmeldung_nur_server_auf_diesem_rechner() {
    for lokal in ["localhost", "LocalHost:25566", "127.0.0.1", "127.0.0.1:25565", "127.1.2.3", "[::1]:25565"] {
        assert!(is_local_address(lokal), "{lokal}");
    }
    for fremd in ["derantonia.de", "192.168.0.10", "10.0.0.1:25565", "0.0.0.0", "localhost.example.com", "hypixel.net"] {
        assert!(!is_local_address(fremd), "{fremd}");
    }
}

// Baut einen komprimierten Frame so, wie ein Server ihn schickt: VarInt mit der
// ENTPACKTEN Laenge, dann die Zlib-Daten. `behauptete_laenge` darf bewusst von der
// echten abweichen -- genau das ist der Angriffsfall.
fn komprimierter_frame(nutzdaten: &[u8], behauptete_laenge: i32) -> Vec<u8> {
    use std::io::Write;
    let mut frame = encode_varint(behauptete_laenge);
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
    enc.write_all(nutzdaten).unwrap();
    frame.extend(enc.finish().unwrap());
    frame
}

// Der Normalfall muss unveraendert durchlaufen -- sonst haette der Deckel echte Server
// mitgesperrt statt nur die unsinnigen Laengen.
#[test]
fn ehrlicher_komprimierter_frame_kommt_unveraendert_an() {
    let nutzdaten = vec![7u8; 5000];
    let frame = komprimierter_frame(&nutzdaten, nutzdaten.len() as i32);
    assert_eq!(decompress_frame(Some(256), frame).unwrap(), nutzdaten);
}

// Eine negative Laenge ist der schlimmste Fall: `-1 as usize` waere usize::MAX und damit
// eine sofortige Allokation ueber den gesamten Adressraum -- der Prozess waere weg.
// Muss ein normaler Protokollfehler sein, den der Aufrufer als Verbindungsabbruch sieht.
#[test]
fn negative_entpackte_laenge_wird_abgelehnt() {
    let frame = komprimierter_frame(b"egal", -1);
    assert!(matches!(decompress_frame(Some(256), frame), Err(NetError::Protocol(_))));
}

// Dekompressions-Bombe: der Server behauptet eine riesige entpackte Groesse. Ohne Deckel
// wuerde vec![0u8; ...] das sofort anfordern, BEVOR ueberhaupt entpackt wird.
#[test]
fn viel_zu_grosse_entpackte_laenge_wird_abgelehnt() {
    let frame = komprimierter_frame(b"egal", MAX_UNCOMPRESSED_LEN + 1);
    assert!(matches!(decompress_frame(Some(256), frame), Err(NetError::Protocol(_))));
    // Genau auf der Grenze darf es NICHT am Deckel scheitern (sondern hoechstens daran,
    // dass die Zlib-Daten kuerzer sind) -- sonst waere der Deckel um eins zu streng.
    let grenzfall = komprimierter_frame(b"egal", MAX_UNCOMPRESSED_LEN);
    assert!(!matches!(decompress_frame(Some(256), grenzfall), Err(NetError::Protocol(_))));
}

// Gelogene Laenge nach unten: entpackt kommen mehr Bytes heraus als angekuendigt. Ohne
// die Nachpruefung haetten wir stillschweigend ein abgeschnittenes Paket weitergereicht.
#[test]
fn zu_klein_angegebene_laenge_wird_erkannt() {
    let frame = komprimierter_frame(&[9u8; 100], 10);
    assert!(matches!(decompress_frame(Some(256), frame), Err(NetError::Protocol(_))));
}

// Ohne ausgehandelte Kompression bleibt der Frame unangetastet -- der Deckel darf sich
// in diesen Pfad nicht einmischen.
#[test]
fn ohne_kompression_bleibt_der_frame_roh() {
    let roh = vec![1u8, 2, 3];
    assert_eq!(decompress_frame(None, roh.clone()).unwrap(), roh);
    // Nur die Null fuer "unkomprimiert": leer (recv ueberspringt es, wie Vanilla).
    assert!(decompress_frame(Some(256), vec![0]).unwrap().is_empty());
}

// Frame-Laengen ueber Vanillas Varint21-Grenze muessen sofort auffallen, statt rbuf
// unbegrenzt weiterwachsen zu lassen (s. MAX_FRAME_LEN).
#[test]
fn zu_lange_frame_laenge_wird_abgelehnt() {
    let zu_lang = encode_varint(MAX_FRAME_LEN as i32 + 1);
    assert!(matches!(peek_frame_len(&zu_lang), Err(NetError::Protocol(_))));
    // Genau auf der Grenze noch erlaubt.
    let grenzfall = encode_varint(MAX_FRAME_LEN as i32);
    assert_eq!(peek_frame_len(&grenzfall).unwrap().map(|(len, _)| len), Some(MAX_FRAME_LEN));
}

// Der SRV-Umweg gilt NUR fuer den Standardport (wie Vanillas RedirectResolver): wer
// "host:25566" eintippt, meint genau diesen Port und darf nicht umgeleitet werden.
// Ohne Netz pruefbar, weil die Abfrage vor jedem DNS-Zugriff abbricht.
#[tokio::test]
async fn srv_wird_bei_explizitem_port_uebersprungen() {
    assert_eq!(resolve_srv("derantonia.de", 25566).await, None);
    assert_eq!(resolve_address("derantonia.de:25566").await, ("derantonia.de".to_string(), 25566));
}
