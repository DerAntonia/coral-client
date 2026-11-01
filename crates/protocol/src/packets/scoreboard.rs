// Scoreboard: Anzeige, Ziele, Punkte und Teams.

use super::*;

// ScoreboardDisplayS2CPacket: welcher Objective in welchem Slot angezeigt wird. Slot 1 =
// Seitenleiste (die uns interessiert), 0 = Spielerliste, 2 = unter dem Namen, 3..18 =
// teamgefaerbte Seitenleisten.
pub const CB_PLAY_SET_DISPLAY_OBJECTIVE: i32 = 0x60;
// ScoreboardObjectiveUpdateS2CPacket: Objective anlegen/entfernen/aktualisieren.
pub const CB_PLAY_SET_OBJECTIVE: i32 = 0x68;
// ScoreboardScoreUpdateS2CPacket: ein Zeilen-Score setzen.
pub const CB_PLAY_SET_SCORE: i32 = 0x6C;
// ScoreboardScoreResetS2CPacket: einen Score-Eintrag entfernen.
pub const CB_PLAY_RESET_SCORE: i32 = 0x4D;

// (slot, objective_name)
pub fn read_display_objective(payload: &[u8]) -> Result<(i32, String), ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let slot = r.read_varint()?;
    let name = r.read_string()?;
    Ok((slot, name))
}

pub struct ObjectiveUpdate {
    pub name: String,
    // 0 = anlegen, 1 = entfernen, 2 = aktualisieren.
    pub mode: u8,
    // Anzeigename (nur bei anlegen/aktualisieren) -- der Titel der Seitenleiste.
    pub display: Option<String>,
    // ScoreboardCriterion.RenderType: HEARTS (1) statt INTEGER (0). Die Spielerliste zeichnet
    // dann Herzen statt einer Zahl.
    pub hearts: bool,
    // Das Zahlenformat des Objectives (gilt fuer jeden Score ohne eigenes).
    pub number_format: Option<NumberFormat>,
}

// NumberFormatTypes, Registrierungsreihenfolge blank/styled/fixed (am 1.21.11-Jar
// abgelesen) -- so, wie ein Score neben dem Namen erscheint.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub enum NumberFormat {
    // Gar nichts anzeigen.
    Blank,
    // Die Zahl, in dieser Farbe (None = Standardfarbe des Aufrufers).
    Styled(Option<u32>),
    // Ein fester Text statt der Zahl.
    Fixed(Vec<crate::nbt::TextSpan>),
}

// NumberFormatTypes.OPTIONAL_PACKET_CODEC: Bool + VarInt-Typ + Inhalt.
pub(crate) fn read_optional_number_format(r: &mut PacketReader) -> Result<Option<NumberFormat>, ReadError> {
    if !r.read_bool()? {
        return Ok(None);
    }
    Ok(Some(match r.read_varint()? {
        0 => NumberFormat::Blank,
        // StyledNumberFormat: ein Style als NBT-Compound.
        1 => NumberFormat::Styled(nbt::read_root(r)?.as_ref().and_then(nbt::style_color)),
        2 => NumberFormat::Fixed(read_text_spans(r)?),
        _ => return Err(ReadError),
    }))
}

pub fn read_objective(payload: &[u8]) -> Result<ObjectiveUpdate, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let name = r.read_string()?;
    let mode = r.read_u8()?;
    // Bei anlegen(0)/aktualisieren(2) folgen Anzeigename + RenderType + Optional<NumberFormat>.
    let (mut display, mut hearts, mut number_format) = (None, false, None);
    if mode == 0 || mode == 2 {
        display = Some(read_text_component_legacy(&mut r)?);
        hearts = r.read_varint()? == 1;
        number_format = read_optional_number_format(&mut r)?;
    }
    Ok(ObjectiveUpdate { name, mode, display, hearts, number_format })
}

pub struct ScoreUpdate {
    pub holder: String,
    pub objective: String,
    pub score: i32,
    // Optionaler Anzeige-Text der Zeile (seit 1.20.3); ohne = der Holder-Name wird angezeigt.
    pub display: Option<String>,
    // Eigenes Zahlenformat dieses Scores (vor dem des Objectives).
    pub number_format: Option<NumberFormat>,
}

