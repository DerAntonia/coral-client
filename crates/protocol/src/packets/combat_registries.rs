// Animationen, Schadensereignisse, Schadensarten, Verzauberungen und Entity-Ereignisse.

use super::*;

/// Entity-ID und Animationsart. Die Art ist ein UNSIGNED BYTE, kein `VarInt` -- Vanilla schreibt
/// sie mit `writeByte` und liest sie mit `readUnsignedByte`.
pub fn read_entity_animation(payload: &[u8]) -> Result<(i32, u8), ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let entity_id = r.read_varint()?;
    let animation = r.read_u8()?;
    Ok((entity_id, animation))
}

// EntityDamageS2CPacket ("Damage Event"): der Server meldet, dass ein Entity Schaden genommen
// hat. Das ist die EINZIGE Quelle fuer Vanillas rotes Aufblitzen -- ClientPlayNetworkHandler.
// onEntityDamage ruft Entity.onDamaged, und LivingEntity.onDamaged setzt daraufhin hurtTime.
//
// Die ID stammt wie bei CB_PLAY_ENTITY_EVENT aus der S2C-Registrierungsreihenfolge in
// PlayStateFactories, diesmal gegen ZEHN bekannte Pakete gegengeprueft (BLOCK_UPDATE 0x08,
// CONTAINER_SET_SLOT 0x14, DISCONNECT 0x20, ENTITY_EVENT 0x22, KEEP_ALIVE 0x2B,
// LEVEL_CHUNK_WITH_LIGHT 0x2C, PLAYER_POSITION 0x46, SET_HEALTH 0x66, SYSTEM_CHAT 0x77,
// TELEPORT_ENTITY 0x7B -- alle zehn treffen). Wichtig beim Nachzaehlen: die Liste enthaelt
// nicht nur `PlayPackets.*`, sondern auch `CommonPackets.*` und `CookiePackets.*` an ihrer
// jeweiligen Stelle -- wer die ueberspringt, zaehlt sich um neun Positionen daneben.
pub const CB_PLAY_DAMAGE_EVENT: i32 = 0x19;

/// Entity-ID und Schadensart.
///
/// Die Schadensart ist eine schlichte VarInt-Registry-ID -- `DamageType.ENTRY_PACKET_CODEC` ist
/// `PacketCodecs.registryEntry(RegistryKey)` in der EINARGUMENTIGEN Form, also OHNE
/// Inline-Codec und damit ohne die +1-Verschiebung der Klang-IDs (am Bytecode geprueft; ein
/// frueherer Kommentar hier vermutete das Gegenteil). Sie zeigt in die vom Server geschickte
/// `minecraft:damage_type`-Registry, s. `read_damage_types`.
///
/// Gebraucht wird sie fuer den EIGENEN Schadenston: den schickt der Server bewusst nicht mit
/// (s. `App::play_own_hurt_sound`), der Client muss ihn selbst spielen -- und welcher es ist,
/// haengt an der Schadensart.
///
/// Danach folgen Verursacher- und direkte Quell-ID (beide als VarInt+1, damit -1 = keine
/// kodierbar bleibt) und optional eine Position (Bool, dann drei Doubles) -- so steht es in
/// `ClientboundDamageEventPacket.write`. Gebraucht fuer die Richtung, aus der ein Kill kam
/// (Todesexplosion). Fehlen sie (abgeschnittenes Paket), bleiben sie leer; Entity und
/// Schadensart zaehlen trotzdem.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DamageEvent {
    pub entity_id: i32,
    pub damage_type: i32,
    // DamageSource.getEntity: wer den Schaden verursacht hat (Schuetze, Schlagender).
    pub cause_id: Option<i32>,
    // DamageSource.getDirectEntity: was direkt getroffen hat (Pfeil, Kristall, der Schlagende).
    pub direct_id: Option<i32>,
    // DamageSource.sourcePositionRaw: feste Quelle ohne Entity (etwa ein Bett, ein Anker).
    pub source_pos: Option<[f64; 3]>,
}

