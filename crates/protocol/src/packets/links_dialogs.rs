// Serverlinks, Dialoge, Verhaltenskodex und Features.

use super::*;

// Beschriftung eines Server-Links: einer der festen Typen (KnownLinkType, Index 0..9) oder
// ein eigener Text-Component.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub enum ServerLinkLabel {
    Known(i32),
    Custom(nbt::NbtValue),
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct ServerLink {
    pub label: ServerLinkLabel,
    pub url: String,
}

// Die Namen der festen Typen, in Reihenfolge ihrer IDs (Sprachschluessel
// "known_server_link.<name>").
pub const KNOWN_SERVER_LINKS: [&str; 10] =
    ["report_bug", "community_guidelines", "support", "status", "feedback", "community", "website", "forums", "news", "announcements"];

// Server Links (Config 0x10 / Play 0x87): Liste aus (Either<Typ, Component>, URL). Either
// steht als Bool davor, true = links (fester Typ).
pub fn read_server_links(payload: &[u8]) -> Result<Vec<ServerLink>, ReadError> {
    let mut r = PacketReader::new(payload);
    r.read_varint()?;
    let count = r.read_varint()?.max(0) as usize;
    let mut out = Vec::with_capacity(count.min(64));
    for _ in 0..count {
        let label = if r.read_bool()? {
            ServerLinkLabel::Known(r.read_varint()?)
        } else {
            ServerLinkLabel::Custom(nbt::read_root(&mut r)?.ok_or(ReadError)?)
        };
        let url = r.read_string()?;
        out.push(ServerLink { label, url });
    }
    Ok(out)
}

// Ein Dialog, wie ihn der Server schickt: direkt als NBT, oder (nur im Play-State) als
// Verweis in die Dialog-Registry aus der Konfiguration.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub enum DialogRef {
    Inline(nbt::NbtValue),
    Registry(i32),
}

// Show Dialog im Configuration-State: dort gibt es noch keine Registries, der Dialog steht
// immer direkt als NBT da (Dialog.CONTEXT_FREE_STREAM_CODEC).
pub fn read_show_dialog_config(payload: &[u8]) -> Result<DialogRef, ReadError> {
    let mut r = PacketReader::new(payload);
    r.read_varint()?;
    Ok(DialogRef::Inline(nbt::read_root(&mut r)?.ok_or(ReadError)?))
}

// Show Dialog im Play-State: ByteBufCodecs.holder -- VarInt 0 = direkt (NBT folgt), sonst
// Registry-Index + 1.
pub fn read_show_dialog_play(payload: &[u8]) -> Result<DialogRef, ReadError> {
    let mut r = PacketReader::new(payload);
    r.read_varint()?;
    match r.read_varint()? {
        0 => Ok(DialogRef::Inline(nbt::read_root(&mut r)?.ok_or(ReadError)?)),
        n => Ok(DialogRef::Registry(n - 1)),
    }
}

// Code of Conduct (Config 0x13): ein einziger String, der Text des Kodex.
pub fn read_code_of_conduct(payload: &[u8]) -> Result<String, ReadError> {
    let mut r = PacketReader::new(payload);
    r.read_varint()?;
    r.read_string()
}

// Update Enabled Features (Config 0x0C): Liste von Feature-Flag-IDs ("minecraft:vanilla").
pub fn read_enabled_features(payload: &[u8]) -> Result<Vec<String>, ReadError> {
    let mut r = PacketReader::new(payload);
    r.read_varint()?;
    let count = r.read_varint()?.max(0) as usize;
    let mut out = Vec::with_capacity(count.min(64));
    for _ in 0..count {
        out.push(r.read_string()?);
    }
    Ok(out)
}

// Accept Code of Conduct (Config 0x09): ohne Inhalt.
#[must_use] 
pub fn accept_code_of_conduct_payload() -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_CONFIG_ACCEPT_CODE_OF_CONDUCT);
    w.into_bytes()
}