pub fn read_score(payload: &[u8]) -> Result<ScoreUpdate, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let holder = r.read_string()?;
    let objective = r.read_string()?;
    let score = r.read_varint()?;
    // Optional<Text> display (bool praesent + ggf. Text); Optional<NumberFormat> danach ignoriert.
    let display = if r.read_bool()? { Some(read_text_component_legacy(&mut r)?) } else { None };
    let number_format = read_optional_number_format(&mut r)?;
    Ok(ScoreUpdate { holder, objective, score, display, number_format })
}

// TeamS2CPacket (SET_PLAYER_TEAM, Index 107 in PlayStateFactories.S2C -- direkt zwischen den
// bereits verifizierten SET_OBJECTIVE 0x68 (104) und SET_SCORE 0x6C (108)). Viele Server bauen
// ihre Seitenleiste aus Dummy-Holdern ("0".."15") und schreiben den sichtbaren Text in
// Prefix/Suffix eines Teams, dem der Holder angehoert -- ohne dieses Paket zeigt die
// Seitenleiste nur die nackten Holder-Namen.
pub const CB_PLAY_SET_PLAYER_TEAM: i32 = 0x6B;

pub struct TeamUpdate {
    pub name: String,
    // 0 = anlegen (mit Team-Info UND Mitgliedern), 1 = entfernen, 2 = Info aktualisieren,
    // 3 = Mitglieder hinzufuegen, 4 = Mitglieder entfernen.
    pub mode: u8,
    // Nur bei 0/2 vorhanden (SerializableTeam).
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    // AbstractTeam.VisibilityRule als Index: 0 ALWAYS, 1 NEVER, 2 HIDE_FOR_OTHER_TEAMS,
    // 3 HIDE_FOR_OWN_TEAM (die Reihenfolge steht so im Enum, der Codec ist PacketCodecs
    // .indexed). Daran haengt, ob ueber einem Spieler ueberhaupt ein Name stehen darf --
    // Server mit eigenen Rang-Hologrammen stellen ihn auf NEVER.
    pub name_tag_visibility: Option<u8>,
    // Formatting-Index der Teamfarbe (dieselben 16 Codes wie die Paragraphen-Farben);
    // Werte ausserhalb 0..15 heissen "keine Farbe" (Vanilla: RESET).
    pub color: Option<i32>,
    // AbstractTeam.CollisionRule als Index: 0 ALWAYS, 1 NEVER, 2 und 3 je nach Teamzugehoerigkeit
    // (s. app::team_allows_push). Entscheidet, ob Entities einander schubsen.
    pub collision_rule: Option<u8>,
    // Nur bei 0/3/4 vorhanden.
    pub entries: Vec<String>,
    // Anzeigename (Legacy-Codes), nur bei 0/2 -- das Zuschauer-Menue nennt Teams damit.
    pub display_name: Option<String>,
}

pub fn read_team(payload: &[u8]) -> Result<TeamUpdate, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let name = r.read_string()?;
    let mode = r.read_u8()?;
    let (mut prefix, mut suffix) = (None, None);
    // SerializableTeam: Text displayName, Byte friendlyFlags, VarInt nameTagVisibility,
    // VarInt collisionRule (beide PacketCodecs.indexed, seit 1.21.5 keine Strings mehr),
    // VarInt color (writeEnumConstant), Text prefix, Text suffix.
    let (mut name_tag_visibility, mut color, mut collision_rule, mut display_name) = (None, None, None, None);
    if mode == 0 || mode == 2 {
        display_name = Some(read_text_component_legacy(&mut r)?);
        let _friendly_flags = r.read_u8()?;
        name_tag_visibility = Some(r.read_varint()?.clamp(0, 255) as u8);
        collision_rule = Some(r.read_varint()?.clamp(0, 255) as u8);
        color = Some(r.read_varint()?);
        prefix = Some(read_text_component_legacy(&mut r)?);
        suffix = Some(read_text_component_legacy(&mut r)?);
    }
    let mut entries = Vec::new();
    if mode == 0 || mode == 3 || mode == 4 {
        let count = r.read_varint()?.max(0);
        entries.reserve(count.min(256) as usize);
        for _ in 0..count {
            entries.push(r.read_string()?);
        }
    }
    Ok(TeamUpdate { name, mode, prefix, suffix, name_tag_visibility, color, collision_rule, entries, display_name })
}

// (holder, optional objective_name -- None = aus allen Objectives entfernen)
pub fn read_reset_score(payload: &[u8]) -> Result<(String, Option<String>), ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let holder = r.read_string()?;
    let objective = if r.read_bool()? { Some(r.read_string()?) } else { None };
    Ok((holder, objective))
}