pub fn read_damage_event(payload: &[u8]) -> Result<DamageEvent, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let entity_id = r.read_varint()?;
    let damage_type = r.read_varint()?;
    let mut ev = DamageEvent { entity_id, damage_type, cause_id: None, direct_id: None, source_pos: None };
    let id = |v: i32| (v > 0).then_some(v - 1);
    if let (Ok(cause), Ok(direct)) = (r.read_varint(), r.read_varint()) {
        ev.cause_id = id(cause);
        ev.direct_id = id(direct);
        if r.read_bool().unwrap_or(false) {
            if let (Ok(x), Ok(y), Ok(z)) = (r.read_f64(), r.read_f64(), r.read_f64()) {
                ev.source_pos = Some([x, y, z]);
            }
        }
    }
    Ok(ev)
}

/// Welchen Klang eine Schadensart ausloest -- Vanillas `DamageEffects`.
///
/// Die Werte kommen als Zeichenkette im Feld `effects` der Schadensart-Registry; fehlt es,
/// gilt `Hurt`. Beachtenswert: `Thorns` hat KEINEN eigenen Klang, es teilt sich den mit
/// `Hurt` (am Bytecode von `DamageEffects` abgelesen, nicht angenommen).
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DamageEffect {
    #[default]
    Hurt,
    Thorns,
    Drowning,
    Burning,
    Poking,
    Freezing,
}

impl DamageEffect {
    #[must_use] 
    pub fn from_name(name: &str) -> Self {
        match name {
            "thorns" => Self::Thorns,
            "drowning" => Self::Drowning,
            "burning" => Self::Burning,
            "poking" => Self::Poking,
            "freezing" => Self::Freezing,
            // Auch fuer unbekannte Werte: Vanillas Standard ist HURT, und ein neuer Effekt in
            // einer spaeteren Version soll den Ton nicht verstummen lassen.
            _ => Self::Hurt,
        }
    }

    /// Der Klang, den ein SPIELER dabei von sich gibt (`PlayerEntity::getHurtSound` ->
    /// `DamageType.effects().getSound()`).
    #[must_use] 
    pub const fn player_hurt_sound(self) -> &'static str {
        match self {
            Self::Hurt | Self::Thorns => "minecraft:entity.player.hurt",
            Self::Drowning => "minecraft:entity.player.hurt_drown",
            Self::Burning => "minecraft:entity.player.hurt_on_fire",
            Self::Poking => "minecraft:entity.player.hurt_sweet_berry_bush",
            Self::Freezing => "minecraft:entity.player.hurt_freeze",
        }
    }
}

/// Die Schadensart-Registry aus den Konfigurationsdaten lesen -- in REGISTRY-REIHENFOLGE, denn
/// das Schadenspaket nennt sie als Index in genau diese Liste.
///
/// `None` = dieses Registry-Paket war eine andere Registry (es kommen viele).
pub fn read_damage_types(payload: &[u8]) -> Result<Option<Vec<DamageEffect>>, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let registry = r.read_string()?;
    let count = r.read_varint()? as usize;

    if registry != "minecraft:damage_type" {
        return Ok(None);
    }

    let mut effects = Vec::with_capacity(count);
    for _ in 0..count {
        let _name = r.read_string()?;
        let has_data = r.read_bool()?;
        let data = if has_data { nbt::read_root(&mut r)? } else { None };
        // Fehlendes `effects` heisst HURT -- so steht es in Vanillas Codec
        // (optionalFieldOf("effects", HURT)), es ist kein Ausweichwert von uns.
        let effect = data.as_ref().and_then(|d| d.get("effects")).and_then(|v| v.as_str()).map(DamageEffect::from_name).unwrap_or_default();
        effects.push(effect);
    }
    Ok(Some(effects))
}

/// Die Verzauberungs-Registry aus den Konfigurationsdaten: je Eintrag der Name und die
/// vollstaendige Definition (Beschreibung, Maximalstufe, Effekte). Die POSITION in der Liste
/// ist die ID, mit der Items ihre Verzauberungen nennen (s. `item::Enchantment`).
///
/// Die Definition kommt immer mit, weil dieser Client keine bekannten Packs meldet (s.
/// `serverbound_known_packs_empty_payload`) -- sonst schickte der Server nur die Namen.
///
/// `None` = dieses Registry-Paket war eine andere Registry.
pub fn read_enchantments(payload: &[u8]) -> Result<Option<Vec<(String, Option<nbt::NbtValue>)>>, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let registry = r.read_string()?;
    if registry != "minecraft:enchantment" {
        return Ok(None);
    }
    let count = r.read_varint()?.max(0) as usize;
    let mut out = Vec::with_capacity(count.min(1024));
    for _ in 0..count {
        let name = r.read_string()?;
        let data = if r.read_bool()? { nbt::read_root(&mut r)? } else { None };
        out.push((name, data));
    }
    Ok(Some(out))
}

