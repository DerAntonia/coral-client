// Client-seitige Vorhersage fuers Platzieren eines Blocks.
//
// WOZU: bisher wurde nur USE_ITEM_ON verschickt und abgewartet. Der Server setzt den Block
// sofort, der Client erst, wenn dessen Block-Update-Paket eintrifft -- dazwischen liegt eine
// volle Rundreise, in der unsere Kollision den Block noch nicht kennt. Wer direkt in einen
// gerade gesetzten Block hineinlaeuft, bewegt sich in dieser Zeit HINEIN, waehrend GrimAC
// schon dagegen rechnet. Das Ergebnis ist eine Abweichung in genau der Groessenordnung, die
// als Simulation-Flag hochkommt.
//
// Vanilla macht das nicht anders: ClientPlayerInteractionManager.interactBlock fuehrt die
// Benutzung LOKAL aus (BlockItem.place setzt den Block in der ClientWorld) und merkt sich im
// PendingUpdateManager den vorherigen Zustand samt Sequenznummer. Kommt der Block Changed Ack
// mit dieser Nummer, wird der gemerkte Zustand angewendet -- entweder der inzwischen vom
// Server geschickte (Platzierung akzeptiert) oder der alte (abgelehnt, s. app::PendingBlock).
//
// BEWUSSTE VEREINFACHUNG: vorhergesagt wird nur der DEFAULT-State des Blocks, nicht die
// Ausrichtung (Treppenrichtung, halber Slab) -- ausser wo sie eindeutig folgt (Fackeln,
// Bloecke mit Blick zum Spieler, Achse einer Saeule). Fuer den Zweck -- Kollision
// und Anti-Cheat -- traegt das: die hier interessanten Bloecke (Obsidian, Enderkiste, Glas,
// Wolle) sind Vollwuerfel mit genau einem State, und der Server korrigiert alles andere
// spaetestens mit seinem Block-Update. Bei Bloecken mit mehreren States wird deshalb NICHT
// vorhergesagt (s. default_state_for_item) -- eine falsche Form waere schlimmer als gar keine.

use world::block_registry;
use world::block_shapes::BlockShapes;

// Bloecke, die beim Platzieren ERSETZT werden, statt dass der neue Block davor landet
// (Vanilla: BlockState.canReplace ueber das Material). Bewusst eine kurze, sichere Liste --
// wer hier fehlt, bekommt schlimmstenfalls keine Vorhersage.
pub fn is_replaceable(name: &str) -> bool {
    matches!(
        name,
        "air"
            | "cave_air"
            | "void_air"
            | "water"
            | "lava"
            | "bubble_column"
            | "short_grass"
            | "tall_grass"
            | "fern"
            | "large_fern"
            | "dead_bush"
            | "seagrass"
            | "tall_seagrass"
            | "vine"
            | "fire"
            | "soul_fire"
            | "snow"
            | "structure_void"
            | "light"
            | "hanging_roots"
            | "warped_roots"
            | "crimson_roots"
            | "nether_sprouts"
            | "glow_lichen"
    )
}

// Block -> sein Default-State als Property-Liste, fuer mehrstatige Bloecke ohne eigenes
// getPlacementState (s. default_state_for_item).
const FESTER_PLATZIERZUSTAND: &[(&str, &[(&str, &str)])] = &[("respawn_anchor", &[("charges", "0")])];

// Der zu setzende State fuer ein Item -- None, wenn wir uns nicht sicher genug sind.
//
// Sicher heisst hier: das Item traegt den Namen eines Blocks, und dieser Block hat GENAU
// EINEN State. Dann gibt es keine Ausrichtung zu raten. Mehrstatige Bloecke (Treppen, Zaeune,
// Staemme) bleiben bewusst aussen vor -- s. Modulkommentar.
pub fn default_state_for_item(item_name: &str) -> Option<u32> {
    // Mehrstatige Bloecke, deren getPlacementState NICHT ueberschrieben ist -- Vanilla setzt
    // dann einfach getDefaultState, es gibt nichts zu raten. Der Respawn-Anker
    // gehoert dazu: er hat fuenf States (charges 0..4) und braucht deshalb diesen Eintrag;
    // sein Default ist `charges=0` (RespawnAnchorBlock-Konstruktor, am Jar geprueft).
    if let Some((_, props)) = FESTER_PLATZIERZUSTAND.iter().find(|(name, _)| *name == item_name) {
        return (0..block_registry::state_count() as u32).find(|&state| {
            block_registry::block_name(state) == Some(item_name)
                && props.iter().all(|(k, v)| world::state_props::property(state, k) == Some(*v))
        });
    }
    let mut gefunden: Option<u32> = None;
    for state in 0..block_registry::state_count() as u32 {
        if block_registry::block_name(state) == Some(item_name) {
            if gefunden.is_some() {
                return None; // mehr als ein State -> Ausrichtung unbekannt, nicht raten
            }
            gefunden = Some(state);
        }
    }
    gefunden
}

// Fackeln: Vanilla setzt an eine Seitenflaeche die Wandfackel, die von der Wand weg zeigt
// (WallStandingBlockItem), oben die stehende. Die stehende hat nur einen State und wurde
// deshalb frueher auch an der Wand vorhergesagt -- einen Frame lang stand die Fackel dann
// frei im Raum, bis der Server die Wandfackel schickte. Unten (Decke) haengt es an
// der Blickrichtung; dort wird nicht vorhergesagt. None: kein Fackel-Item.
pub fn torch_state(item_name: &str, face: i32) -> Option<Option<u32>> {
    let wand = match item_name {
        "torch" => "wall_torch",
        "soul_torch" => "soul_wall_torch",
        "copper_torch" => "copper_wall_torch",
        _ => return None,
    };
    let richtung = match face {
        1 => return Some(default_state_for_item(item_name)),
        2 => "north",
        3 => "south",
        4 => "west",
        5 => "east",
        _ => return Some(None),
    };
    Some((0..block_registry::state_count() as u32).find(|&state| {
        block_registry::block_name(state) == Some(wand) && world::state_props::property(state, "facing") == Some(richtung)
    }))
}

// Bloecke, die beim Setzen zum Spieler schauen (facing = Gegenteil seiner waagerechten
// Blickrichtung) und sonst nur Properties mit fester Vorgabe haben. Ohne Vorhersage blieb der
// Platz fuer uns leer: der naechste Rechtsklick traf wieder dieselbe, beim Server schon
// verdeckte Flaeche -- GrimAC flaggt das, und der Platziersound kam ohne Block.
// None: nicht in der Liste.
pub fn facing_state(item_name: &str, yaw_deg: f32) -> Option<Option<u32>> {
    let feste: &[(&str, &str)] = match item_name {
        n if n.ends_with("_glazed_terracotta") => &[],
        "carved_pumpkin" | "jack_o_lantern" | "loom" | "stonecutter" => &[],
        "furnace" | "smoker" | "blast_furnace" => &[("lit", "false")],
        _ => return None,
    };
    // Direction.fromYRot: Viertel um 0 = Sueden, im Uhrzeigersinn West, Norden, Osten.
    let blick = ((yaw_deg / 90.0 + 0.5).floor() as i32).rem_euclid(4);
    let richtung = ["north", "east", "south", "west"][blick as usize];
    Some((0..block_registry::state_count() as u32).find(|&state| {
        block_registry::block_name(state) == Some(item_name)
            && world::state_props::property(state, "facing") == Some(richtung)
            && feste.iter().all(|(k, v)| world::state_props::property(state, k) == Some(*v))
    }))
}

