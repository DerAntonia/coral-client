// Systemchat, Textkomponenten, Titel und Erfahrung.

use super::*;

// System Chat (0x77): GameMessageS2CPacket -- Text-Component (Netzwerk-NBT) + bool overlay
// (Actionbar statt normaler Chat). Deckt Server-/Plugin-Broadcasts und Kommandoantworten ab
// (NICHT signierten Spieler-Chat -- das ist ein eigenes Paket, PLAYER_CHAT, hier nicht
// implementiert). ID direkt aus GameProtocols.CLIENTBOUND_TEMPLATE (echtes, Mojang-
// gemapptes Paper-1.21.11-Server-Jar) abgezaehlt -- dieselbe Quelle, die oben acht weitere
// IDs (davon zwei empirisch verifiziert: KEEP_ALIVE=0x2B, CHUNK_DATA=0x2C) exakt bestaetigt.
pub const CB_PLAY_SYSTEM_CHAT: i32 = 0x77;

pub fn read_system_chat(payload: &[u8]) -> Result<(String, bool), ReadError> {
    let (spans, overlay) = read_system_chat_spans(payload)?;
    Ok((spans.iter().map(|s| s.text.as_str()).collect::<String>(), overlay))
}

// Dasselbe Paket MIT seinen Farben. Der Chat ist der Ort, an dem Farbe Bedeutung traegt:
// Teamfarben, Warnungen, Serverpraefixe. Sie beim Lesen wegzuwerfen (wie es
// read_text_component tut) macht aus jeder Nachricht graue Einheitssuppe.
pub fn read_system_chat_spans(payload: &[u8]) -> Result<(Vec<crate::nbt::TextSpan>, bool), ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let spans = read_text_spans(&mut r)?;
    let overlay = r.read_bool()?;
    Ok((spans, overlay))
}

// Opfer und Taeter, wenn die System-Chat-Zeile eine Vanilla-Todesmeldung ist (s.
// nbt::death_message).
pub fn read_system_chat_death(payload: &[u8]) -> Result<Option<(String, Option<String>)>, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    Ok(nbt::read_root(&mut r)?.as_ref().and_then(nbt::death_message))
}

// Ein Text-Component als farbige Abschnitte statt als nackter String.
pub(crate) fn read_text_spans(r: &mut PacketReader) -> Result<Vec<crate::nbt::TextSpan>, ReadError> {
    Ok(nbt::read_root(r)?.as_ref().map(nbt::text_to_spans).unwrap_or_default())
}

// Grund eines Disconnects im Configuration- ODER Play-State. Beide Pakete tragen genau ein
// Text-Component -- dieselbe Kodierung wie System-Chat (anders als der Login-State, der
// noch JSON schickt, s. LoginDisconnect).
//
// Ohne das stand im Log nur "Server-Disconnect", und man musste raten, warum: fehlender
// Resourcepack, Anticheat, Whitelist, Versionskonflikt -- der Server SAGT es, wir haben es
// bloss weggeworfen.
// Der Grund mit seinen Farben als Paragraphen-Codes (s. read_text_component_legacy) -- das
// Disconnect-Fenster zeigt ihn farbig, Log und Addons bekommen den reinen Text.
pub fn read_disconnect_reason(payload: &[u8]) -> Result<String, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    read_text_component_legacy(&mut r)
}

// Ein Text-Component (Netzwerk-NBT, TextCodecs.UNLIMITED_REGISTRY_PACKET_CODEC) mit den
// Farben als Paragraphen-Codes im Text (s. nbt::spans_to_legacy) -- fuer Scoreboard und Teams,
// deren Texte erst im Client zu einer Zeile zusammengesetzt werden (ohne das kam das
// Scoreboard farblos an), und fuer den Disconnect-Grund.
pub(crate) fn read_text_component_legacy(r: &mut PacketReader) -> Result<String, ReadError> {
    Ok(nbt::read_root(r)?.as_ref().map(|v| nbt::spans_to_legacy(&nbt::text_to_spans(v))).unwrap_or_default())
}