/// Die Registries, deren Eintraege nur als NAMEN gebraucht werden (Items nennen sie per
/// Position): Trim-Materialien, Trim-Muster, Bannermuster. Liefert (Registry, je Eintrag Name
/// und Definition) -- `None` fuer jede andere Registry.
pub fn read_named_registry(payload: &[u8]) -> Result<Option<(String, Vec<(String, Option<nbt::NbtValue>)>)>, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let registry = r.read_string()?;
    if !matches!(registry.as_str(), "minecraft:trim_material" | "minecraft:trim_pattern" | "minecraft:banner_pattern" | "minecraft:jukebox_song" | "minecraft:cat_variant" | "minecraft:painting_variant" | "minecraft:dialog") {
        return Ok(None);
    }
    let count = r.read_varint()?.max(0) as usize;
    let mut out = Vec::with_capacity(count.min(1024));
    for _ in 0..count {
        let name = r.read_string()?;
        let data = if r.read_bool()? { nbt::read_root(&mut r)? } else { None };
        out.push((name, data));
    }
    Ok(Some((registry, out)))
}

#[cfg(test)]
mod animate_tests {
    use super::*;

    #[test]
    // Die Animationsart ist ein einzelnes BYTE hinter einer VarInt-Entity-ID. Als VarInt
    // gelesen ginge es fuer die Werte 0..5 zufaellig gut -- bis zu einer ID mit gesetztem
    // Fortsetzungsbit, ab der alles verrutscht.
    fn liest_id_als_varint_und_die_art_als_byte() {
        let mut w = PacketWriter::new();
        w.write_varint(CB_PLAY_ANIMATE);
        w.write_varint(300);
        w.write_u8(ANIMATION_SWING_OFF_HAND);
        assert_eq!(read_entity_animation(&w.into_bytes()).expect("lesbar"), (300, ANIMATION_SWING_OFF_HAND));
    }

    #[test]
    fn abgeschnittenes_paket_ist_ein_fehler() {
        assert!(read_entity_animation(&[CB_PLAY_ANIMATE as u8]).is_err());
    }

    #[test]
    // Die 1 ist in Vanilla nicht vergeben. Steht sie hier doch in der Liste, hat jemand die
    // Werte durchnummeriert statt sie abzuschreiben -- und ab der 1 waere alles verschoben.
    fn die_luecke_bei_eins_bleibt_eine_luecke() {
        for art in [ANIMATION_SWING_MAIN_HAND, ANIMATION_WAKE_UP, ANIMATION_SWING_OFF_HAND, ANIMATION_CRIT, ANIMATION_ENCHANTED_HIT] {
            assert_ne!(art, 1, "1 ist in EntityAnimationS2CPacket unbelegt");
        }
        assert_eq!(ANIMATION_SWING_MAIN_HAND, 0);
        assert_eq!(ANIMATION_SWING_OFF_HAND, 3);
    }
}

#[cfg(test)]
mod damage_event_tests {
    use super::*;

    #[test]
    // Die Entity-ID ist hier ein VarInt -- anders als beim Entity-Event daneben, das sie als
    // festen i32 schreibt. Eine als i32 gelesene VarInt-ID waere stiller Muell.
    fn liest_die_entity_id_als_varint() {
        let mut w = PacketWriter::new();
        w.write_varint(CB_PLAY_DAMAGE_EVENT);
        w.write_varint(300);
        w.write_varint(7); // Schadensart
        let ev = read_damage_event(&w.into_bytes()).expect("lesbar");
        assert_eq!((ev.entity_id, ev.damage_type, ev.cause_id, ev.direct_id, ev.source_pos), (300, 7, None, None, None));
    }

    // Wie ClientboundDamageEventPacket.write: IDs als VarInt+1 (0 = keine), Position optional.
    #[test]
    fn liest_quelle_und_position() {
        let mut w = PacketWriter::new();
        w.write_varint(CB_PLAY_DAMAGE_EVENT);
        w.write_varint(300);
        w.write_varint(7);
        w.write_varint(42 + 1);
        w.write_varint(0);
        w.write_bool(true);
        w.write_f64(1.5);
        w.write_f64(64.0);
        w.write_f64(-3.25);
        let ev = read_damage_event(&w.into_bytes()).expect("lesbar");
        assert_eq!(ev.cause_id, Some(42));
        assert_eq!(ev.direct_id, None);
        assert_eq!(ev.source_pos, Some([1.5, 64.0, -3.25]));
    }