// Truhen, Endertruhe und Kupfergolem-Statue: Blick zum Spieler wie oben, dazu
// waterlogged nach dem Platz (FluidState.is(WATER): nur stehendes Quellwasser). Gemeldet wie
// frueher bei den Staemmen -- ohne Vorhersage Ton und Armschwung ohne Block, und der naechste
// Klick traf die beim Server schon verdeckte Flaeche. Eine Truhe wird nur EINZELN
// vorhergesagt: steht nebenan eine einzelne Truhe, kann sie sich anschliessen
// (ChestBlock.getStateForPlacement), dann setzt der Server allein. None: nicht in der Liste.
pub fn facing_waterlogged_state(item_name: &str, yaw_deg: f32, water: bool, single_chest_beside: bool) -> Option<Option<u32>> {
    let feste: &[(&str, &str)] = match item_name {
        "ender_chest" => &[],
        "chest" | "trapped_chest" => &[("type", "single")],
        n if n.ends_with("copper_chest") => &[("type", "single")],
        n if world::copper_golem_statue_texture(n).is_some() => &[("copper_golem_pose", "standing")],
        _ => return None,
    };
    if single_chest_beside && feste.contains(&("type", "single")) {
        return Some(None);
    }
    let blick = ((yaw_deg / 90.0 + 0.5).floor() as i32).rem_euclid(4);
    let richtung = ["north", "east", "south", "west"][blick as usize];
    let nass = if water { "true" } else { "false" };
    Some((0..block_registry::state_count() as u32).find(|&state| {
        block_registry::block_name(state) == Some(item_name)
            && world::state_props::property(state, "facing") == Some(richtung)
            && world::state_props::property(state, "waterlogged") == Some(nass)
            && feste.iter().all(|(k, v)| world::state_props::property(state, k) == Some(*v))
    }))
}

// Kupfergolem-Statue: jeder Klick ohne Axt schaltet die Haltung weiter (CopperGolemStatueBlock.
// useItemOn -> updatePose, auch mit leerer Hand): Stehen, Sitzen, Laufen, Stern, wieder Stehen.
// Vanillas Client tut das sofort selbst; ohne Vorhersage hing jeder Wechsel an der Rundreise.
// None: keine Statue, oder die Axt (dann PASS -- sie schabt oder entwachst).
pub fn next_statue_pose_state(state: u32, item: Option<&str>) -> Option<u32> {
    world::copper_golem_statue_texture(block_registry::block_name(state)?)?;
    if item.is_some_and(|n| n.ends_with("_axe") && !n.ends_with("pickaxe")) {
        return None;
    }
    let naechste = match world::state_props::property(state, "copper_golem_pose")? {
        "standing" => "sitting",
        "sitting" => "running",
        "running" => "star",
        _ => "standing",
    };
    state_with(state, "copper_golem_pose", naechste)
}

// Stehendes Quellwasser (Fluids.WATER, nicht FLOWING_WATER): Wasser mit level 0.
pub fn is_water_source(state: u32) -> bool {
    block_registry::block_name(state) == Some("water") && world::state_props::property(state, "level") == Some("0")
}

// Eine einzelne Truhe, an die sich eine neue anschliessen koennte (s. facing_waterlogged_state).
pub fn is_single_chest(state: u32) -> bool {
    block_registry::block_name(state).is_some_and(|n| n == "chest" || n == "trapped_chest" || n.ends_with("copper_chest"))
        && world::state_props::property(state, "type") == Some("single")
}

// Saeulen (PillarBlock: Staemme, Holz, Basalt, Quarz- und Purpursaeule, Heuballen,
// Knochenblock ...): die Achse ist die der getroffenen Flaeche, sonst gibt es keine Property.
// Erkannt an genau drei States, die sich nur in `axis` unterscheiden. Ohne
// Vorhersage (etwa Junglestaemme auf den eigenen Fuessen: Ton, Animation, Grim-Flag) fehlte die Pruefung, ob der Block im Spieler steckt. None: keine solche Saeule.
pub fn axis_state(item_name: &str, face: i32) -> Option<u32> {
    let staende: Vec<u32> = (0..block_registry::state_count() as u32).filter(|&s| block_registry::block_name(s) == Some(item_name)).collect();
    if staende.len() != 3 || !staende.iter().all(|&s| world::state_props::property(s, "axis").is_some()) {
        return None;
    }
    let achse = match face {
        0 | 1 => "y",
        2 | 3 => "z",
        _ => "x",
    };
    staende.into_iter().find(|&s| world::state_props::property(s, "axis") == Some(achse))
}

// Steckte der Block in JEDER Form, die Vanilla beim Setzen waehlen kann, im Spieler? Fuer
// Bloecke ohne Vorhersage (Treppen, Zaeune): Vanilla setzt dann sicher nichts -- kein Ton,
// kein Armschwung, kein Item weg.
pub fn every_state_collides(item_name: &str, pos: (i32, i32, i32), ort: &PlaceSpot, shapes: &BlockShapes, spieler_min: [f64; 3], spieler_max: [f64; 3]) -> bool {
    let mut staende = placement_candidates(item_name, ort).into_iter().peekable();
    staende.peek().is_some() && staende.all(|s| collides_with_player(pos, s, shapes, spieler_min, spieler_max))
}

// Was getStateForPlacement ueber den Klick weiss (BlockPlaceContext).
pub struct PlaceSpot {
    // Getroffene Flaeche, Vanilla-Index (s. target_pos).
    pub face: i32,
    // Hoehe des Treffers ueber der Unterkante der ZIELzelle (clickLocation.y - pos.y).
    pub rel_y: f32,
    pub yaw_deg: f32,
    // Was vorher in der Zielzelle stand.
    pub vorher: u32,
}

