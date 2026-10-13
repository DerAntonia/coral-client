// Partikel auf der Leitung: der Typ-Codec (ParticleTypes.PACKET_CODEC) und die drei Pakete,
// die Partikel ausloesen -- Level Particles, Level Event und Explode.
//
// Ein Partikel ist auf der Leitung "Typ-ID, dann typabhaengige Parameter". Die ID ist
// `PacketCodecs.registryValue(PARTICLE_TYPE)`, also die NACKTE Registry-Zahl (kein +1 wie bei
// den registryEntry-Feldern, s. read_sound_event). Die Parameter haengen am Typ -- und weil
// hinter einem Partikel im Explode-Paket noch Ton und Blockpartikel-Liste folgen, muss JEDER
// Typ byteweise korrekt ueberlesen werden koennen, auch einer, den der Client gar nicht
// zeichnet.

use crate::buffer::{PacketReader, ReadError};
use crate::item::read_optional_slot;
use crate::packets::decode_block_pos;

// Registry-Reihenfolge von Registries.PARTICLE_TYPE, maschinell aus ParticleTypes.<clinit>
// (Yarn 1.21.11) gezogen. PARTICLE_TYPE ist eine statische Registry: die Roh-ID IST die
// Registrierungsreihenfolge, der Server kann sie nicht umstellen.
pub const PARTICLE_NAMES: [&str; 115] = [
    "angry_villager", "block", "block_marker", "bubble", "cloud", "copper_fire_flame", "crit",
    "damage_indicator", "dragon_breath", "dripping_lava", "falling_lava", "landing_lava",
    "dripping_water", "falling_water", "dust", "dust_color_transition", "effect", "elder_guardian",
    "enchanted_hit", "enchant", "end_rod", "entity_effect", "explosion_emitter", "explosion",
    "gust", "small_gust", "gust_emitter_large", "gust_emitter_small", "sonic_boom", "falling_dust",
    "firework", "fishing", "flame", "infested", "cherry_leaves", "pale_oak_leaves",
    "tinted_leaves", "sculk_soul", "sculk_charge", "sculk_charge_pop", "soul_fire_flame", "soul",
    "flash", "happy_villager", "composter", "heart", "instant_effect", "item", "vibration",
    "trail", "item_slime", "item_cobweb", "item_snowball", "large_smoke", "lava", "mycelium",
    "note", "poof", "portal", "rain", "smoke", "white_smoke", "sneeze", "spit", "squid_ink",
    "sweep_attack", "totem_of_undying", "underwater", "splash", "witch", "bubble_pop",
    "current_down", "bubble_column_up", "nautilus", "dolphin", "campfire_cosy_smoke",
    "campfire_signal_smoke", "dripping_honey", "falling_honey", "landing_honey", "falling_nectar",
    "falling_spore_blossom", "ash", "crimson_spore", "warped_spore", "spore_blossom_air",
    "dripping_obsidian_tear", "falling_obsidian_tear", "landing_obsidian_tear", "reverse_portal",
    "white_ash", "small_flame", "snowflake", "dripping_dripstone_lava", "falling_dripstone_lava",
    "dripping_dripstone_water", "falling_dripstone_water", "glow_squid_ink", "glow", "wax_on",
    "wax_off", "electric_spark", "scrape", "shriek", "egg_crack", "dust_plume",
    "trial_spawner_detection", "trial_spawner_detection_ominous", "vault_connection",
    "dust_pillar", "ominous_spawning", "raid_omen", "trial_omen", "block_crumble", "firefly",
];