    #[test]
    fn abgeschnittenes_paket_ist_ein_fehler() {
        assert!(read_damage_event(&[CB_PLAY_DAMAGE_EVENT as u8]).is_err());
    }

    // Die Zuordnung stammt aus Vanillas DamageEffects-Enum. Besonders `thorns`: es hat KEINEN
    // eigenen Klang, sondern denselben wie ein gewoehnlicher Treffer -- das sieht wie ein
    // Fehler aus und ist keiner.
    #[test]
    fn die_schadensarten_treffen_vanillas_klaenge() {
        use DamageEffect::*;
        assert_eq!(Hurt.player_hurt_sound(), "minecraft:entity.player.hurt");
        assert_eq!(Thorns.player_hurt_sound(), "minecraft:entity.player.hurt");
        assert_eq!(Drowning.player_hurt_sound(), "minecraft:entity.player.hurt_drown");
        assert_eq!(Burning.player_hurt_sound(), "minecraft:entity.player.hurt_on_fire");
        assert_eq!(Poking.player_hurt_sound(), "minecraft:entity.player.hurt_sweet_berry_bush");
        assert_eq!(Freezing.player_hurt_sound(), "minecraft:entity.player.hurt_freeze");
    }

    // Ein unbekannter Effekt darf nicht verstummen, sondern faellt auf HURT zurueck -- sonst
    // waere der eigene Schadenston nach dem naechsten Vanilla-Update weg.
    #[test]
    fn ein_unbekannter_effekt_faellt_auf_hurt_zurueck() {
        assert_eq!(DamageEffect::from_name("gibtsnochnicht"), DamageEffect::Hurt);
        assert_eq!(DamageEffect::default(), DamageEffect::Hurt);
    }

    // Die Registry kommt in Reihenfolge; der Index aus dem Schadenspaket zeigt genau hierhin.
    // Ein Eintrag ohne `effects` ist HURT (Vanillas optionalFieldOf-Standard), einer ganz ohne
    // Daten ebenfalls -- beides kommt in echten Registry-Paketen vor.
    #[test]
    fn die_schadensart_registry_wird_in_reihenfolge_gelesen() {
        let mut w = PacketWriter::new();
        w.write_varint(CB_CONFIG_REGISTRY_DATA).write_string("minecraft:damage_type").write_varint(3);
        w.write_string("minecraft:in_fire").write_bool(true);
        nbt_mit_effekt(&mut w, "burning");
        w.write_string("minecraft:generic").write_bool(true);
        nbt_ohne_effekt(&mut w);
        w.write_string("minecraft:drown").write_bool(false);
        let effekte = read_damage_types(&w.into_bytes()).expect("lesbar").expect("ist die Schadensart-Registry");
        assert_eq!(effekte, vec![DamageEffect::Burning, DamageEffect::Hurt, DamageEffect::Hurt]);
    }

    // Es kommen viele Registry-Pakete -- die anderen muessen sauber None ergeben.
    #[test]
    fn eine_andere_registry_wird_uebergangen() {
        let mut w = PacketWriter::new();
        w.write_varint(CB_CONFIG_REGISTRY_DATA).write_string("minecraft:worldgen/biome").write_varint(0);
        assert_eq!(read_damage_types(&w.into_bytes()).expect("lesbar"), None);
    }

    // Ein NBT-Compound mit genau einem String-Feld "effects" (Netzwerkform: kein Wurzelname).
    //
    // NICHT write_bytes benutzen: das ist Minecrafts "Prefixed Array of Byte" und schreibt
    // eine VarInt-Laenge davor. NBT-Zeichenketten haben eine u16-Laenge und danach die
    // ROHBYTES -- mit write_bytes stuende dort eine Laenge zu viel.
    fn nbt_string(w: &mut PacketWriter, text: &str) {
        w.write_u16(text.len() as u16);
        for b in text.bytes() {
            w.write_u8(b);
        }
    }