// Die Zustaende, die getStateForPlacement liefern kann. Alle Zustaende des Blocks waeren zu
// viele: offene Falltueren und Zauntore, angebissene Kuchen, obere Stufen oder quer liegende
// Ketten stecken mitunter nicht im Spieler, gesetzt wird aber die eine Form, die der Klick
// bestimmt -- und die steckt drin. Mit allen Zustaenden kamen dort Ton und Armschwung ohne
// Block. Was nur an Redstone haengt (open/powered), gilt ohne Strom. Offen bleibt, was an den
// Nachbarn haengt (Verbindungen von Zaun, Scheibe, Gitter).
fn placement_candidates(item_name: &str, ort: &PlaceSpot) -> Vec<u32> {
    use world::state_props::property;
    let face = ort.face;
    // Schnee, Kerzen, Seegurken: auf denselben Block eins mehr, sonst eins.
    let gleicher = block_registry::block_name(ort.vorher) == Some(item_name);
    let anzahl = |k: &str| match property(ort.vorher, k) {
        Some(n) if gleicher => n.parse::<u32>().map_or(1, |n| n + 1).to_string(),
        _ => "1".to_string(),
    };
    let (lagen, kerzen, gurken) = (anzahl("layers"), anzahl("candles"), anzahl("pickles"));
    let flaeche = ["down", "up", "north", "south", "west", "east"].get(face as usize).copied().unwrap_or("up");
    let achse = match face {
        0 | 1 => "y",
        2 | 3 => "z",
        _ => "x",
    };
    // Blickrichtung des Spielers (getHorizontalDirection) und ihr Gegenteil.
    let blick = ((ort.yaw_deg / 90.0 + 0.5).floor() as i32).rem_euclid(4) as usize;
    let zum_spieler = ["north", "east", "south", "west"][blick];
    let weg = ["south", "west", "north", "east"][blick];
    // SlabBlock/StairBlock: unten, ausser an der Unterseite oder oberhalb der Mitte einer
    // Seite. TrapDoorBlock: an einer Seite nach der Hoehe, sonst nach der Flaeche.
    let unten = face != 0 && (face == 1 || ort.rel_y <= 0.5);
    let halb_unten = if face >= 2 { ort.rel_y <= 0.5 } else { face == 1 };
    let knospe = item_name.ends_with("amethyst_bud") || item_name == "amethyst_cluster";
    let stange = item_name.ends_with("lightning_rod") || item_name == "end_rod";
    let tuer = item_name.ends_with("_door");
    let falltuer = item_name.ends_with("_trapdoor");
    let stufe = item_name.ends_with("_slab");
    (0..block_registry::state_count() as u32)
        .filter(|&s| block_registry::block_name(s) == Some(item_name))
        .filter(|&s| {
            let gleich = |k: &str, v: &str| property(s, k).is_none_or(|x| x == v);
            gleich("open", "false")
                && gleich("powered", "false")
                && gleich("extended", "false")
                && gleich("bites", "0")
                && gleich("tilt", "none")
                && gleich("layers", &lagen)
                && gleich("candles", &kerzen)
                && gleich("pickles", &gurken)
                && (!item_name.ends_with("_wall") || gleich("up", "true"))
                && (!(knospe || stange || item_name == "ladder") || gleich("facing", flaeche))
                && (!item_name.ends_with("_fence_gate") || gleich("facing", weg))
                && gleich("eye", "false")
                && (!item_name.ends_with("chain") || gleich("axis", achse))
                && (item_name != "pale_moss_carpet" || gleich("bottom", "true"))
                && (!stufe || gleich("type", if gleicher { "double" } else if unten { "bottom" } else { "top" }))
                && (!falltuer || gleich("half", if halb_unten { "bottom" } else { "top" }))
                && (!tuer || (gleich("half", "lower") && gleich("facing", weg)))
                && (!item_name.ends_with("_shelf") || gleich("facing", zum_spieler))
        })
        .collect()
}

// Wohin der Block kommt: in den getroffenen Block selbst, falls der ersetzbar ist, sonst
// einen Schritt in Richtung der getroffenen Flaeche (Vanilla: ItemUsageContext.getBlockPos).
// `face` ist der Vanilla-Index (0=unten, 1=oben, 2=nord/-Z, 3=sued/+Z, 4=west/-X, 5=ost/+X).
pub fn target_pos(hit: (i32, i32, i32), face: i32, hit_block_name: Option<&str>) -> (i32, i32, i32) {
    if hit_block_name.is_some_and(is_replaceable) {
        return hit;
    }
    let (dx, dy, dz) = match face {
        0 => (0, -1, 0),
        1 => (0, 1, 0),
        2 => (0, 0, -1),
        3 => (0, 0, 1),
        4 => (-1, 0, 0),
        _ => (1, 0, 0),
    };
    (hit.0 + dx, hit.1 + dy, hit.2 + dz)
}

// Wuerde der Block in den Spieler hineinragen? Vanilla laesst dann gar nicht platzieren
// (World.canPlace -> ShapeContext.of(player) prueft die Entity-Kollision), und genau das
// muessen wir mitmachen: ein vorhergesagter Block IM Spieler schoebe unsere eigene Kollision
// weg und machte die Abweichung groesser statt kleiner.
#[allow(clippy::too_many_arguments)]
pub fn collides_with_player(pos: (i32, i32, i32), state: u32, shapes: &BlockShapes, spieler_min: [f64; 3], spieler_max: [f64; 3]) -> bool {
    // Die KOLLISIONSform wie Vanillas canPlace, nicht die Modellform: Truhen, Betten, Koepfe
    // und Statuen haben ein leeres Blockmodell (gezeichnet wird das Entity-Modell) und
    // steckten sonst nie im Spieler -- Ton und Armschwung kamen trotzdem.
    world::collision_boxes(state, shapes).iter().any(|shape| {
        let min = [pos.0 as f64 + shape.min[0] as f64, pos.1 as f64 + shape.min[1] as f64, pos.2 as f64 + shape.min[2] as f64];
        let max = [pos.0 as f64 + shape.max[0] as f64, pos.1 as f64 + shape.max[1] as f64, pos.2 as f64 + shape.max[2] as f64];
        (0..3).all(|i| min[i] < spieler_max[i] && max[i] > spieler_min[i])
    })
}

// --- Oeffnen/Schliessen per Hand (Tuer, Falltuer, Zauntor) ---
//
// Zweiter Fall derselben Luecke wie oben (eine Falltuer klappte erst nach der
// Serverantwort): Vanillas Client fuehrt die Benutzung
// SOFORT in der eigenen Welt aus (ClientPlayerInteractionManager.interactBlockInternal ->
// BlockState.onUse -> TrapdoorBlock.flip -> world.setBlockState, ohne jede isClient-Sperre),
// die Sequenznummer dient nur dazu, eine abgelehnte Benutzung spaeter zurueckzunehmen.
// GrimAC klappt seine Kopie ebenfalls sofort beim Empfang des Pakets um
// (CheckManagerListener -> CompensatedWorld.tickOpenable). Wer stattdessen auf das
// Block-Update des Servers wartet, rechnet eine volle Rundreise lang mit der ALTEN Form --
// beim Kriechen unter einer Falltuer genau die Zeit, in der die Pose umschlaegt.
//
// canOpenByHand() (BlockSetType): Eisentuer und Eisenfalltuer lassen sich NICHT von Hand
// oeffnen, alle anderen Holz-/Kupfer-/Bambus-Varianten schon.
pub fn is_hand_openable(name: &str) -> bool {
    if name == "iron_door" || name == "iron_trapdoor" {
        return false;
    }
    name.ends_with("_door") || name.ends_with("_trapdoor") || name.ends_with("_fence_gate")
}