// --- Titel / Untertitel / Actionbar (IDs aus PlayStateFactories, gegen 6 bekannte Anker
// verifiziert: ADD_ENTITY 0x01, PLAYER_POSITION 0x46, ROTATE_HEAD 0x51, SET_HEALTH 0x66,
// SYSTEM_CHAT 0x77, TELEPORT_ENTITY 0x7B) --------------------------------------------------

// OverlayMessageS2CPacket: ein Text ueber der Hotbar (Actionbar). Manche Server nutzen dies,
// andere System-Chat mit overlay=true -- der Client behandelt beide gleich.
pub const CB_PLAY_SET_ACTION_BAR_TEXT: i32 = 0x55;
// TitleS2CPacket / SubtitleS2CPacket: je ein Text (gross in der Bildschirmmitte).
pub const CB_PLAY_SET_TITLE_TEXT: i32 = 0x70;
pub const CB_PLAY_SET_SUBTITLE_TEXT: i32 = 0x6E;
// TitleFadeS2CPacket: Einblend-/Halte-/Ausblendzeit in Ticks (je i32).
pub const CB_PLAY_SET_TITLES_ANIMATION: i32 = 0x71;
// ClearTitleS2CPacket: Titel ausblenden (bool reset = auch Zeiten zuruecksetzen).
pub const CB_PLAY_CLEAR_TITLES: i32 = 0x0E;

// Manche Server schicken die Actionbar als eigenes Paket statt als System-Chat mit
// overlay=true -- beide Wege muessen dieselben Farben liefern.
pub fn read_action_bar_spans(payload: &[u8]) -> Result<Vec<crate::nbt::TextSpan>, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    read_text_spans(&mut r)
}

pub fn read_action_bar(payload: &[u8]) -> Result<String, ReadError> {
    Ok(read_action_bar_spans(payload)?.iter().map(|s| s.text.as_str()).collect())
}

// Titel und Untertitel MIT Farben, als Abschnitte wie Chat und Actionbar. Vorher als nackter
// Text gelesen -- ein farbiger Titel kam weiss an.
pub fn read_title_spans(payload: &[u8]) -> Result<Vec<crate::nbt::TextSpan>, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    read_text_spans(&mut r)
}

pub fn read_subtitle_spans(payload: &[u8]) -> Result<Vec<crate::nbt::TextSpan>, ReadError> {
    read_title_spans(payload)
}

// (fadeIn, stay, fadeOut) in Ticks.
pub fn read_title_fade(payload: &[u8]) -> Result<(i32, i32, i32), ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    Ok((r.read_i32()?, r.read_i32()?, r.read_i32()?))
}

// --- Erfahrung und Attribute (Rueckgrat der HUD-Leisten) ------------------------------------

// ExperienceBarUpdateS2CPacket (Index 101 in PlayStateFactories.S2C -- direkter Nachbar des
// bereits verifizierten SET_HEALTH 0x66 = Index 102).
pub const CB_PLAY_SET_EXPERIENCE: i32 = 0x65;

// Auf der Leitung: Fortschritt, STUFE, Gesamterfahrung -- am Lesecode des 1.21.11-Jars
// nachgesehen (ClientboundSetExperiencePacket(FriendlyByteBuf) liest experienceLevel vor
// totalExperience). Hier stand frueher Gesamt vor Stufe; die Leiste zeigte dann die
// Gesamtpunkte als Stufe.
//
// Rueckgabe: (Fortschritt 0..1 innerhalb der Stufe, Stufe, Gesamterfahrung).
pub fn read_experience(payload: &[u8]) -> Result<(f32, i32, i32), ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let progress = r.read_f32()?;
    let level = r.read_varint()?;
    let total = r.read_varint()?;
    Ok((progress, level, total))
}
