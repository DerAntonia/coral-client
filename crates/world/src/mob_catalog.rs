// Welches Modell und welche Textur ein Entity-Typ bekommt, fuer alle Typen ohne eigenes,
// handgeschriebenes Modell (s. entity_model::model_table). Die Geometrie kommt aus
// model_data (Layer-Name), die Textur aus der Installation. Die Zuordnung folgt den
// Renderern des 1.21.11-Clients (EntityRendererFactories): Layer und Standardtextur je Typ.
//
// Varianten (Fellfarben, Biome, ...) kennt diese Tabelle nur, wo der Server sie schickt und
// der Client sie liest -- sonst steht die Standardtextur da.

use crate::entity_model::{Model, Part, PartPose, Pose};

// Wie das Modell bewegt wird. Fast alle Vanilla-Modelle benennen ihre Teile gleich (head,
// right_hind_leg, right_leg, right_arm, ...); daran haengt die gemeinsame Animation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anim {
    // Kopf folgt dem Blick, Beine (vier oder zwei) und Arme schwingen beim Laufen.
    Walker,
    // Dorfbewohner-Art: verschraenkte Arme, Beine mit halbem Ausschlag.
    Villager,
    // Illager: verschraenkte Arme, solange nichts in der Hand ist, sonst ausgestreckte.
    Illager,
    // Nur der Kopf (Schwebende, Fische, Golems ohne Beine).
    HeadOnly,
    // Starr (Boote, Loren, ...).
    Static,
}

#[derive(Clone, Copy, Debug)]
pub struct MobLook {
    pub layer: &'static str,
    // Eigenes Babymodell (1.21.11 hat fuer viele Tiere eines); sonst das erwachsene.
    pub baby_layer: Option<&'static str>,
    pub texture: &'static str,
    // Textur des Babys, falls es eine eigene hat.
    pub baby_texture: Option<&'static str>,
    pub anim: Anim,
}

const fn look(layer: &'static str, texture: &'static str, anim: Anim) -> MobLook {
    MobLook { layer, baby_layer: None, texture, baby_texture: None, anim }
}

const fn ageable(layer: &'static str, baby: &'static str, texture: &'static str, anim: Anim) -> MobLook {
    MobLook { layer, baby_layer: Some(baby), texture, baby_texture: None, anim }
}