// --- Reagiert dieser Block ueberhaupt auf einen Rechtsklick? ---
//
// Gebraucht fuer die Frage, ob der Arm schwingt (s. App::do_item_use): Vanilla schwingt NUR
// bei einer angenommenen Benutzung. Ein Rechtsklick auf Stein tut nichts und bewegt darum auch
// nichts -- weder in der eigenen Ansicht noch bei den anderen Spielern.
//
// Die Liste ist aus dem Jar abgeleitet, nicht aus dem Gedaechtnis: es sind genau die Bloecke,
// deren Klasse `onUse` oder `onUseWithItem` ueberschreibt (67 bzw. 27 Klassen in 1.21.11).
// Uebersetzt in Namen, weil dieser Client Bloecke nur ueber ihren Namen kennt.
//
// Was die Liste NICHT leisten kann: viele dieser Bloecke nehmen die Benutzung nur unter
// Bedingungen an (Kuchen nur bei Hunger, Komposter nur mit kompostierbarem Item, Kessel nur
// mit passendem Eimer). Ein Schwung zu viel ist an dieser Stelle das kleinere Uebel -- er
// entsteht nur bei einem Klick auf genau diese Bloecke, waehrend das frueher jeder
// Rechtsklick tat.
pub fn is_hand_interactive(name: &str) -> bool {
    // Nach Endung: die grossen Familien.
    const ENDUNGEN: &[&str] = &[
        "_door",
        "_trapdoor",
        "_fence_gate",
        "_button",
        "_bed",
        "_sign", // deckt auch _wall_sign und _hanging_sign ab
        "_shulker_box",
        "_candle", // und _candle_cake ueber die Liste unten
        "_cauldron",
        "_anvil", // chipped_/damaged_
        "_campfire",
        "_beehive",
    ];
    // Einzelne Bloecke.
    const NAMEN: &[&str] = &[
        "chest",
        "trapped_chest",
        "ender_chest",
        "barrel",
        "hopper",
        "dispenser",
        "dropper",
        "crafter",
        "furnace",
        "blast_furnace",
        "smoker",
        "brewing_stand",
        "beacon",
        "anvil",
        "crafting_table",
        "cartography_table",
        "smithing_table",
        "loom",
        "stonecutter",
        "grindstone",
        "enchanting_table",
        "lectern",
        "jukebox",
        "note_block",
        "bell",
        "composter",
        "cake",
        "candle_cake",
        "decorated_pot",
        "chiseled_bookshelf",
        "flower_pot",
        "cauldron",
        "campfire",
        "soul_campfire",
        // "respawn_anchor" steht hier BEWUSST NICHT: er ist der eine Block, dessen Annahme
        // wirklich vom Zustand abhaengt und nicht bloss einen Schwung zu viel kostet (s.
        // Kommentar oben). Ein UNGELADENER Anker gibt ohne Glowstone PASS zurueck -- dagegen
        // darf man ganz normal einen Block setzen. Die vollstaendige Fallunterscheidung steht
        // in App::resolve_anchor_use.
        "vault",
        "dragon_egg",
        "daylight_detector",
        "comparator",
        "repeater",
        "lever",
        "shelf",
        "beehive",
        "bee_nest",
        "redstone_ore",
        "deepslate_redstone_ore",
        "sweet_berry_bush",
        "cave_vines",
        "cave_vines_plant",
        "tnt",
        "pumpkin",
        "structure_block",
        "jigsaw",
        "command_block",
        "chain_command_block",
        "repeating_command_block",
        "copper_golem_statue",
    ];
    if NAMEN.contains(&name) {
        return true;
    }
    // Die Eisenvarianten von Tuer und Falltuer lassen sich NICHT von Hand oeffnen -- dieselbe
    // Ausnahme wie in is_hand_openable, und hier ebenso wichtig: ein Klick darauf tut nichts.
    if name == "iron_door" || name == "iron_trapdoor" {
        return false;
    }
    ENDUNGEN.iter().any(|e| name.ends_with(e))
}

// Derselbe Block mit GENAU EINER geaenderten Property, oder None (Property fehlt am Block /
// den Zielwert gibt es nicht). Entspricht Vanillas `state.with(PROPERTY, wert)`.
//
// Gesucht wird im zusammenhaengenden State-Bereich des Blocks: Minecraft vergibt die
// State-IDs eines Blocks am Stueck, ein Lauf nach oben und unten vom aktuellen State aus
// deckt sie also vollstaendig ab, ohne die gesamte Tabelle zu durchsuchen.
pub fn state_with(state: u32, key: &str, value: &str) -> Option<u32> {
    let name = block_registry::block_name(state)?;
    world::state_props::property(state, key)?; // der Block muss die Property ueberhaupt haben
    let passt = |kandidat: u32| -> bool {
        world::state_props::property(kandidat, key) == Some(value)
            && world::state_props::properties_of(kandidat)
                .filter(|(k, _)| *k != key)
                .eq(world::state_props::properties_of(state).filter(|(k, _)| *k != key))
    };
    let mut kandidat = state;
    while kandidat > 0 && block_registry::block_name(kandidat - 1) == Some(name) {
        kandidat -= 1;
    }
    let ende = block_registry::state_count() as u32;
    while kandidat < ende && block_registry::block_name(kandidat) == Some(name) {
        if passt(kandidat) {
            return Some(kandidat);
        }
        kandidat += 1;
    }
    None
}

// Derselbe Block mit umgeklapptem `open`, oder None (keine open-Property / nicht gefunden).
pub fn toggled_open_state(state: u32) -> Option<u32> {
    let ziel = match world::state_props::property(state, "open")? {
        "true" => "false",
        _ => "true",
    };
    state_with(state, "open", ziel)
}