pub const PARTICLE_BLOCK: i32 = 1;
pub const PARTICLE_BLOCK_MARKER: i32 = 2;
pub const PARTICLE_CRIT: i32 = 6;
pub const PARTICLE_DAMAGE_INDICATOR: i32 = 7;
pub const PARTICLE_DRAGON_BREATH: i32 = 8;
pub const PARTICLE_DUST: i32 = 14;
pub const PARTICLE_DUST_COLOR_TRANSITION: i32 = 15;
pub const PARTICLE_EFFECT: i32 = 16;
pub const PARTICLE_ENCHANTED_HIT: i32 = 18;
pub const PARTICLE_ENTITY_EFFECT: i32 = 21;
pub const PARTICLE_EXPLOSION_EMITTER: i32 = 22;
pub const PARTICLE_EXPLOSION: i32 = 23;
pub const PARTICLE_GUST: i32 = 24;
pub const PARTICLE_SMALL_GUST: i32 = 25;
pub const PARTICLE_GUST_EMITTER_LARGE: i32 = 26;
pub const PARTICLE_GUST_EMITTER_SMALL: i32 = 27;
pub const PARTICLE_FALLING_DUST: i32 = 29;
pub const PARTICLE_TINTED_LEAVES: i32 = 36;
pub const PARTICLE_SCULK_CHARGE: i32 = 38;
pub const PARTICLE_FLASH: i32 = 42;
pub const PARTICLE_INSTANT_EFFECT: i32 = 46;
pub const PARTICLE_ITEM: i32 = 47;
pub const PARTICLE_VIBRATION: i32 = 48;
pub const PARTICLE_TRAIL: i32 = 49;
// Splitter fester Items ohne Parameter (BreakingItemParticle.SlimeProvider & Co.).
pub const PARTICLE_ITEM_SLIME: i32 = 50;
pub const PARTICLE_ITEM_COBWEB: i32 = 51;
pub const PARTICLE_ITEM_SNOWBALL: i32 = 52;
pub const PARTICLE_LARGE_SMOKE: i32 = 53;
pub const PARTICLE_POOF: i32 = 57;
pub const PARTICLE_SMOKE: i32 = 60;
pub const PARTICLE_SWEEP_ATTACK: i32 = 65;
pub const PARTICLE_TOTEM_OF_UNDYING: i32 = 66;
pub const PARTICLE_CURRENT_DOWN: i32 = 71;
pub const PARTICLE_BUBBLE_COLUMN_UP: i32 = 72;
pub const PARTICLE_SHRIEK: i32 = 103;
pub const PARTICLE_DUST_PILLAR: i32 = 109;
pub const PARTICLE_BLOCK_CRUMBLE: i32 = 113;

pub fn particle_name(type_id: i32) -> Option<&'static str> {
    usize::try_from(type_id).ok().and_then(|i| PARTICLE_NAMES.get(i)).copied()
}

// ParticleType.shouldAlwaysSpawn() -- das zweite Argument von ParticleTypes.register, alle 28
// `true`-Faelle maschinell aus demselben <clinit> gezogen (auch die mehrzeiligen). Solche
// Typen ignorieren Entfernungsgrenze und Partikel-Stufe (s. app::particles::spawn_allowed).
pub fn particle_always_spawns(type_id: i32) -> bool {
    matches!(
        particle_name(type_id),
        Some(
            "block_marker"
                | "damage_indicator"
                | "elder_guardian"
                | "explosion_emitter"
                | "explosion"
                | "gust"
                | "gust_emitter_large"
                | "gust_emitter_small"
                | "sonic_boom"
                | "sculk_charge"
                | "sculk_charge_pop"
                | "vibration"
                | "poof"
                | "spit"
                | "squid_ink"
                | "sweep_attack"
                | "campfire_cosy_smoke"
                | "campfire_signal_smoke"
                | "glow_squid_ink"
                | "glow"
                | "wax_on"
                | "wax_off"
                | "electric_spark"
                | "scrape"
                | "trial_spawner_detection"
                | "trial_spawner_detection_ominous"
                | "vault_connection"
                | "ominous_spawning"
        )
    )
}