pub fn mob_look(name: &str) -> Option<MobLook> {
    use Anim::*;
    Some(match name {
        "allay" => look("allay#main", "entity/allay/allay", HeadOnly),
        "armadillo" => ageable("armadillo#main", "armadillo_baby#main", "entity/armadillo", Walker),
        "armor_stand" => look("armor_stand#main", "entity/armorstand/wood", Static),
        "axolotl" => ageable("axolotl#main", "axolotl_baby#main", "entity/axolotl/axolotl_lucy", Walker),
        "bat" => look("bat#main", "entity/bat", HeadOnly),
        "bee" => ageable("bee#main", "bee_baby#main", "entity/bee/bee", Static),
        "blaze" => look("blaze#main", "entity/blaze", HeadOnly),
        "breeze" => look("breeze#main", "entity/breeze/breeze", HeadOnly),
        "camel" => ageable("camel#main", "camel_baby#main", "entity/camel/camel", Walker),
        "camel_husk" => ageable("camel#main", "camel_baby#main", "entity/camel/camel_husk", Walker),
        "chicken" => ageable("chicken#main", "chicken_baby#main", "entity/chicken/temperate_chicken", Walker),
        "cod" => look("cod#main", "entity/fish/cod", Static),
        "copper_golem" => look("copper_golem#main", "entity/copper_golem/copper_golem", Walker),
        "creaking" => look("creaking#main", "entity/creaking/creaking", Walker),
        "dolphin" => ageable("dolphin#main", "dolphin_baby#main", "entity/dolphin", HeadOnly),
        "donkey" => ageable("donkey#main", "donkey_baby#main", "entity/horse/donkey", Walker),
        "mule" => ageable("mule#main", "mule_baby#main", "entity/horse/mule", Walker),
        "horse" => ageable("horse#main", "horse_baby#main", "entity/horse/horse_brown", Walker),
        "skeleton_horse" => ageable("skeleton_horse#main", "skeleton_horse_baby#main", "entity/horse/horse_skeleton", Walker),
        "zombie_horse" => ageable("zombie_horse#main", "zombie_horse_baby#main", "entity/horse/horse_zombie", Walker),
        "elder_guardian" => look("elder_guardian#main", "entity/guardian_elder", HeadOnly),
        "guardian" => look("guardian#main", "entity/guardian", HeadOnly),
        "ender_dragon" => look("ender_dragon#main", "entity/enderdragon/dragon", Static),
        "endermite" => look("endermite#main", "entity/endermite", Static),
        "evoker" => look("evoker#main", "entity/illager/evoker", Illager),
        "illusioner" => look("illusioner#main", "entity/illager/illusioner", Illager),
        "pillager" => look("pillager#main", "entity/illager/pillager", Illager),
        "vindicator" => look("vindicator#main", "entity/illager/vindicator", Illager),
        "fox" => ageable("fox#main", "fox_baby#main", "entity/fox/fox", Walker),
        "frog" => look("frog#main", "entity/frog/temperate_frog", HeadOnly),
        "ghast" => look("ghast#main", "entity/ghast/ghast", Static),
        "happy_ghast" => MobLook {
            layer: "happy_ghast#main",
            baby_layer: Some("happy_ghast_baby#main"),
            texture: "entity/ghast/happy_ghast",
            baby_texture: Some("entity/ghast/happy_ghast_baby"),
            anim: Static,
        },
        "giant" => look("giant#main", "entity/zombie/zombie", Walker),
        "glow_squid" => ageable("glow_squid#main", "glow_squid_baby#main", "entity/squid/glow_squid", Static),
        "squid" => ageable("squid#main", "squid_baby#main", "entity/squid/squid", Static),
        "goat" => ageable("goat#main", "goat_baby#main", "entity/goat/goat", Walker),
        "hoglin" => ageable("hoglin#main", "hoglin_baby#main", "entity/hoglin/hoglin", Walker),
        "zoglin" => ageable("zoglin#main", "zoglin_baby#main", "entity/hoglin/zoglin", Walker),
        "husk" => ageable("husk#main", "husk_baby#main", "entity/zombie/husk", Walker),
        "iron_golem" => look("iron_golem#main", "entity/iron_golem/iron_golem", Walker),
        "llama" => ageable("llama#main", "llama_baby#main", "entity/llama/creamy", Walker),
        "trader_llama" => ageable("trader_llama#main", "trader_llama_baby#main", "entity/llama/creamy", Walker),
        "nautilus" => MobLook {
            layer: "nautilus#main",
            baby_layer: Some("nautilus_baby#main"),
            texture: "entity/nautilus/nautilus",
            baby_texture: Some("entity/nautilus/nautilus_baby"),
            anim: Static,
        },
        "zombie_nautilus" => look("zombie_nautilus#main", "entity/nautilus/zombie_nautilus", Static),
        "panda" => ageable("panda#main", "panda_baby#main", "entity/panda/panda", Walker),
        "parrot" => look("parrot#main", "entity/parrot/parrot_red_blue", HeadOnly),
        "phantom" => look("phantom#main", "entity/phantom", Static),
        "polar_bear" => ageable("polar_bear#main", "polar_bear_baby#main", "entity/bear/polarbear", Walker),
        "pufferfish" => look("pufferfish_small#main", "entity/fish/pufferfish", Static),
        "rabbit" => ageable("rabbit#main", "rabbit_baby#main", "entity/rabbit/brown", HeadOnly),
        "ravager" => look("ravager#main", "entity/illager/ravager", Walker),
        "salmon" => look("salmon#main", "entity/fish/salmon", Static),
        "shulker" => look("shulker#main", "entity/shulker/shulker", Static),
        "silverfish" => look("silverfish#main", "entity/silverfish", Static),
        "sniffer" => ageable("sniffer#main", "sniffer_baby#main", "entity/sniffer/sniffer", Walker),
        "snow_golem" => look("snow_golem#main", "entity/snow_golem", HeadOnly),
        "strider" => ageable("strider#main", "strider_baby#main", "entity/strider/strider", Walker),
        "tadpole" => look("tadpole#main", "entity/tadpole/tadpole", Static),
        "tropical_fish" => look("tropical_fish_small#main", "entity/fish/tropical_a", Static),
        "vex" => look("vex#main", "entity/illager/vex", HeadOnly),
        "villager" => ageable("villager#main", "villager_baby#main", "entity/villager/villager", Villager),
        "wandering_trader" => look("wandering_trader#main", "entity/wandering_trader", Villager),
        "warden" => look("warden#main", "entity/warden/warden", Walker),
        "witch" => look("witch#main", "entity/witch", Villager),
        "wither" => look("wither#main", "entity/wither/wither", Static),
        "wolf" => ageable("wolf#main", "wolf_baby#main", "entity/wolf/wolf", Walker),
        "zombie_villager" => ageable("zombie_villager#main", "zombie_villager_baby#main", "entity/zombie_villager/zombie_villager", Walker),
        // Keine Lebewesen, aber mit eigenem Modell.
        "minecart" | "chest_minecart" | "command_block_minecart" | "furnace_minecart" | "hopper_minecart" | "spawner_minecart" | "tnt_minecart" => {
            look("minecart#main", "entity/minecart", Static)
        }
        "leash_knot" => look("leash_knot#main", "entity/lead_knot", Static),
        "llama_spit" => look("llama_spit#main", "entity/llama/spit", Static),
        "shulker_bullet" => look("shulker_bullet#main", "entity/shulker/spark", Static),
        "evoker_fangs" => look("evoker_fangs#main", "entity/illager/evoker_fangs", Static),
        _ => return boat_look(name),
    })
}