    fn nbt_mit_effekt(w: &mut PacketWriter, effekt: &str) {
        w.write_u8(10); // TAG_Compound
        w.write_u8(8); // TAG_String
        nbt_string(w, "effects");
        nbt_string(w, effekt);
        w.write_u8(0); // TAG_End
    }

    fn nbt_ohne_effekt(w: &mut PacketWriter) {
        w.write_u8(10);
        w.write_u8(0);
    }

    #[test]
    // Beide Pakete betreffen Entities und liegen im Strom dicht beieinander -- eine
    // Verwechslung der IDs waere teuer zu finden.
    fn ist_ein_anderes_paket_als_das_entity_event() {
        assert_ne!(CB_PLAY_DAMAGE_EVENT, CB_PLAY_ENTITY_EVENT);
    }
}

// EntityId ist hier ein fester i32, KEIN VarInt (EntityStatusS2CPacket: buf.writeInt) --
// eines der wenigen Pakete, die aus der VarInt-Regel ausbrechen.
pub fn read_entity_event(payload: &[u8]) -> Result<(i32, i8), ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let entity_id = r.read_i32()?;
    let status = r.read_u8()? as i8;
    Ok((entity_id, status))
}

#[cfg(test)]
mod entity_event_tests {
    use super::*;

    #[test]
    // Der Aufbau des Pakets, und vor allem: die EntityId ist ein fester i32. Liest man sie
    // als VarInt, kommt bei kleinen IDs zufaellig das Richtige heraus und ab EntityId 128
    // dauerhaft Muell -- ein Fehler, der erst auf einem laufenden Server auffaellt.
    fn liest_id_als_festen_i32_und_den_status() {
        let mut w = PacketWriter::new();
        w.write_varint(CB_PLAY_ENTITY_EVENT);
        w.write_i32(300);
        w.write_u8(ENTITY_STATUS_USE_TOTEM as u8);
        let (id, status) = read_entity_event(&w.into_bytes()).expect("lesbar");
        assert_eq!(id, 300, "EntityId 300 braucht 4 feste Bytes -- als VarInt gelesen waere es 172");
        assert_eq!(status, ENTITY_STATUS_USE_TOTEM);
    }

    #[test]
    // Statuswerte sind VORZEICHENBEHAFTET (byte). Vanilla nutzt auch negative (z.B. -1 gibt
    // es nicht, aber Werte > 127 gaebe es als negative Bytes) -- als u8 gelesen waere ein
    // Vergleich mit einer negativen Konstante nie wahr.
    fn status_ist_vorzeichenbehaftet() {
        let mut w = PacketWriter::new();
        w.write_varint(CB_PLAY_ENTITY_EVENT);
        w.write_i32(1);
        w.write_u8(0xFF);
        let (_, status) = read_entity_event(&w.into_bytes()).expect("lesbar");
        assert_eq!(status, -1);
    }

    #[test]
    fn abgeschnittenes_paket_ist_ein_fehler() {
        assert!(read_entity_event(&[0x22, 0x00]).is_err());
    }
}

// --- Befehls-Vervollstaendigung (Tab) ------------------------------------------------------
//
// Die Vorschlaege kommen vom SERVER, nicht aus einem lokalen Befehlsbaum: der Client fragt
// mit dem bisher getippten Text an, der Server antwortet mit den passenden Ergaenzungen.
// Das deckt Befehlsnamen UND Argumente ab (Spielernamen, Item-IDs, Warps eines Plugins) --
// ein lokal nachgebauter Baum koennte das Zweite gar nicht wissen.
//
// IDs: C2S ist die 0-basierte Position in der C2S-Registrierung (COMMAND_SUGGESTION an
// Position 15 der 1-basierten Liste -> 0x0E), S2C die 1-basierte (COMMAND_SUGGESTIONS an 15
// -> 0x0F; die S2C-Liste hat bei 0 den Bundle-Delimiter). Beide Konventionen gegen vorhandene,
// nachweislich funktionierende Konstanten geprueft: SB_PLAY_CHAT (0x08, Position 9) und
// SB_PLAY_CHAT_COMMAND (0x06, Position 7) fuer C2S, CONTAINER_SET_SLOT (0x14, Position 20)
// fuer S2C.
pub const SB_PLAY_COMMAND_SUGGESTION: i32 = 0x0E;
pub const CB_PLAY_COMMAND_SUGGESTIONS: i32 = 0x0F;