// Die Parameter eines Partikels. Nur was der Client tatsaechlich auswertet, bekommt Felder;
// der Rest ist gelesen, aber verworfen.
#[derive(Clone, Debug, PartialEq)]
pub enum ParticleOptions {
    None,
    // BlockStateParticleEffect (block, block_marker, falling_dust, dust_pillar, block_crumble).
    BlockState(u32),
    // DustParticleEffect: Farbe als 0xRRGGBB, Groesse (Vanilla klemmt sie auf 0.01..4).
    Dust { color: u32, scale: f32 },
    DustTransition { from: u32, to: u32, scale: f32 },
    // TintedParticleEffect (entity_effect, tinted_leaves, flash): ARGB.
    Tinted(u32),
    // EffectParticleEffect (effect, instant_effect): Farbe + Staerke.
    Effect { color: u32, power: f32 },
    // ItemStackParticleEffect: das Item und, falls gesetzt, seine item_model-Component -- das
    // Aussehen folgt dem Modell.
    Item { item_id: u32, item_model: Option<String> },
    Other,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParticleEffect {
    pub type_id: i32,
    pub options: ParticleOptions,
}

pub fn read_particle(r: &mut PacketReader) -> Result<ParticleEffect, ReadError> {
    let type_id = r.read_varint()?;
    let options = match type_id {
        PARTICLE_BLOCK | PARTICLE_BLOCK_MARKER | PARTICLE_FALLING_DUST | PARTICLE_DUST_PILLAR
        | PARTICLE_BLOCK_CRUMBLE => ParticleOptions::BlockState(r.read_varint()? as u32),
        PARTICLE_DRAGON_BREATH | PARTICLE_SCULK_CHARGE => {
            r.read_f32()?;
            ParticleOptions::Other
        }
        PARTICLE_DUST => {
            let color = r.read_i32()? as u32;
            ParticleOptions::Dust { color, scale: r.read_f32()? }
        }
        PARTICLE_DUST_COLOR_TRANSITION => {
            let from = r.read_i32()? as u32;
            let to = r.read_i32()? as u32;
            ParticleOptions::DustTransition { from, to, scale: r.read_f32()? }
        }
        PARTICLE_EFFECT | PARTICLE_INSTANT_EFFECT => {
            let color = r.read_i32()? as u32;
            ParticleOptions::Effect { color, power: r.read_f32()? }
        }
        PARTICLE_ENTITY_EFFECT | PARTICLE_TINTED_LEAVES | PARTICLE_FLASH => ParticleOptions::Tinted(r.read_i32()? as u32),
        // ItemStack.PACKET_CODEC -- dasselbe Leitungsformat wie der optionale Slot, nur dass
        // Vanilla einen leeren Stapel ablehnt. Eine unbekannte Komponente macht die
        // Leseposition unbrauchbar; dann ist das ganze Paket verloren.
        PARTICLE_ITEM => match read_optional_slot(r) {
            Ok(Some(stack)) => ParticleOptions::Item { item_id: stack.item_id, item_model: stack.item_model },
            Ok(None) => return Err(ReadError),
            Err(_) => return Err(ReadError),
        },
        // VibrationParticleEffect: PositionSource (Typ-ID, dann Block-Position ODER
        // Entity-ID + Hoehenversatz), danach die Ankunftszeit in Ticks.
        PARTICLE_VIBRATION => {
            match r.read_varint()? {
                0 => {
                    r.read_i64()?;
                }
                1 => {
                    r.read_varint()?;
                    r.read_f32()?;
                }
                _ => return Err(ReadError),
            }
            r.read_varint()?;
            ParticleOptions::Other
        }
        // TrailParticleEffect: Ziel (3x f64), Farbe (i32), Dauer (VarInt).
        PARTICLE_TRAIL => {
            r.read_f64()?;
            r.read_f64()?;
            r.read_f64()?;
            r.read_i32()?;
            r.read_varint()?;
            ParticleOptions::Other
        }
        PARTICLE_SHRIEK => {
            r.read_varint()?;
            ParticleOptions::Other
        }
        _ if particle_name(type_id).is_some() => ParticleOptions::None,
        // Eine ID jenseits der Tabelle: der Server spricht eine neuere Version. Ob der Typ
        // Parameter hat, ist unbekannt -- weiterlesen hiesse raten.
        _ => return Err(ReadError),
    };
    Ok(ParticleEffect { type_id, options })
}

// --- Level Particles --------------------------------------------------------------------
//
// ParticleS2CPacket. ID 0x2E = Position 46 der S2C-Reihenfolge in PlayStateFactories, direkt
// hinter LEVEL_EVENT (45) und gegen die Nachbarn ENTITY_EVENT 0x22 (34), EXPLODE 0x24 (36)
// und KEEP_ALIVE 0x2B (43) gegengezaehlt -- alle drei sind im Client verifiziert.
pub const CB_PLAY_LEVEL_PARTICLES: i32 = 0x2E;

#[derive(Clone, Debug, PartialEq)]
pub struct LevelParticles {
    pub force_spawn: bool,
    pub important: bool,
    pub pos: (f64, f64, f64),
    pub offset: (f32, f32, f32),
    pub speed: f32,
    pub count: i32,
    pub particle: ParticleEffect,
}

pub fn read_level_particles(payload: &[u8]) -> Result<LevelParticles, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let force_spawn = r.read_bool()?;
    let important = r.read_bool()?;
    let pos = (r.read_f64()?, r.read_f64()?, r.read_f64()?);
    let offset = (r.read_f32()?, r.read_f32()?, r.read_f32()?);
    let speed = r.read_f32()?;
    let count = r.read_i32()?;
    let particle = read_particle(&mut r)?;
    Ok(LevelParticles { force_spawn, important, pos, offset, speed, count, particle })
}