// Boote und Floesse: "<holz>_boat", "<holz>_chest_boat", "bamboo_raft", "bamboo_chest_raft".
// Layer und Textur tragen den Holznamen (boat/oak, entity/boat/oak).
pub const BOAT_WOODS: [&str; 10] = ["oak", "spruce", "birch", "jungle", "acacia", "cherry", "dark_oak", "mangrove", "pale_oak", "bamboo"];

fn boat_look(name: &str) -> Option<MobLook> {
    let (holz, kiste) = if let Some(h) = name.strip_suffix("_chest_boat").or_else(|| name.strip_suffix("_chest_raft")) {
        (h, true)
    } else {
        (name.strip_suffix("_boat").or_else(|| name.strip_suffix("_raft"))?, false)
    };
    let i = BOAT_WOODS.iter().position(|w| *w == holz)?;
    const LAYER: [&str; 10] = [
        "boat/oak#main",
        "boat/spruce#main",
        "boat/birch#main",
        "boat/jungle#main",
        "boat/acacia#main",
        "boat/cherry#main",
        "boat/dark_oak#main",
        "boat/mangrove#main",
        "boat/pale_oak#main",
        "boat/bamboo#main",
    ];
    const LAYER_KISTE: [&str; 10] = [
        "chest_boat/oak#main",
        "chest_boat/spruce#main",
        "chest_boat/birch#main",
        "chest_boat/jungle#main",
        "chest_boat/acacia#main",
        "chest_boat/cherry#main",
        "chest_boat/dark_oak#main",
        "chest_boat/mangrove#main",
        "chest_boat/pale_oak#main",
        "chest_boat/bamboo#main",
    ];
    const TEX: [&str; 10] = [
        "entity/boat/oak",
        "entity/boat/spruce",
        "entity/boat/birch",
        "entity/boat/jungle",
        "entity/boat/acacia",
        "entity/boat/cherry",
        "entity/boat/dark_oak",
        "entity/boat/mangrove",
        "entity/boat/pale_oak",
        "entity/boat/bamboo",
    ];
    const TEX_KISTE: [&str; 10] = [
        "entity/chest_boat/oak",
        "entity/chest_boat/spruce",
        "entity/chest_boat/birch",
        "entity/chest_boat/jungle",
        "entity/chest_boat/acacia",
        "entity/chest_boat/cherry",
        "entity/chest_boat/dark_oak",
        "entity/chest_boat/mangrove",
        "entity/chest_boat/pale_oak",
        "entity/chest_boat/bamboo",
    ];
    Some(if kiste { look(LAYER_KISTE[i], TEX_KISTE[i], Anim::Static) } else { look(LAYER[i], TEX[i], Anim::Static) })
}