// --- Eimer (BucketItem.use / placeFluid) ---
//
// Der Client SETZT die Fluessigkeit in Vanilla selbst: placeFluid laeuft innerhalb von
// sendSequencedPacket und ruft `world.setBlockState(pos, fluid.getDefaultState()...)` ohne
// isClient-Sperre -- genau wie eine Blockplatzierung, zurueckgenommen ueber dieselbe
// Sequenznummer. Der Ablauf, auf das Vorhersagbare reduziert:
//
//   Strahl ignoriert Fluessigkeiten (FluidHandling.NONE) -> getroffener Block + Seite.
//   Ziel = getroffener Block, wenn er Wasser aufnehmen kann (FluidFillable) und es Wasser ist,
//          sonst der Nachbar auf der getroffenen Seite.
//   placeFluid(ziel):
//     bl  = canBucketPlace        (hier: Luft oder ersetzbar -- s. is_replaceable)
//     bl3 = bl || FluidFillable.canFillWithFluid   (waterlogged=false, nur Wasser)
//     bl4 = Luft || bl3 && (!schleichend || zweiter Versuch)
//     !bl4 -> ein zweiter Versuch an getroffen+Seite, ohne Schleich-Sperre
//     FluidFillable && Wasser -> waterlogged=true
//     sonst                   -> Quellblock (level=0)
//
// Nicht vorhergesagt: das Verdampfen im Nether (die Dimension kennt der Client nicht) und
// Sonderfaelle von canBucketPlace ausserhalb der ersetzbaren Bloecke -- dort setzt der Server
// allein, der Ack gleicht ab. Rueckgabe: (Position, neuer Zustand).
pub fn bucket_place(
    hit: (i32, i32, i32),
    face: i32,
    fluid: &str,
    sneaking: bool,
    block_at: impl Fn(i32, i32, i32) -> u32,
) -> Option<((i32, i32, i32), u32)> {
    let wasser = fluid == "water";
    let nachbar = target_pos(hit, face, None);
    let fuellbar = |state: u32| wasser && world::state_props::property(state, "waterlogged") == Some("false");
    let getroffen = block_at(hit.0, hit.1, hit.2);
    let erster = if fuellbar(getroffen) { hit } else { nachbar };
    for (pos, zweiter_versuch) in [(erster, false), (nachbar, true)] {
        let state = block_at(pos.0, pos.1, pos.2);
        let name = block_registry::block_name(state).unwrap_or("air");
        let luft = matches!(name, "air" | "cave_air" | "void_air");
        let bl3 = luft || is_replaceable(name) || fuellbar(state);
        let bl4 = luft || bl3 && (!sneaking || zweiter_versuch);
        if !bl4 {
            continue;
        }
        let neu = if fuellbar(state) { state_with(state, "waterlogged", "true")? } else { fluid_source_state(fluid)? };
        // setBlockState liefert false, wenn sich nichts aendert -- dann gibt es nichts
        // vorherzusagen.
        return (neu != state).then_some((pos, neu));
    }
    None
}

// Quellblock einer Fluessigkeit: level=0.
pub fn fluid_source_state(fluid: &str) -> Option<u32> {
    (0..block_registry::state_count() as u32)
        .find(|&s| block_registry::block_name(s) == Some(fluid) && world::state_props::property(s, "level") == Some("0"))
}

// --- Seelenanker (respawn_anchor) ---
//
// Vanillas MAX_CHARGES. Ein voller Anker nimmt kein Glowstone mehr an (`canCharge(state)` ist
// `state.get(CHARGES) < 4`).
pub const ANCHOR_MAX_CHARGES: u8 = 4;

// Der Anker eine Ladestufe hoeher, oder None (kein Anker / schon voll). Das ist Vanillas
// RespawnAnchorBlock.charge:
//
//     BlockState blockState = state.with(CHARGES, state.get(CHARGES) + 1);
//     world.setBlockState(pos, blockState, Block.NOTIFY_ALL);
//
// Wichtig ist, was dort NICHT steht: keine isClient-Sperre. Der Client laedt den Anker also
// selbst sofort hoch, genau wie er eine Falltuer selbst umklappt (s. is_hand_openable) -- die
// Sequenznummer dient nur dazu, eine abgelehnte Benutzung spaeter zurueckzunehmen.
pub fn charged_anchor_state(state: u32) -> Option<u32> {
    if block_registry::block_name(state)? != "respawn_anchor" {
        return None;
    }
    let charges: u8 = world::state_props::property(state, "charges")?.parse().ok()?;
    if charges >= ANCHOR_MAX_CHARGES {
        return None;
    }
    state_with(state, "charges", &(charges + 1).to_string())
}

// Die zweite Haelfte einer Tuer (unten <-> oben), sofern der getroffene Block eine Tuer ist.
//
// Vanillas Client klappt streng genommen nur die angeklickte Haelfte um (setBlockState mit
// NOTIFY_LISTENERS, ohne Nachbar-Update) -- der SERVER klappt beide, und genau dessen Welt
// bildet GrimAC nach (CompensatedWorld.tickOpenable fasst beide Haelften an). Weil unsere
// Vorhersage dazu da ist, waehrend der Rundreise mit dem Server/Grim uebereinzustimmen,
// folgen wir hier bewusst dem Server-Verhalten; das Block-Update raeumt es ohnehin gleich
// darauf endgueltig auf.
pub fn door_other_half(name: &str, pos: (i32, i32, i32), state: u32) -> Option<(i32, i32, i32)> {
    if !name.ends_with("_door") {
        return None;
    }
    let dy = if world::state_props::property(state, "half") == Some("lower") { 1 } else { -1 };
    Some((pos.0, pos.1 + dy, pos.2))
}

#[cfg(test)]
mod interaktion_tests {
    use super::*;

    // Jeder Name aus der Liste muss es in der Blockregistry auch geben. Ein Tippfehler waere
    // sonst unsichtbar -- er wuerde einfach nie greifen, und der Arm bliebe an genau diesem
    // Block still.
    #[test]
    fn jeder_gelistete_block_existiert_wirklich() {
        for name in [
            "chest",
            "barrel",
            "furnace",
            "crafting_table",
            "enchanting_table",
            "lectern",
            "note_block",
            "bell",
            "composter",
            "cake",
            "flower_pot",
            "cauldron",
            "lever",
            "comparator",
            "repeater",
            "beehive",
            "bee_nest",
            "sweet_berry_bush",
            "dragon_egg",
            "candle_cake",
            "soul_campfire",
            "deepslate_redstone_ore",
        ] {
            assert!(block_registry::is_block(name), "{name} steht nicht in der Blockregistry");
            assert!(is_hand_interactive(name), "{name} muss als benutzbar gelten");
        }
        // Der Seelenanker stand frueher in derselben Liste. Er ist immer noch ein echter
        // Block, gilt hier aber bewusst nicht mehr als pauschal benutzbar -- s.
        // tests::anker_ist_nicht_pauschal_handbenutzbar.
        assert!(block_registry::is_block("respawn_anchor"));
    }

    #[test]
    fn endungen_greifen_und_gewoehnliche_bloecke_nicht() {
        for name in
            ["oak_door", "birch_trapdoor", "spruce_fence_gate", "stone_button", "red_bed", "oak_sign", "oak_wall_sign", "blue_shulker_box"]
        {
            assert!(is_hand_interactive(name), "{name} muss als benutzbar gelten");
        }
        // Der Alltag: darauf zu klicken tut nichts, und genau deshalb darf der Arm nicht
        // schwingen.
        for name in ["stone", "dirt", "grass_block", "obsidian", "oak_planks", "diamond_block", "cobblestone"] {
            assert!(!is_hand_interactive(name), "{name} darf nicht als benutzbar gelten");
        }
    }