// --- Level Event ------------------------------------------------------------------------
//
// WorldEventS2CPacket: Ereignis-ID (i32), Block-Position (Long), Datenwert (i32), global
// (bool). Position 45 = 0x2D, s. CB_PLAY_LEVEL_PARTICLES.
pub const CB_PLAY_LEVEL_EVENT: i32 = 0x2D;

// WorldEvents.BLOCK_BROKEN: Datenwert ist die Block-State-ID. Der Server schickt es an alle
// AUSSER dem Abbauenden -- der spielt denselben Effekt lokal ab (s. app::particles).
pub const LEVEL_EVENT_BLOCK_BROKEN: i32 = 2001;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LevelEvent {
    pub event: i32,
    pub pos: (i32, i32, i32),
    pub data: i32,
}

pub fn read_level_event(payload: &[u8]) -> Result<LevelEvent, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let event = r.read_i32()?;
    let pos = decode_block_pos(r.read_i64()?);
    let data = r.read_i32()?;
    let _global = r.read_bool()?;
    Ok(LevelEvent { event, pos, data })
}

// --- Explode ----------------------------------------------------------------------------
//
// ExplosionS2CPacket, 1.21.11-Fassung. Bis zum Rueckstoss s. packets::read_explosion_knockback;
// danach folgen der Partikel in der Mitte (bei Kristallen und TNT explosion_emitter), der Ton
// (SoundEvent.ENTRY_PACKET_CODEC) und eine gewichtete Liste von Blockpartikeln, die Vanilla im
// Explosionsradius verteilt (s. app::particles::schedule_block_particles).