// Custom Click Action (Play 0x41, Config 0x08): Identifier, dann die Nutzlast als optionales
// NBT-Tag (TAG_End = keine), das Ganze mit VarInt-Laenge davor (lengthPrefixed(65536)).
// Geschrieben wird immer mit der Play-ID; im Configuration-State tauscht
// config_outgoing_payload sie aus.
#[must_use] 
pub fn custom_click_action_payload(id: &str, payload: Option<&nbt::NbtValue>) -> Vec<u8> {
    let mut tag = PacketWriter::new();
    match payload {
        Some(v) => nbt::write_root(&mut tag, v),
        None => {
            tag.write_u8(0);
        }
    }
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_CUSTOM_CLICK_ACTION).write_string(id).write_bytes(&tag.into_bytes());
    w.into_bytes()
}

// Was die App waehrend der Konfiguration schicken darf, in Config-IDs: Resourcepack-Status
// und Klick-Aktionen kommen mit ihrer Play-ID und werden umgeschrieben, die Annahme des
// Verhaltenskodex gibt es nur hier und kommt schon fertig. Alles andere: None.
#[must_use] 
pub fn config_outgoing_payload(play_payload: &[u8]) -> Option<Vec<u8>> {
    if let Some(status) = config_resource_pack_status(play_payload) {
        return Some(status);
    }
    if play_payload == accept_code_of_conduct_payload().as_slice() {
        return Some(play_payload.to_vec());
    }
    let mut r = PacketReader::new(play_payload);
    if r.read_varint().ok()? == SB_PLAY_CUSTOM_CLICK_ACTION {
        let mut w = PacketWriter::new();
        w.write_varint(SB_CONFIG_CUSTOM_CLICK_ACTION).write_raw_bytes(r.remaining());
        return Some(w.into_bytes());
    }
    None
}

#[cfg(test)]
pub(crate) mod dialog_packet_tests {
    use super::*;

    #[test]
    fn server_links_fester_typ_und_eigener_text() {
        let mut w = PacketWriter::new();
        w.write_varint(CB_PLAY_SERVER_LINKS).write_varint(2);
        w.write_bool(true).write_varint(6).write_string("https://example.org");
        w.write_bool(false);
        nbt::write_root(&mut w, &nbt::NbtValue::String("Shop".into()));
        w.write_string("https://shop.example.org");
        let links = read_server_links(&w.into_bytes()).unwrap();
        assert_eq!(links[0], ServerLink { label: ServerLinkLabel::Known(6), url: "https://example.org".into() });
        assert_eq!(links[1].label, ServerLinkLabel::Custom(nbt::NbtValue::String("Shop".into())));
    }

    #[test]
    fn dialog_im_spiel_direkt_oder_als_verweis() {
        let mut w = PacketWriter::new();
        w.write_varint(CB_PLAY_SHOW_DIALOG).write_varint(3);
        assert_eq!(read_show_dialog_play(&w.into_bytes()).unwrap(), DialogRef::Registry(2));
        let mut w = PacketWriter::new();
        w.write_varint(CB_PLAY_SHOW_DIALOG).write_varint(0);
        nbt::write_root(&mut w, &nbt::NbtValue::String("x".into()));
        assert_eq!(read_show_dialog_play(&w.into_bytes()).unwrap(), DialogRef::Inline(nbt::NbtValue::String("x".into())));
    }

    #[test]
    fn klick_aktion_bekommt_in_der_konfiguration_ihre_config_id() {
        let mut map = std::collections::HashMap::new();
        map.insert("name".to_string(), nbt::NbtValue::String("Steve".into()));
        let play = custom_click_action_payload("test:ok", Some(&nbt::NbtValue::Compound(map)));
        let config = config_outgoing_payload(&play).unwrap();
        assert_eq!(i32::from(config[0]), SB_CONFIG_CUSTOM_CLICK_ACTION);
        assert_eq!(&config[1..], &play[1..]);
        let mut r = PacketReader::new(&play);
        r.read_varint().unwrap();
        assert_eq!(r.read_string().unwrap(), "test:ok");
        let laenge = r.read_varint().unwrap() as usize;
        assert_eq!(laenge, r.remaining().len());
        let tag = nbt::read_root(&mut r).unwrap().unwrap();
        assert_eq!(tag.get("name").and_then(|v| v.as_str()), Some("Steve"));
    }

    #[test]
    fn verhaltenskodex_wird_durchgereicht() {
        assert_eq!(config_outgoing_payload(&accept_code_of_conduct_payload()), Some(vec![SB_CONFIG_ACCEPT_CODE_OF_CONDUCT as u8]));
        assert!(config_outgoing_payload(&custom_payload("x", &[])).is_none());
    }
}