pub fn is_boat(name: &str) -> bool {
    boat_look(name).is_some()
}

pub fn is_minecart(name: &str) -> bool {
    name == "minecart" || name.ends_with("_minecart")
}

// Dorfbewohner-Daten (VillagerData): Biom-Typ und Beruf als Registry-IDs, Stufe 1..5. Die
// Namen in Registry-Reihenfolge (villager_type, villager_profession).
pub const VILLAGER_TYPES: [&str; 7] = ["desert", "jungle", "plains", "savanna", "snow", "swamp", "taiga"];
pub const VILLAGER_PROFESSIONS: [&str; 15] = [
    "none",
    "armorer",
    "butcher",
    "cartographer",
    "cleric",
    "farmer",
    "fisherman",
    "fletcher",
    "leatherworker",
    "librarian",
    "mason",
    "nitwit",
    "shepherd",
    "toolsmith",
    "weaponsmith",
];
const VILLAGER_LEVELS: [&str; 5] = ["stone", "iron", "gold", "emerald", "diamond"];

// Die Schichten eines Dorfbewohners bzw. Zombie-Dorfbewohners ueber der Grundtextur, von
// unten nach oben (VillagerClothingFeatureRenderer): Biom-Kleidung, Beruf, Stufen-Abzeichen.
// Babys tragen keinen Beruf; Nitwits kein Abzeichen.
pub fn villager_layers(zombie: bool, data: Option<(i32, i32, i32)>, baby: bool) -> Vec<String> {
    let ordner = if zombie { "entity/zombie_villager" } else { "entity/villager" };
    let (typ, beruf, stufe) = data.unwrap_or((2, 0, 1));
    let mut out = Vec::new();
    if let Some(t) = usize::try_from(typ).ok().and_then(|i| VILLAGER_TYPES.get(i)) {
        out.push(format!("{ordner}/type/{t}"));
    }
    let beruf = usize::try_from(beruf).ok().and_then(|i| VILLAGER_PROFESSIONS.get(i)).copied().unwrap_or("none");
    if beruf != "none" && !baby {
        out.push(format!("{ordner}/profession/{beruf}"));
        if beruf != "nitwit" {
            let i = (stufe.clamp(1, 5) - 1) as usize;
            out.push(format!("{ordner}/profession_level/{}", VILLAGER_LEVELS[i]));
        }
    }
    out
}

// Alle Texturen, die diese Tabelle braucht -- fuer den Entity-Atlas.
pub fn all_textures() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let namen = [
        "allay", "armadillo", "armor_stand", "axolotl", "bat", "bee", "blaze", "breeze", "camel", "camel_husk", "chicken", "cod", "copper_golem",
        "creaking", "dolphin", "donkey", "mule", "horse", "skeleton_horse", "zombie_horse", "elder_guardian", "guardian", "ender_dragon",
        "endermite", "evoker", "illusioner", "pillager", "vindicator", "fox", "frog", "ghast", "happy_ghast", "giant", "glow_squid", "squid",
        "goat", "hoglin", "zoglin", "husk", "iron_golem", "llama", "trader_llama", "nautilus", "zombie_nautilus", "panda", "parrot", "phantom",
        "polar_bear", "pufferfish", "rabbit", "ravager", "salmon", "shulker", "silverfish", "sniffer", "snow_golem", "strider", "tadpole",
        "tropical_fish", "vex", "villager", "wandering_trader", "warden", "witch", "wither", "wolf", "zombie_villager", "minecart", "leash_knot",
        "llama_spit", "shulker_bullet", "evoker_fangs",
    ];
    let boote = BOAT_WOODS.iter().flat_map(|w| {
        let raft = if *w == "bamboo" { "raft" } else { "boat" };
        [format!("{w}_{raft}"), format!("{w}_chest_{raft}")]
    });
    for n in namen.iter().map(|s| s.to_string()).chain(boote) {
        if let Some(l) = mob_look(&n) {
            out.push(l.texture.to_string());
            out.extend(l.baby_texture.map(str::to_string));
        }
    }
    for zombie in [false, true] {
        let ordner = if zombie { "entity/zombie_villager" } else { "entity/villager" };
        out.extend(VILLAGER_TYPES.iter().map(|t| format!("{ordner}/type/{t}")));
        out.extend(VILLAGER_PROFESSIONS.iter().filter(|p| **p != "none").map(|p| format!("{ordner}/profession/{p}")));
        out.extend(VILLAGER_LEVELS.iter().map(|l| format!("{ordner}/profession_level/{l}")));
    }
    out.sort();
    out.dedup();
    out
}