#[derive(Clone, Debug, PartialEq)]
pub struct BlockParticle {
    pub particle: ParticleEffect,
    pub scaling: f32,
    pub speed: f32,
    pub weight: i32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Explosion {
    pub center: (f64, f64, f64),
    pub radius: f32,
    pub block_count: i32,
    pub knockback: Option<(f64, f64, f64)>,
    pub particle: ParticleEffect,
    // None: vom Server frei benannter oder uns unbekannter Ton.
    pub sound: Option<String>,
    pub block_particles: Vec<BlockParticle>,
}

pub fn read_explosion(payload: &[u8]) -> Result<Explosion, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let center = (r.read_f64()?, r.read_f64()?, r.read_f64()?);
    let radius = r.read_f32()?;
    let block_count = r.read_i32()?;
    let knockback = if r.read_bool()? { Some((r.read_f64()?, r.read_f64()?, r.read_f64()?)) } else { None };
    let particle = read_particle(&mut r)?;
    let sound = crate::packets::read_sound_event_entry(&mut r)?;
    let n = r.read_varint()?;
    if !(0..=4096).contains(&n) {
        return Err(ReadError);
    }
    let mut block_particles = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let particle = read_particle(&mut r)?;
        let scaling = r.read_f32()?;
        let speed = r.read_f32()?;
        let weight = r.read_varint()?;
        block_particles.push(BlockParticle { particle, scaling, speed, weight });
    }
    Ok(Explosion { center, radius, block_count, knockback, particle, sound, block_particles })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::PacketWriter;

    #[test]
    fn konstanten_zeigen_auf_die_richtigen_namen() {
        let paare = [
            (PARTICLE_BLOCK, "block"),
            (PARTICLE_BLOCK_MARKER, "block_marker"),
            (PARTICLE_CRIT, "crit"),
            (PARTICLE_DAMAGE_INDICATOR, "damage_indicator"),
            (PARTICLE_DRAGON_BREATH, "dragon_breath"),
            (PARTICLE_DUST, "dust"),
            (PARTICLE_DUST_COLOR_TRANSITION, "dust_color_transition"),
            (PARTICLE_EFFECT, "effect"),
            (PARTICLE_ENCHANTED_HIT, "enchanted_hit"),
            (PARTICLE_ENTITY_EFFECT, "entity_effect"),
            (PARTICLE_EXPLOSION_EMITTER, "explosion_emitter"),
            (PARTICLE_EXPLOSION, "explosion"),
            (PARTICLE_FALLING_DUST, "falling_dust"),
            (PARTICLE_TINTED_LEAVES, "tinted_leaves"),
            (PARTICLE_SCULK_CHARGE, "sculk_charge"),
            (PARTICLE_FLASH, "flash"),
            (PARTICLE_INSTANT_EFFECT, "instant_effect"),
            (PARTICLE_ITEM, "item"),
            (PARTICLE_VIBRATION, "vibration"),
            (PARTICLE_TRAIL, "trail"),
            (PARTICLE_ITEM_SLIME, "item_slime"),
            (PARTICLE_ITEM_COBWEB, "item_cobweb"),
            (PARTICLE_ITEM_SNOWBALL, "item_snowball"),
            (PARTICLE_LARGE_SMOKE, "large_smoke"),
            (PARTICLE_POOF, "poof"),
            (PARTICLE_SMOKE, "smoke"),
            (PARTICLE_SWEEP_ATTACK, "sweep_attack"),
            (PARTICLE_TOTEM_OF_UNDYING, "totem_of_undying"),
            (PARTICLE_CURRENT_DOWN, "current_down"),
            (PARTICLE_BUBBLE_COLUMN_UP, "bubble_column_up"),
            (PARTICLE_SHRIEK, "shriek"),
            (PARTICLE_DUST_PILLAR, "dust_pillar"),
            (PARTICLE_BLOCK_CRUMBLE, "block_crumble"),
        ];
        for (id, name) in paare {
            assert_eq!(particle_name(id), Some(name), "ID {id}");
        }
    }

    #[test]
    fn genau_28_typen_erscheinen_immer() {
        let n = (0..PARTICLE_NAMES.len() as i32).filter(|&id| particle_always_spawns(id)).count();
        assert_eq!(n, 28, "Zahl der register(..., true)-Aufrufe in ParticleTypes");
        assert!(particle_always_spawns(PARTICLE_EXPLOSION_EMITTER));
        assert!(!particle_always_spawns(PARTICLE_CRIT));
    }

    #[test]
    fn paket_ids_liegen_zwischen_den_verifizierten_nachbarn() {
        // KEEP_ALIVE 0x2B (43), dann 44 = LEVEL_CHUNK_WITH_LIGHT 0x2C, 45 = LEVEL_EVENT,
        // 46 = LEVEL_PARTICLES.
        assert_eq!(crate::packets::CB_PLAY_CHUNK_DATA + 1, CB_PLAY_LEVEL_EVENT);
        assert_eq!(CB_PLAY_LEVEL_EVENT + 1, CB_PLAY_LEVEL_PARTICLES);
        assert_eq!(crate::packets::CB_PLAY_EXPLODE, 0x24);
    }

    fn kopf(w: &mut PacketWriter) {
        w.write_varint(CB_PLAY_LEVEL_PARTICLES);
        w.write_bool(false).write_bool(true);
        w.write_f64(1.5).write_f64(64.0).write_f64(-3.25);
        w.write_f32(0.1).write_f32(0.2).write_f32(0.3);
        w.write_f32(0.5);
        w.write_i32(12);
    }

    #[test]
    fn liest_level_particles_mit_staub_parametern() {
        let mut w = PacketWriter::new();
        kopf(&mut w);
        w.write_varint(PARTICLE_DUST).write_i32(0xFF8000).write_f32(1.5);
        let p = read_level_particles(&w.into_bytes()).expect("lesbar");
        assert!(p.important && !p.force_spawn);
        assert_eq!(p.pos, (1.5, 64.0, -3.25));
        assert_eq!(p.count, 12);
        assert_eq!(p.particle.options, ParticleOptions::Dust { color: 0xFF8000, scale: 1.5 });
    }

    #[test]
    fn liest_blockpartikel_und_parameterlose_typen() {
        let mut w = PacketWriter::new();
        kopf(&mut w);
        w.write_varint(PARTICLE_BLOCK).write_varint(9);
        let p = read_level_particles(&w.into_bytes()).expect("lesbar");
        assert_eq!(p.particle.options, ParticleOptions::BlockState(9));

        let mut w = PacketWriter::new();
        kopf(&mut w);
        w.write_varint(PARTICLE_CRIT);
        let p = read_level_particles(&w.into_bytes()).expect("lesbar");
        assert_eq!(p.particle, ParticleEffect { type_id: PARTICLE_CRIT, options: ParticleOptions::None });
    }

    #[test]
    fn unbekannte_typ_id_ist_ein_fehler() {
        let mut w = PacketWriter::new();
        kopf(&mut w);
        w.write_varint(PARTICLE_NAMES.len() as i32);
        assert!(read_level_particles(&w.into_bytes()).is_err());
    }

    #[test]
    fn liest_level_event() {
        let mut w = PacketWriter::new();
        w.write_varint(CB_PLAY_LEVEL_EVENT);
        w.write_i32(LEVEL_EVENT_BLOCK_BROKEN);
        // x=10, y=-5, z=-20 nach BlockPos.asLong (X 26 Bit @38, Z 26 Bit @12, Y 12 Bit @0).
        let packed = ((10i64 & 0x3FF_FFFF) << 38) | ((-20i64 & 0x3FF_FFFF) << 12) | (-5i64 & 0xFFF);
        w.write_i64(packed).write_i32(1234).write_bool(false);
        let e = read_level_event(&w.into_bytes()).expect("lesbar");
        assert_eq!(e, LevelEvent { event: LEVEL_EVENT_BLOCK_BROKEN, pos: (10, -5, -20), data: 1234 });
    }

    #[test]
    // Das ganze Paket bis zum letzten Byte -- gerade die Teile HINTER dem Rueckstoss, die der
    // alte Parser nie angefasst hat.
    fn liest_explosion_komplett() {
        let mut w = PacketWriter::new();
        w.write_varint(crate::packets::CB_PLAY_EXPLODE);
        w.write_f64(0.5).write_f64(70.0).write_f64(0.5);
        w.write_f32(6.0).write_i32(40);
        w.write_bool(true).write_f64(0.25).write_f64(0.5).write_f64(-0.25);
        w.write_varint(PARTICLE_EXPLOSION_EMITTER);
        // Ton als Registry-Eintrag: ID + 1.
        let explode = crate::sound_events::SOUND_EVENTS
            .iter()
            .position(|n| *n == "entity.generic.explode")
            .expect("Ton bekannt");
        w.write_varint(explode as i32 + 1);
        w.write_varint(2);
        w.write_varint(PARTICLE_POOF).write_f32(0.5).write_f32(1.0).write_varint(3);
        w.write_varint(PARTICLE_SMOKE).write_f32(1.0).write_f32(1.0).write_varint(1);
        let e = read_explosion(&w.into_bytes()).expect("lesbar");
        assert_eq!(e.knockback, Some((0.25, 0.5, -0.25)));
        assert_eq!(e.particle.type_id, PARTICLE_EXPLOSION_EMITTER);
        assert_eq!(e.sound.as_deref(), Some("minecraft:entity.generic.explode"));
        assert_eq!(e.block_particles.len(), 2);
        assert_eq!(e.block_particles[0].weight, 3);
        assert_eq!(e.block_particles[1].particle.type_id, PARTICLE_SMOKE);
    }
}