    // Dieselbe Ausnahme wie beim Oeffnen von Hand: Eisen geht nur ueber Redstone.
    #[test]
    fn eisentuer_und_eisenfalltuer_reagieren_nicht_auf_die_hand() {
        assert!(!is_hand_interactive("iron_door"));
        assert!(!is_hand_interactive("iron_trapdoor"));
        assert!(is_hand_interactive("copper_door"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keramik_und_ofen_schauen_zum_spieler() {
        let facing = |item: &str, yaw: f32| {
            let state = facing_state(item, yaw).flatten().unwrap_or_else(|| panic!("{item} ohne State"));
            world::state_props::property(state, "facing").unwrap()
        };
        // Blick nach Sueden (yaw 0) -> Vorderseite nach Norden, zum Spieler hin.
        assert_eq!(facing("yellow_glazed_terracotta", 0.0), "north");
        assert_eq!(facing("yellow_glazed_terracotta", 90.0), "east");
        assert_eq!(facing("yellow_glazed_terracotta", -170.0), "south");
        assert_eq!(facing("carved_pumpkin", 260.0), "west");
        let ofen = facing_state("furnace", 0.0).flatten().unwrap();
        assert_eq!(world::state_props::property(ofen, "lit"), Some("false"));
        for item in ["jack_o_lantern", "loom", "stonecutter", "smoker", "blast_furnace"] {
            facing(item, 45.0);
        }
        assert!(facing_state("yellow_wool", 0.0).is_none());
    }

    #[test]
    fn ersetzbare_bloecke_werden_ueberbaut() {
        // Gras wird ersetzt -- der Block landet AUF derselben Stelle, nicht davor.
        assert_eq!(target_pos((5, 10, 5), 1, Some("short_grass")), (5, 10, 5));
        // Stein nicht -- der Block landet auf der getroffenen Seite.
        assert_eq!(target_pos((5, 10, 5), 1, Some("stone")), (5, 11, 5));
        assert_eq!(target_pos((5, 10, 5), 0, Some("stone")), (5, 9, 5));
        assert_eq!(target_pos((5, 10, 5), 4, Some("stone")), (4, 10, 5));
        assert_eq!(target_pos((5, 10, 5), 5, Some("stone")), (6, 10, 5));
        assert_eq!(target_pos((5, 10, 5), 2, Some("stone")), (5, 10, 4));
        assert_eq!(target_pos((5, 10, 5), 3, Some("stone")), (5, 10, 6));
    }

    // Einstatige Bloecke bekommen eine Vorhersage, mehrstatige bewusst nicht (Ausrichtung).
    #[test]
    fn fackel_an_der_wand_wird_wandfackel() {
        let wand = torch_state("torch", 2).unwrap().unwrap();
        assert_eq!(block_registry::block_name(wand), Some("wall_torch"));
        assert_eq!(world::state_props::property(wand, "facing"), Some("north"));
        let seele = torch_state("soul_torch", 5).unwrap().unwrap();
        assert_eq!(block_registry::block_name(seele), Some("soul_wall_torch"));
        assert_eq!(world::state_props::property(seele, "facing"), Some("east"));
        assert_eq!(block_registry::block_name(torch_state("torch", 1).unwrap().unwrap()), Some("torch"));
        assert_eq!(torch_state("torch", 0), Some(None));
        assert_eq!(torch_state("stone", 2), None);
    }

    #[test]
    fn nur_eindeutige_bloecke_werden_vorhergesagt() {
        let obsidian = default_state_for_item("obsidian").expect("Obsidian hat genau einen State");
        assert_eq!(block_registry::block_name(obsidian), Some("obsidian"));
        assert!(default_state_for_item("oak_stairs").is_none(), "Treppen haben eine Ausrichtung");
        assert!(default_state_for_item("diamond_sword").is_none(), "kein Block");
    }

    #[test]
    fn truhen_schauen_zum_spieler() {
        let p = |s: u32, k: &str| world::state_props::property(s, k);
        // Blick nach Norden (yaw 180): die Truhe schaut nach Sueden, einzeln, trocken.
        let s = facing_waterlogged_state("chest", 180.0, false, false).flatten().expect("Truhe");
        assert_eq!((p(s, "facing"), p(s, "type"), p(s, "waterlogged")), (Some("south"), Some("single"), Some("false")));
        let n = facing_waterlogged_state("ender_chest", 0.0, true, true).flatten().expect("Endertruhe");
        assert_eq!((p(n, "facing"), p(n, "waterlogged")), (Some("north"), Some("true")));
        assert_eq!(facing_waterlogged_state("trapped_chest", 0.0, false, true), Some(None), "koennte sich anschliessen");
        let g = facing_waterlogged_state("copper_golem_statue", 90.0, false, false).flatten().expect("Statue");
        assert_eq!((p(g, "facing"), p(g, "copper_golem_pose")), (Some("east"), Some("standing")));
        assert!(facing_waterlogged_state("stone", 0.0, false, false).is_none());
        // Haltungen der Reihe nach, mit leerer Hand und Block; die Axt schaltet nichts.
        let mut haltungen = vec![];
        let mut z = g;
        for _ in 0..4 {
            z = next_statue_pose_state(z, None).expect("Statue");
            haltungen.push(p(z, "copper_golem_pose").unwrap());
            assert_eq!((p(z, "facing"), block_registry::block_name(z)), (Some("east"), Some("copper_golem_statue")));
        }
        assert_eq!(haltungen, ["sitting", "running", "star", "standing"]);
        assert!(next_statue_pose_state(g, Some("stone")).is_some());
        assert!(next_statue_pose_state(g, Some("iron_axe")).is_none());
        assert!(next_statue_pose_state(g, Some("iron_pickaxe")).is_some());
        // Leeres Blockmodell (wie bei uns die Truhe): in den Fuessen zaehlt trotzdem die
        // Kollisionsform.
        let leer = BlockShapes::new(vec![Some(Vec::new()); block_registry::state_count()]);
        assert!(collides_with_player((5, 10, 5), s, &leer, [5.3, 10.0, 5.3], [5.9, 11.8, 5.9]), "Truhe in den Füßen");
        assert!(collides_with_player((5, 10, 5), g, &leer, [5.3, 10.0, 5.3], [5.9, 11.8, 5.9]), "Statue in den Füßen");
    }

    #[test]
    fn saeulen_nehmen_die_achse_der_flaeche() {
        for (face, achse) in [(0, "y"), (1, "y"), (2, "z"), (3, "z"), (4, "x"), (5, "x")] {
            let s = axis_state("jungle_log", face).expect("Dschungelstamm ist eine Säule");
            assert_eq!(block_registry::block_name(s), Some("jungle_log"));
            assert_eq!(world::state_props::property(s, "axis"), Some(achse));
        }
        assert!(axis_state("hay_block", 1).is_some());
        assert!(axis_state("yellow_wool", 1).is_none(), "keine Achse");
        assert!(axis_state("oak_stairs", 1).is_none(), "mehr als nur die Achse");
    }

    // Ein Vollwuerfel genau dort, wo der Spieler steht, darf nicht vorhergesagt werden.
    #[test]
    fn block_im_spieler_wird_erkannt() {
        let shapes = BlockShapes::all_full_cubes();
        let min = [5.3, 10.0, 5.3];
        let max = [5.9, 11.8, 5.9];
        assert!(collides_with_player((5, 10, 5), 1, &shapes, min, max));
        assert!(collides_with_player((5, 11, 5), 1, &shapes, min, max), "Kopfhöhe zählt mit");
        assert!(!collides_with_player((5, 12, 5), 1, &shapes, min, max));
        assert!(!collides_with_player((6, 10, 5), 1, &shapes, min, max));
    }

    // Falltuer auf/zu: derselbe Block, nur `open` gedreht, alle anderen Eigenschaften gleich.
    #[test]
    fn falltuer_wird_umgeklappt() {
        let zu = (0..block_registry::state_count() as u32)
            .find(|&s| {
                block_registry::block_name(s) == Some("oak_trapdoor")
                    && world::state_props::property(s, "open") == Some("false")
                    && world::state_props::property(s, "half") == Some("bottom")
                    && world::state_props::property(s, "facing") == Some("north")
                    && world::state_props::property(s, "waterlogged") == Some("false")
                    && world::state_props::property(s, "powered") == Some("false")
            })
            .expect("geschlossene Eichen-Falltür");
        let auf = toggled_open_state(zu).expect("offener Gegenpart");
        assert_eq!(block_registry::block_name(auf), Some("oak_trapdoor"));
        assert_eq!(world::state_props::property(auf, "open"), Some("true"));
        for schluessel in ["half", "facing", "waterlogged", "powered"] {
            assert_eq!(
                world::state_props::property(auf, schluessel),
                world::state_props::property(zu, schluessel),
                "{schluessel} darf sich nicht mitaendern"
            );
        }
        // Und zurueck ist wieder der Ausgangszustand -- sonst driftet ein Auf/Zu-Paar weg.
        assert_eq!(toggled_open_state(auf), Some(zu));
    }

    #[test]
    fn eisenvarianten_lassen_sich_nicht_von_hand_oeffnen() {
        assert!(is_hand_openable("oak_trapdoor"));
        assert!(is_hand_openable("copper_door"));
        assert!(is_hand_openable("spruce_fence_gate"));
        assert!(!is_hand_openable("iron_door"), "Eisentür braucht Redstone");
        assert!(!is_hand_openable("iron_trapdoor"), "Eisenfalltür braucht Redstone");
        assert!(!is_hand_openable("stone"));
    }

    // Die zweite Tuerhaelfte liegt oben, wenn man unten klickt -- und umgekehrt.
    #[test]
    fn tuer_kennt_ihre_zweite_haelfte() {
        let unten = (0..block_registry::state_count() as u32)
            .find(|&s| block_registry::block_name(s) == Some("oak_door") && world::state_props::property(s, "half") == Some("lower"))
            .expect("untere Türhälfte");
        let oben = (0..block_registry::state_count() as u32)
            .find(|&s| block_registry::block_name(s) == Some("oak_door") && world::state_props::property(s, "half") == Some("upper"))
            .expect("obere Türhälfte");
        assert_eq!(door_other_half("oak_door", (3, 64, 7), unten), Some((3, 65, 7)));
        assert_eq!(door_other_half("oak_door", (3, 65, 7), oben), Some((3, 64, 7)));
        assert_eq!(door_other_half("oak_trapdoor", (3, 64, 7), unten), None, "Falltüren haben keine zweite Hälfte");
    }

    fn anker_mit(charges: u8) -> u32 {
        (0..block_registry::state_count() as u32)
            .find(|&s| {
                block_registry::block_name(s) == Some("respawn_anchor")
                    && world::state_props::property(s, "charges") == Some(charges.to_string().as_str())
            })
            .expect("Seelenanker-State")
    }

    // Frueher wurde mit Glowstone auf einen Anker geklickt bloss Glowstone
    // daneben gesetzt. Die Ladestufe muss stattdessen um EINS steigen -- und bei 4 aufhoeren,
    // genau wie Vanillas canCharge(state) = `state.get(CHARGES) < 4`.
    #[test]
    fn anker_laedt_bis_vier_und_dann_nicht_weiter() {
        for stufe in 0..ANCHOR_MAX_CHARGES {
            let vorher = anker_mit(stufe);
            let nachher = charged_anchor_state(vorher).expect("ein nicht voller Anker muss ladbar sein");
            assert_eq!(
                world::state_props::property(nachher, "charges"),
                Some((stufe + 1).to_string().as_str()),
                "Stufe {stufe} muss auf {} steigen",
                stufe + 1
            );
            assert_eq!(block_registry::block_name(nachher), Some("respawn_anchor"));
        }
        assert_eq!(charged_anchor_state(anker_mit(ANCHOR_MAX_CHARGES)), None, "ein voller Anker nimmt nichts mehr an");
    }

    // Gegenprobe: charged_anchor_state darf nicht auf irgendeinen anderen Block anspringen,
    // auch nicht auf einen mit einer "charges"-Property (die hat sonst nur die Kerze nicht --
    // aber der Namensvergleich ist die eigentliche Sperre).
    #[test]
    fn nur_der_anker_laesst_sich_laden() {
        for name in ["stone", "glowstone", "oak_door", "chest"] {
            let state = (0..block_registry::state_count() as u32).find(|&s| block_registry::block_name(s) == Some(name)).expect("State");
            assert_eq!(charged_anchor_state(state), None, "{name} ist kein Seelenanker");
        }
    }

    // Der Anker darf NICHT in is_hand_interactive stehen: ein ungeladener Anker gibt ohne
    // Glowstone PASS zurueck, dagegen setzt man ganz normal einen Block. Seine eigene
    // Fallunterscheidung steht in App::resolve_anchor_use.
    #[test]
    fn anker_ist_nicht_pauschal_handbenutzbar() {
        assert!(!is_hand_interactive("respawn_anchor"));
        // Die Nachbarn in derselben Liste bleiben aber drin -- die nehmen wirklich immer an.
        assert!(is_hand_interactive("chest"));
        assert!(is_hand_interactive("vault"));
        assert!(is_hand_interactive("soul_campfire"));
    }

    // state_with ist seit dem Anker gemeinsame Grundlage von toggled_open_state und
    // charged_anchor_state -- es muss GENAU eine Property aendern und alle anderen behalten.
    #[test]
    fn eimer_setzt_quelle_vor_die_getroffene_flaeche_und_fuellt_stufen() {
        let stein = (0..block_registry::state_count() as u32).find(|&s| block_registry::block_name(s) == Some("stone")).unwrap();
        let stufe = (0..block_registry::state_count() as u32)
            .find(|&s| {
                block_registry::block_name(s) == Some("oak_slab")
                    && world::state_props::property(s, "waterlogged") == Some("false")
                    && world::state_props::property(s, "type") == Some("bottom")
            })
            .unwrap();
        // Stein bei y=0, Luft sonst: Wasser auf die Oberseite -> Quelle bei y=1.
        let welt = |_x: i32, y: i32, _z: i32| if y == 0 { stein } else { world::STATE_AIR };
        let (pos, neu) = bucket_place((0, 0, 0), 1, "water", false, welt).unwrap();
        assert_eq!(pos, (0, 1, 0));
        assert_eq!(neu, fluid_source_state("water").unwrap());
        // Lava genauso.
        assert_eq!(bucket_place((0, 0, 0), 1, "lava", false, welt).unwrap().1, fluid_source_state("lava").unwrap());
        // Stufe getroffen: Wasser geht IN die Stufe, Lava davor.
        let welt = |_x: i32, y: i32, _z: i32| if y == 0 { stufe } else { world::STATE_AIR };
        let (pos, neu) = bucket_place((0, 0, 0), 1, "water", false, welt).unwrap();
        assert_eq!(pos, (0, 0, 0));
        assert_eq!(world::state_props::property(neu, "waterlogged"), Some("true"));
        assert_eq!(bucket_place((0, 0, 0), 1, "lava", false, welt).unwrap().0, (0, 1, 0));
        // Fester Block vor der Flaeche: nichts.
        let welt = |_x: i32, _y: i32, _z: i32| stein;
        assert!(bucket_place((0, 0, 0), 1, "water", false, welt).is_none());
    }

    #[test]
    fn state_with_aendert_genau_eine_property() {
        let tuer = (0..block_registry::state_count() as u32)
            .find(|&s| {
                block_registry::block_name(s) == Some("oak_door")
                    && world::state_props::property(s, "open") == Some("false")
                    && world::state_props::property(s, "facing") == Some("north")
            })
            .expect("Eichentür");
        let offen = state_with(tuer, "open", "true").expect("offene Variante");
        assert_eq!(world::state_props::property(offen, "open"), Some("true"));
        assert_eq!(world::state_props::property(offen, "facing"), Some("north"), "facing darf sich nicht mitaendern");
        // Eine Property, die es an diesem Block gar nicht gibt, ergibt None statt eines
        // zufaelligen Nachbarstates.
        assert_eq!(state_with(tuer, "charges", "1"), None);
        // Ebenso ein Wert, den die Property nicht annehmen kann.
        assert_eq!(state_with(tuer, "open", "vielleicht"), None);
    }

    // Der Anker wird mit charges=0 vorhergesagt, obwohl er mehrere States hat.
    #[test]
    fn anker_wird_leer_vorhergesagt() {
        let state = default_state_for_item("respawn_anchor").expect("Anker muss vorhersagbar sein");
        assert_eq!(block_registry::block_name(state), Some("respawn_anchor"));
        assert_eq!(world::state_props::property(state, "charges"), Some("0"));
    }
}

#[cfg(test)]
mod fuesse_tests {
    use super::*;

    // Kein Block-Item ohne Vorhersage, bei dem es vom Zufall abhaengt, ob er in einem mittig
    // stehenden Spieler steckt: die Kandidaten stecken alle drin oder keiner. Sonst kaeme
    // wieder Ton und Armschwung ohne Block (gemeldet fuer Truhen, Staemme, Falltueren ...).
    #[test]
    fn kein_block_unsicher_in_den_fuessen() {
        let leer = BlockShapes::new(vec![Some(Vec::new()); block_registry::state_count()]);
        let (min, max) = ([0.2, 0.0, 0.2], [0.8, 1.8, 0.8]);
        let mut unsicher = Vec::new();
        for id in 0..world::item_registry::item_count() as u32 {
            let Some(item) = world::item_registry::item_name(id) else { continue };
            if !block_registry::is_block(item) {
                continue;
            }
            for (face, yaw) in [(1, 0.0), (0, 90.0), (2, 180.0), (5, 270.0)] {
                let vorhergesagt = torch_state(item, face)
                    .or_else(|| facing_state(item, yaw))
                    .or_else(|| facing_waterlogged_state(item, yaw, false, false))
                    .unwrap_or_else(|| axis_state(item, face).or_else(|| default_state_for_item(item)));
                if vorhergesagt.is_some() {
                    continue;
                }
                let staende = placement_candidates(item, &PlaceSpot { face, rel_y: 0.3, yaw_deg: yaw, vorher: world::STATE_AIR });
                let drin = staende.iter().filter(|&&s| collides_with_player((0, 0, 0), s, &leer, min, max)).count();
                if drin > 0 && drin < staende.len() {
                    unsicher.push(format!("{item} (Fläche {face})"));
                }
            }
        }
        assert!(unsicher.is_empty(), "unsicher: {unsicher:?}");
    }

    // Stufe/Falltuer nach Trefferhoehe, Kette nach Flaeche, Tuer in Blickrichtung.
    #[test]
    fn kandidaten_nach_dem_klick() {
        let p = |s: u32, k: &str| world::state_props::property(s, k).unwrap().to_string();
        let ort = |face, rel_y, yaw_deg| PlaceSpot { face, rel_y, yaw_deg, vorher: world::STATE_AIR };
        let alle = |item: &str, o: &PlaceSpot, k: &str| {
            let mut v: Vec<String> = placement_candidates(item, o).into_iter().map(|s| p(s, k)).collect();
            v.sort();
            v.dedup();
            v
        };
        assert_eq!(alle("oak_slab", &ort(2, 0.7, 0.0), "type"), ["top"]);
        assert_eq!(alle("oak_slab", &ort(2, 0.3, 0.0), "type"), ["bottom"]);
        assert_eq!(alle("oak_slab", &ort(0, 0.0, 0.0), "type"), ["top"]);
        assert_eq!(alle("oak_trapdoor", &ort(1, 0.0, 0.0), "half"), ["bottom"]);
        assert_eq!(alle("oak_trapdoor", &ort(4, 0.8, 0.0), "half"), ["top"]);
        assert_eq!(alle("oak_trapdoor", &ort(4, 0.8, 0.0), "open"), ["false"]);
        assert_eq!(alle("iron_chain", &ort(5, 0.5, 0.0), "axis"), ["x"]);
        assert_eq!(alle("oak_door", &ort(1, 0.0, 90.0), "facing"), ["west"]);
        assert_eq!(alle("oak_door", &ort(1, 0.0, 90.0), "half"), ["lower"]);
        assert_eq!(alle("oak_fence_gate", &ort(1, 0.0, 180.0), "facing"), ["north"]);
    }
}