fn has_part(part: &Part, name: &str) -> bool {
    part.name == name || part.children.iter().any(|c| has_part(c, name))
}

// Die gemeinsame Laufanimation (QuadrupedModel, BipedModel, VillagerModel, IllagerModel:
// ueberall dieselben Formeln an denselben Teilnamen). Winkel in Grad (Kopf relativ zum
// Rumpf), Laufphase und -staerke wie bei quadruped_pose.
pub fn walk_pose(model: &Model, anim: Anim, head_yaw_rel: f32, head_pitch: f32, swing: f32, amount: f32, holds_item: bool) -> Pose {
    use std::f32::consts::PI;
    const DEG: f32 = PI / 180.0;
    let mut pose = Pose::new();
    if anim == Anim::Static {
        return pose;
    }
    pose.rotate("head", [head_pitch * DEG, head_yaw_rel * DEG, 0.0]);
    if anim == Anim::HeadOnly {
        return pose;
    }
    let root = &model.parts[0];
    let vorn = (swing * 0.6662).cos() * 1.4 * amount;
    let hinten = (swing * 0.6662 + PI).cos() * 1.4 * amount;
    if has_part(root, "right_hind_leg") {
        pose.rotate("right_hind_leg", [vorn, 0.0, 0.0]);
        pose.rotate("left_hind_leg", [hinten, 0.0, 0.0]);
        pose.rotate("right_front_leg", [hinten, 0.0, 0.0]);
        pose.rotate("left_front_leg", [vorn, 0.0, 0.0]);
    }
    // Dorfbewohner und Illager schwingen die Beine nur halb so weit.
    let bein = if matches!(anim, Anim::Villager | Anim::Illager) { 0.5 } else { 1.0 };
    pose.rotate("right_leg", [vorn * bein, 0.0, 0.0]);
    pose.rotate("left_leg", [hinten * bein, 0.0, 0.0]);
    match anim {
        Anim::Illager => {
            // Verschraenkt ODER ausgestreckt -- nie beides (IllagerEntityModel.setAngles).
            let versteckt = if holds_item { "arms" } else { "right_arm" };
            pose.set(versteckt, PartPose { hidden: true, ..Default::default() });
            if !holds_item {
                pose.set("left_arm", PartPose { hidden: true, ..Default::default() });
            } else {
                pose.rotate("right_arm", [(swing * 0.6662 + PI).cos() * amount, 0.0, 0.0]);
                pose.rotate("left_arm", [(swing * 0.6662).cos() * amount, 0.0, 0.0]);
            }
        }
        Anim::Walker if !has_part(root, "arms") => {
            pose.rotate("right_arm", [(swing * 0.6662 + PI).cos() * amount, 0.0, 0.0]);
            pose.rotate("left_arm", [(swing * 0.6662).cos() * amount, 0.0, 0.0]);
        }
        _ => {}
    }
    pose
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dorfbewohner_schichten() {
        // Schnee-Bibliothekar Stufe 3: Kleidung, Beruf, Gold-Abzeichen.
        assert_eq!(
            villager_layers(false, Some((4, 9, 3)), false),
            ["entity/villager/type/snow", "entity/villager/profession/librarian", "entity/villager/profession_level/gold"]
        );
        // Nitwit ohne Abzeichen, Baby ohne Beruf.
        assert_eq!(villager_layers(false, Some((2, 11, 1)), false).len(), 2);
        assert_eq!(villager_layers(true, Some((0, 5, 2)), true), ["entity/zombie_villager/type/desert"]);
    }
}
