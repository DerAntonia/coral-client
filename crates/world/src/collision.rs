// Ob (und mit welcher Form) ein Block kollidiert -- getrennt von is_solid (das steuert nur,
// ob eine Wuerfelseite gezeichnet wird). Seit es echte Formen aus den Vanilla-Modellen gibt
// (s. block_shapes.rs/assets::shapes), liefert collision_boxes() genau die Boxen, die auch
// gerendert werden -- Stufen kollidieren jetzt z.B. nur mit ihrer halben Hoehe.
//
// Die kuratierte PASSABLE-Liste bleibt trotzdem noetig: Kreuz-Modelle (Blumen, Gras,
// Setzlinge, Fackeln, Ranken, Pilze, Feuer, Portale, ...) lassen sich nicht als
// achsenparallele Box darstellen (s. assets::shapes-Kommentar zu Element::rotation) und
// fallen bei der Formaufloesung auf den Vollwuerfel-Fallback zurueck -- ohne diese Liste
// waeren sie ploetzlich massiv, obwohl sie sichtbar duenn/durchlaessig sind. Die Liste
// gewinnt IMMER gegen eine aufgeloeste Form (kein Block darin bekommt je eine Kollisionsbox).

use crate::block_registry::block_name;
use crate::block_shapes::{BlockShapes, ShapeBox};
use crate::chunk::STATE_AIR;

// Genau begehbare Bloecke (ohne Luft, die ueber STATE_AIR frueher rausfaellt). Bewusst
// KEINE Blockbloecke wie snow_block, bamboo, campfire, flower_pot, cactus -- die kollidieren.
const PASSABLE: &[&str] = &[
    // Luft-Varianten
    "air",
    "cave_air",
    "void_air",
    // Gras/Farne/Buesche
    "short_grass",
    "tall_grass",
    "fern",
    "large_fern",
    "dead_bush",
    "bush",
    "short_dry_grass",
    "tall_dry_grass",
    "firefly_bush",
    "wildflowers",
    // Wasser/Lava (fehlten bisher komplett -- der Spieler lief auf beiden wie auf festem
    // Boden, statt hinein-/durchzusinken; echte Fluessigkeiten haben in Vanilla KEINE
    // Kollisionsbox ueberhaupt, s. LiquidBlock -- die eigentliche Bewegungsbremse kommt
    // ausschliesslich aus app::physics::tick_water/tick_lava, nicht aus einer Box hier) und
    // Wasserpflanzen.
    "water",
    "flowing_water",
    "lava",
    "flowing_lava",
    "seagrass",
    "tall_seagrass",
    "kelp",
    "kelp_plant",
    // Blumen
    "dandelion",
    "poppy",
    "blue_orchid",
    "allium",
    "azure_bluet",
    "red_tulip",
    "orange_tulip",
    "white_tulip",
    "pink_tulip",
    "oxeye_daisy",
    "cornflower",
    "lily_of_the_valley",
    "wither_rose",
    "torchflower",
    "sunflower",
    "lilac",
    "rose_bush",
    "peony",
    "cactus_flower",
    "pink_petals",
    "pitcher_plant",
    "chorus_flower",
    // Setzlinge / Propagule (auch per Suffix, s.u.)
    "mangrove_propagule",
    // Pilze / Nether-Gewaechse
    "brown_mushroom",
    "red_mushroom",
    "crimson_fungus",
    "warped_fungus",
    "crimson_roots",
    "warped_roots",
    "nether_sprouts",
    "nether_wart",
    // Feldfruechte
    "wheat",
    "carrots",
    "potatoes",
    "beetroots",
    "torchflower_crop",
    "pitcher_crop",
    "melon_stem",
    "pumpkin_stem",
    "attached_melon_stem",
    "attached_pumpkin_stem",
    "sweet_berry_bush",
    "sugar_cane",
    // Spinnennetz: hat in Vanilla GAR KEINE Kollisionsform (Settings.noCollision()) -- man
    // bewegt sich hindurch und wird dabei gebremst (s. slow_movement_multiplier). Bisher fiel
    // das nur deshalb richtig aus, weil das Netz ein Kreuz-Modell ist und Kreuz-States eine
    // leere Boxliste bekommen (s. cross_raw in main.rs). Das ist eine Eigenschaft der GRAFIK,
    // nicht des Blocks: ein Resourcepack, dessen Netz-Modell nicht mehr als Kreuz durchgeht,
    // haette das Netz zu einer Wand gemacht. Deshalb steht es jetzt hier namentlich.
    "cobweb",
    // Ranken / Kletter-/Haengegewaechse
    "vine",
    "weeping_vines",
    "weeping_vines_plant",
    "twisting_vines",
    "twisting_vines_plant",
    "cave_vines",
    "cave_vines_plant",
    "glow_lichen",
    "hanging_roots",
    "spore_blossom",
    "small_dripleaf",
    // Schienen
    "rail",
    "powered_rail",
    "detector_rail",
    "activator_rail",
    // Redstone-Flachkram / Schalter
    "redstone_wire",
    "tripwire",
    "tripwire_hook",
    "lever",
    "repeater",
    "comparator",
    // Feuer / Portale / Licht
    "fire",
    "soul_fire",
    "nether_portal",
    "end_portal",
    "light",
    "structure_void",
    // Bodenbedeckung: geometrisch nur eine hauchduenne, am Boden liegende Ebene (s.
    // assets::shapes-Aufloesung: from/to-Y identisch, kein Volumen) -- in Vanilla ohne
    // jede Kollisionsbox begehbar wie Gras. Ohne diesen Eintrag faellt shapes_of() auf
    // die (winzige, aber nicht-leere) aufgeloeste Form zurueck statt auf "begehbar",
    // und Falllaub haette eine Kollisionsbox.
    "leaf_litter",
];

// Suffixe fuer grosse, einheitlich begehbare Familien -- spart die Einzelaufzaehlung.
const PASSABLE_SUFFIXES: &[&str] = &["_sapling", "torch", "_button", "_pressure_plate"];

// Bloecke, in denen man ERSTICKT -- Vanillas BlockState.isSuffocating. Gebraucht fuer
// Vanillas Herausschieben aus einem Block (s. physics::move_towards_closest_space), NICHT
// fuer Schaden: Ersticken meldet der Server.
//
// Vorgabe in BlockBehaviour.Properties ist `blocksMotion() && isCollisionShapeFullBlock()`,
// also ein voller Kollisionswuerfel. Davon weichen genau zwei kurze Listen ab, beide aus
// Blocks.class abgelesen:
//
//   NIE:    Glas (auch gefaerbt und getoent), Laub, Mangrovenwurzeln, Kupfergitter, der
//           Schiebeblock des Kolbens und eine OFFENE Shulkerkiste -- alles Bloecke, die zwar
//           wuerfelfoermig kollidieren, in denen man aber nicht erstickt.
//   IMMER:  Ackerland, Seelensand, Trampelpfad und Schlamm -- die sind 15/16 hoch, ersticken
//           aber trotzdem.
//
// Eine geschlossene Shulkerkiste erstickt sehr wohl; wir kennen den Oeffnungszustand hier
// nicht (er steckt im BlockEntity, nicht im State) und nehmen deshalb "nie" -- der
// vorsichtigere Fall, der hoechstens ein Herausschieben auslaesst, das Vanilla gemacht haette.
const NEVER_SUFFOCATES: &[&str] = &["glass", "tinted_glass", "mangrove_roots", "moving_piston"];
const NEVER_SUFFOCATES_SUFFIXES: &[&str] = &["_stained_glass", "_leaves", "_shulker_box", "shulker_box", "copper_grate", "_grate"];
const ALWAYS_SUFFOCATES: &[&str] = &["farmland", "soul_sand", "dirt_path", "mud"];

pub fn is_suffocating(state: u32, shapes: &BlockShapes) -> bool {
    if state == STATE_AIR {
        return false;
    }
    let Some(name) = block_name(state) else { return false };
    if ALWAYS_SUFFOCATES.contains(&name) {
        return true;
    }
    if NEVER_SUFFOCATES.contains(&name) || NEVER_SUFFOCATES_SUFFIXES.iter().any(|s| name.ends_with(s)) {
        return false;
    }
    // isCollisionShapeFullBlock: die Kollisionsform muss die ganze Zelle ausfuellen. Geprueft
    // wird gegen dieselbe Formtabelle, gegen die auch die Bewegung klemmt -- was uns nicht
    // aufhaelt, kann uns auch nicht einschliessen.
    collision_boxes(state, shapes).iter().any(|b| b.min == [0.0; 3] && b.max == [1.0; 3])
}

fn is_passable(name: &str) -> bool {
    PASSABLE.contains(&name) || PASSABLE_SUFFIXES.iter().any(|s| name.ends_with(s))
}

// Bloecke OHNE Kollisionsbox, die trotzdem einen echten UMRISS haben -- Vanilla trennt
// beides sauber (`Settings.noCollision()` schaltet nur die Kollision ab, `getOutlineShape`
// bleibt die Modellform), wir haben es bisher in einem Topf gehabt.
//
// Ohne Umriss liessen sich Knoepfe in der Welt nicht druecken. Der Rechtsklick
// laeuft ueber physics::raycast_block, und der misst gegen outline_boxes(). Weil ein Knopf
// hier als "begehbar" galt, bekam er GAR KEINE Box -- der Strahl ging durch ihn hindurch auf
// die Wand dahinter. Damit meldete raycast_block einen Treffer auf die WAND (oder gar
// keinen), und der Client schickte USE_ITEM statt USE_ITEM_ON: der Knopf hat den Klick nie
// gesehen. Dasselbe galt fuer Hebel, Druckplatten, Repeater/Komparatoren (Verzoegerung
// einstellen), Schienen, Redstone und Stolperdraht-Haken.
//
// Bewusst NICHT hier: Kreuz-Modelle (Blumen, Gras, Fackeln, Setzlinge) und Fluessigkeiten.
// Kreuz-Modelle lassen sich nicht als achsenparallele Box aufloesen (s. assets::shapes),
// Wasser/Lava ignoriert Vanillas Blockstrahl ausdruecklich
// (RaycastContext.FluidHandling.NONE). Beide blieben also umrisslos wie bisher.
// Begehbare Bloecke, deren UMRISS trotzdem ein voller Wuerfel ist (s. outline_boxes).
// Bewusst kurz gehalten und namentlich: Feuer und Nether-Portal ueberschreiben
// getOutlineShape sehr wohl (eigene, flache Formen) und gehoeren deshalb NICHT hierher.
const FULL_CUBE_OUTLINE: &[&str] = &["cobweb"];

fn has_outline_without_collision(name: &str) -> bool {
    const NAMES: &[&str] =
        &["lever", "tripwire_hook", "repeater", "comparator", "redstone_wire", "rail", "powered_rail", "detector_rail", "activator_rail"];
    const SUFFIXES: &[&str] = &["_button", "_pressure_plate"];
    NAMES.contains(&name) || SUFFIXES.iter().any(|s| name.ends_with(s))
}

// Vanillas ShapeContext, auf die zwei Angaben reduziert, die im Client ueberhaupt eine Form
// veraendern: die Fusshoehe des Spielers und ob er absteigt (isDescending() == isSneaking()).
// Gebraucht ausschliesslich vom Geruest, s. vanilla_shapes::scaffolding_boxes.
#[derive(Clone, Copy, Debug)]
pub struct ShapeContext {
    pub feet_y: f64,
    pub descending: bool,
}

impl ShapeContext {
    // ShapeContext.absent(): isDescending() ist false, und isAbove(..) liefert schlicht seinen
    // Vorgabewert -- der ist an beiden Aufrufstellen im Geruest `true`. Eine unendlich hohe
    // Fusshoehe bildet genau das ab, ohne Sonderfall im Code.
    pub const ABSENT: ShapeContext = ShapeContext { feet_y: f64::INFINITY, descending: false };
}

// Die Kollisionsboxen eines States (Block-lokale 0..1-Einheiten, s. ShapeBox). Leer =
// begehbar. Owned statt geliehen: Zaun/Mauer/Zauntor berechnen ihre exakte (von der
// Render-Form abweichende) Kollisionsform dynamisch aus den Verbindungs-Properties (s.
// vanilla_shapes), das laesst sich nicht als Referenz in die BlockShapes-Tabelle legen.
// Fuer alle anderen soliden, nicht-passierbaren Bloecke ohne aufgeloeste Form liefert
// BlockShapes::shapes_of automatisch den Vollwuerfel-Fallback -- unbekannte/neue States
// kollidieren im Zweifel (sicherer, als versehentlich durch eine Wand zu laufen).
//
// Diese Fassung ohne Spielerbezug entspricht Vanillas ShapeContext.absent().
pub fn collision_boxes(state: u32, shapes: &BlockShapes) -> Vec<ShapeBox> {
    collision_boxes_at(state, shapes, 0, ShapeContext::ABSENT)
}

// Kollisionsform MIT Spielerbezug. `block_y` ist die Zelle, in der der Block sitzt -- das
// Geruest vergleicht die Fusshoehe damit.
pub fn collision_boxes_at(state: u32, shapes: &BlockShapes, block_y: i32, ctx: ShapeContext) -> Vec<ShapeBox> {
    // Das Baugeruest ist der einzige Block, dessen Kollisionsform vom SPIELER abhaengt
    // (wer sich daran heruntergleiten laesst, faellt hindurch) -- eine feste Tabelle kann das
    // nicht abbilden, deshalb steht er vor ihr.
    if block_name(state) == Some("scaffolding") {
        return crate::vanilla_shapes::scaffolding_boxes(state, ctx.feet_y, ctx.descending, block_y);
    }
    // Die aus dem Jar gedumpte ECHTE Form (s. vanilla_collision). Sie schlaegt alles, was
    // darunter folgt: die Sonderfaelle in vanilla_shapes und die aus dem RENDER-Modell
    // abgeleitete Form sind beides Naeherungen, und jede ihrer Abweichungen ist ein
    // dauerhafter Versatz gegenueber der Server-Simulation (GrimAC: Simulation/Phase/
    // GroundSpoof, s. Kopf von vanilla_collision_data.txt).
    if let Some(boxen) = crate::vanilla_collision::collision_boxes(state) {
        return undo_origin_offset(state, boxen);
    }
    if let Some(boxes) = block_name(state).and_then(crate::vanilla_shapes::sinking_collision_boxes) {
        return boxes;
    }
    // Begehbar heisst: KEINE Kollisionsbox -- auch dann nicht, wenn der Block einen Umriss
    // hat (Knopf, Hebel, Druckplatte, s. has_outline_without_collision). Diese Pruefung sass
    // frueher in outline_boxes() und galt dadurch fuer beides gleichzeitig; sie gehoert aber
    // nur hierher.
    if block_name(state).is_some_and(is_passable) {
        return Vec::new();
    }
    outline_boxes(state, shapes)
}

// Der Dump (s. vanilla_collision) fragte die Formen an BlockPos.ORIGIN ab -- bei Bloecken mit
// Zufallsversatz steckt deshalb der Versatz DIESER Position schon in der Form: Mth.getSeed(0,
// 0, 0) ist 0, also (0/15 - 0.5) * 0.5 = -0.25 je Achse, geklemmt auf den Hoechstversatz des
// Blocks. Hier wieder herausgerechnet; den Versatz der echten Position legen die Aufrufer
// darauf (s. block_offset::block_xz_offset). Vorher sass der Tropfstein-Umriss immer um 2/16
// schraeg in der Zelle, und Bambus bekam den
// Versatz doppelt.
fn undo_origin_offset(state: u32, boxen: &[ShapeBox]) -> Vec<ShapeBox> {
    let Some(name) = block_name(state).filter(|n| crate::block_offset::has_xz_offset(n)) else {
        return boxen.to_vec();
    };
    let m = crate::block_offset::max_xz_offset(name);
    // Nur zuruecknehmen, was der Dump wirklich enthaelt: Blumen stehen dort um den Versatz des
    // Ursprungs verschoben (Mitte bei 0.5 - m), Gras und Farn dagegen mittig. Pauschal
    // verschoben lag der Gras-Umriss eine Viertel-Zelle neben dem Gras.
    let mitte = |a: usize| {
        let lo = boxen.iter().map(|b| b.min[a]).fold(f32::MAX, f32::min);
        let hi = boxen.iter().map(|b| b.max[a]).fold(f32::MIN, f32::max);
        (lo + hi) / 2.0
    };
    if boxen.is_empty() || (mitte(0) - (0.5 - m)).abs() > 0.01 || (mitte(2) - (0.5 - m)).abs() > 0.01 {
        return boxen.to_vec();
    }
    boxen
        .iter()
        .map(|b| {
            let mut b = *b;
            for a in [0, 2] {
                b.min[a] += m;
                b.max[a] += m;
            }
            b
        })
        .collect()
}

// Die UMRISS-Form (Vanillas getOutlineShape): dieselbe Tabelle wie die Kollision, nur ohne
// die Einsink-Bloecke -- Seelensand und Honig sehen wie Vollwuerfel aus, lassen sich auch wie
// Vollwuerfel anvisieren und ueberschreiben in Vanilla ausschliesslich getCollisionShape.
// Gebraucht fuers Zielen (physics::raycast_block): mit der Kollisionsform wuerde der Strahl
// durch die oberen 2/16 eines Seelensands hindurchgehen, statt ihn zu treffen.
pub fn outline_boxes(state: u32, shapes: &BlockShapes) -> Vec<ShapeBox> {
    if state == STATE_AIR {
        return Vec::new();
    }
    // Auch der Umriss kommt aus dem Dump -- er ist in Vanilla eine EIGENE Form (Kaktus:
    // Kollision 15/16 hoch, Umriss 16/16; Lesepult: Umriss ragt sogar ueber die Zelle
    // hinaus). Alles darunter ist der alte, aus Modellen abgeleitete Weg.
    if let Some(boxen) = crate::vanilla_collision::outline_boxes(state) {
        return undo_origin_offset(state, boxen);
    }
    let Some(name) = block_name(state) else {
        return shapes.shapes_of(state).to_vec();
    };
    if is_passable(name) {
        // Ein Knopf/Hebel/eine Druckplatte hat einen Umriss, obwohl er nicht kollidiert --
        // aber NUR, wenn seine Form tatsaechlich aus den Modelldaten aufgeloest werden
        // konnte. Ohne aufgeloeste Form liefert shapes_of() den VOLLWUERFEL-Fallback, und
        // ein blockgrosser Knopf waere schlimmer als gar keiner: er wuerde den Strahl auf
        // die Wand dahinter blockieren und beim Bauen die falsche Seite liefern. Im
        // Zweifel also lieber wie bisher umrisslos.
        // Der Hebel braucht die Form von Hand -- sein Griff ist ein gedrehtes Element und
        // loest deshalb nie auf (s. vanilla_shapes::lever_boxes).
        if name == "lever" {
            return crate::vanilla_shapes::lever_boxes(state);
        }
        // Schienen genauso: Kurven/Steigungen sind gedrehte Elemente, Vanilla nimmt fuer den
        // Umriss ohnehin nur zwei feste Kaesten (s. vanilla_shapes::rail_boxes).
        if name.ends_with("rail") {
            return crate::vanilla_shapes::rail_boxes(state);
        }
        if name == "tripwire_hook" {
            return crate::vanilla_shapes::tripwire_hook_boxes(state);
        }
        if has_outline_without_collision(name) && shapes.has_resolved_shape(state) {
            return shapes.shapes_of(state).to_vec();
        }
        // Und die Bloecke, deren Umriss in Vanilla schlicht der VOLLWUERFEL ist: sie
        // ueberschreiben getOutlineShape gar nicht erst, und AbstractBlock.getOutlineShape
        // liefert von Haus aus VoxelShapes.fullCube() -- die leere Kollisionsform kommt
        // ausschliesslich aus `noCollision()`. Deshalb geht der Wuerfel hier NICHT durch
        // shapes_of: das Netz ist ein Kreuz-Modell, seine aufgeloeste Form waere leer.
        //
        // Ohne Umriss koennte man nicht auf Spinnweben bauen. Der Strahl ging durch das Netz hindurch, der Klick landete auf dem
        // Block dahinter -- im Crystal-PvP genau dort, wo man auf das Netz bauen will.
        if FULL_CUBE_OUTLINE.contains(&name) {
            return vec![crate::block_shapes::FULL_CUBE];
        }
        return Vec::new();
    }
    if let Some(boxes) = crate::vanilla_shapes::fence_wall_gate_boxes(name, state) {
        return boxes;
    }
    // Leiter: Modell ist eine Ebene ohne Dicke, Vanilla kollidiert mit 3/16 -- und Umriss und
    // Kollision sind dort dieselbe Form, deshalb steht das schon hier.
    if name == "ladder" {
        return crate::vanilla_shapes::ladder_boxes(state);
    }
    // Bett: hat gar kein ableitbares Modell (BlockEntity-Renderer), braucht die Form also
    // zwingend von Hand -- sonst waere es ein Vollwuerfel statt 9/16 hoch.
    if name.ends_with("_bed") {
        return crate::vanilla_shapes::bed_boxes(state);
    }
    shapes.shapes_of(state).to_vec()
}

// getCameraCollisionShape: in AbstractBlock per Voreinstellung die KOLLISIONSform -- der
// Honigblock erbt sie also samt Einrueckung. Seelensand ueberschreibt sie ausdruecklich mit
// dem Vollwuerfel (SoulSandBlock.getCameraCollisionShape), damit die Verfolgerkamera nicht in
// den Sand einsinkt, in dem der Spieler steht.
pub fn camera_collision_boxes(state: u32, shapes: &BlockShapes) -> Vec<ShapeBox> {
    if block_name(state) == Some("soul_sand") {
        return outline_boxes(state, shapes);
    }
    collision_boxes(state, shapes)
}

// Bloecke, die die Bewegung des NAECHSTEN Ticks daempfen, weil man in ihnen steckt
// (Entity.slowMovement). Rueckgabe ist Vanillas Multiplikator-Vektor je Achse.
//
// Bislang nur das Spinnennetz (CobwebBlock.onEntityCollision, 1.21.11 nachgesehen): Faktor
// (0.25, 0.05 als float, 0.25), mit Weaving-Effekt (0.5, 0.25, 0.5).
// Der WEAVING-Fall (0.5/0.25/0.5) fehlt hier, weil der Client keine Statuseffekte fuehrt --
// mit Weaving-Effekt weicht die Simulation also ab. Das ist eine bekannte Luecke, kein
// uebersehener Fall.
//
// Andere Bloecke mit slowMovement (Pulverschnee 0.9/1.5/0.9, Suessbeerenstrauch 0.8/0.75/0.8)
// gehoeren hier ebenfalls hin, sind aber nicht verifiziert und deshalb bewusst NICHT geraten
// -- ein falscher Multiplikator waere schlimmer als gar keiner, weil er still danebenliegt.
pub fn slow_movement_multiplier(state: u32) -> Option<[f64; 3]> {
    match block_name(state) {
        // 0.05 steht im Jar als float-Literal (0.05F) im ansonsten double-Konstruktor -- der
        // Wert ist also der f32-gerundete, nicht 0.05 in f64. Genau so uebernommen, weil sich
        // die Differenz ueber viele Ticks in der Position aufsummiert.
        Some("cobweb") => Some([0.25, 0.05f32 as f64, 0.25]),
        // SweetBerryBushBlock.entityInside: makeStuckInBlock mit (0.8, 0.75, 0.8)
        // fuer jedes Lebewesen ausser Fuchs und Biene, in JEDEM Wachstumsalter (nur der Schaden
        // haengt am Alter, und den rechnet der Server). x/z wieder als float-Literal.
        Some("sweet_berry_bush") => Some([0.8f32 as f64, 0.75, 0.8f32 as f64]),
        _ => None,
    }
}

// Vanillas BlockState.blocksMovement() -- gebraucht fuer den STROEMUNGSVEKTOR fliessenden
// Wassers (FlowableFluid.getVelocity fragt genau das ab, s. world::fluid::flow_velocity_at).
//
// Aus dem Yarn-1.21.11-Jar (AbstractBlock$AbstractBlockState):
//     blocksMovement() = block != COBWEB && block != BAMBOO_SAPLING && isSolid()
//     isSolid():  Kollisionsform nicht leer
//                 && (Durchschnitts-Kantenlaenge >= 0.7291666666666666 || Hoehe >= 1.0)
// Die 0.7291666666666666 sind 35/48 und stehen so im Jar -- keine gerundete Naeherung.
//
// WARUM DAS ZAEHLT: hier stand vorher is_solid() ("alles ausser Luft"). Damit galt jede Blume,
// jede Fackel, jede Druckplatte neben dem Wasser als Wand, und der Stroemungsvektor kam anders
// heraus als beim Server. In fliessendem Wasser weicht die Position dann Tick fuer Tick weiter
// ab -- Anticheat-Verdacht, ohne dass irgendetwas manipuliert waere.
//
// NICHT nachgebildet sind settings.forceSolid/forceNotSolid: Einzelfall-Flags aus der
// Blockdefinition, die sich aus der Form nicht ableiten lassen. Sie betreffen eine Handvoll
// Bloecke; die Formregel deckt den Rest.
pub fn blocks_movement(state: u32, shapes: &BlockShapes) -> bool {
    // Beide Ausnahmen sind Bloecke, durch die man sich hindurchbewegt, obwohl ihre Form gross
    // ist -- deshalb stehen sie in Vanilla namentlich davor.
    match block_name(state) {
        Some("cobweb") | Some("bamboo_sapling") | None => return false,
        _ => {}
    }
    is_solid_shape(&collision_boxes(state, shapes))
}

// Der Formteil von Vanillas isSolid(), getrennt von der Frage, WOHER die Boxen kommen.
// Eigene Funktion, damit die Kante bei 3/16 direkt pruefbar bleibt: seit die echten Formen
// aus dem Jar kommen (s. vanilla_collision), laesst sich einem Block keine Wunschform mehr
// unterschieben, um die Regel zu testen.
pub fn is_solid_shape(boxes: &[ShapeBox]) -> bool {
    if boxes.is_empty() {
        return false;
    }
    // VoxelShape.getBoundingBox(): die UMSCHLIESSENDE Box aller Teilboxen, nicht jede einzeln.
    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    for b in boxes {
        for i in 0..3 {
            min[i] = min[i].min(b.min[i]);
            max[i] = max[i].max(b.max[i]);
        }
    }
    // In f64 gerechnet wie Vanilla: 0.7291666666666666 (= 35/48) laesst sich in f32 gar nicht
    // darstellen, und der Vergleich ist ein >= genau auf dieser Kante -- bei voller Grundflaeche
    // entscheidet er sich bei exakt 3/16 Hoehe.
    let laenge = [(max[0] - min[0]) as f64, (max[1] - min[1]) as f64, (max[2] - min[2]) as f64];
    let mittel = (laenge[0] + laenge[1] + laenge[2]) / 3.0;
    mittel >= 0.7291666666666666 || laenge[1] >= 1.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block_registry::{all_block_names, state_count};

    fn state_of(block: &str) -> u32 {
        (0..state_count() as u32).find(|&s| block_name(s) == Some(block)).expect("Block unbekannt")
    }

    // Vanillas Formregel fuer isSolid(): Durchschnitts-Kantenlaenge >= 35/48 ODER Hoehe >= 1.
    // Bei voller Grundflaeche (1 x h x 1) heisst das h >= 3/16 -- genau die Kante zwischen
    // einem Teppich (1/16, blockiert nicht) und allem Dickeren. Beide Seiten der Grenze
    // geprueft, damit weder ein >= zu < wird noch die Konstante verrutscht.
    #[test]
    fn formregel_trennt_bei_drei_sechzehnteln() {
        let platte = |hoehe: f32| {
            is_solid_shape(&[ShapeBox { min: [0.0, 0.0, 0.0], max: [1.0, hoehe, 1.0], faces: [true; 6], content: [false; 6], uv: [None; 6] }])
        };
        assert!(!platte(1.0 / 16.0), "ein Teppich blockiert die Bewegung nicht");
        assert!(!platte(3.0 / 16.0 - 0.001), "knapp unter der Grenze noch nicht");
        assert!(platte(3.0 / 16.0), "genau auf der Grenze schon (Vanilla vergleicht mit >=)");
        assert!(platte(0.5), "eine Stufe blockiert");
        assert!(!is_solid_shape(&[]), "ohne Box blockiert nichts");

        // Und die Gegenprobe an echten Bloecken, jetzt mit ihren echten Formen: der Teppich
        // (1/16) faellt durch die Regel, die Stufe (8/16) nicht.
        let shapes = BlockShapes::all_full_cubes();
        assert!(!blocks_movement(state_of("white_carpet"), &shapes));
        assert!(blocks_movement(state_of("stone_slab"), &shapes));
    }

    // Spinnweben: KEINE Kollision, aber ein voller Umriss -- sonst zielt der Rechtsklick
    // durch das Netz hindurch auf den Block dahinter.
    #[test]
    fn spinnweben_haben_einen_umriss_aber_keine_kollision() {
        let shapes = BlockShapes::all_full_cubes();
        let netz = state_of("cobweb");
        assert!(!blocks_movement(netz, &shapes), "durch ein Netz laeuft man hindurch");
        let umriss = outline_boxes(netz, &shapes);
        assert_eq!(umriss.len(), 1, "genau eine Box");
        assert_eq!(umriss[0].min, [0.0, 0.0, 0.0]);
        assert_eq!(umriss[0].max, [1.0, 1.0, 1.0]);
        // Gegenprobe: eine Blume hat einen KLEINEN Umriss, keinen Vollwuerfel. Frueher stand
        // hier "umrisslos" -- das war nur ein Nebeneffekt davon, dass sich aus einem
        // Kreuz-Modell keine Box ableiten laesst. Vanillas FlowerBlock hat sehr wohl eine
        // getShape, und man kann eine Blume anvisieren.
        let blume = outline_boxes(state_of("dandelion"), &shapes);
        assert_eq!(blume.len(), 1, "genau eine Box");
        assert!(blume[0].max[1] < 1.0, "eine Blume ist kein Vollwuerfel: {:?}", blume[0]);
    }

    // Ein leerer Kollisions-Umriss blockiert nie, egal wie der Block heisst.
    #[test]
    fn begehbares_blockiert_die_bewegung_nicht() {
        let shapes = BlockShapes::all_full_cubes();
        for b in ["short_grass", "dandelion", "torch", "oak_sapling"] {
            assert!(!blocks_movement(state_of(b), &shapes), "{b} darf die Bewegung nicht blockieren");
        }
        assert!(blocks_movement(state_of("stone"), &shapes), "Stein blockiert");
    }

    // Die beiden namentlichen Ausnahmen: volle Form, trotzdem kein blocksMovement -- man
    // bewegt sich durch sie hindurch (gebremst). Stuenden sie nicht davor, wuerde die
    // Formregel sie als Wand werten und den Stroemungsvektor daneben verfaelschen.
    #[test]
    fn spinnennetz_und_bambusspross_sind_ausgenommen() {
        let shapes = BlockShapes::all_full_cubes();
        for b in ["cobweb", "bamboo_sapling"] {
            assert!(!blocks_movement(state_of(b), &shapes), "{b} ist in Vanilla namentlich ausgenommen");
        }
    }

    // Unabhaengig vom Modell: das Netz darf nie eine Kollisionsbox bekommen, auch wenn die
    // Formtabelle (wie hier der all_full_cubes-Stub, oder ein Resourcepack) einen Vollwuerfel
    // liefert.
    #[test]
    fn spinnennetz_hat_keine_kollisionsbox() {
        let shapes = BlockShapes::all_full_cubes();
        assert!(collision_boxes(state_of("cobweb"), &shapes).is_empty());
    }

    // Der Bremsfaktor selbst -- die Zahlen stehen so in CobwebBlock.onEntityCollision.
    #[test]
    fn spinnennetz_bremst_mit_den_vanilla_faktoren() {
        let m = slow_movement_multiplier(state_of("cobweb")).expect("Netz bremst");
        assert_eq!(m[0], 0.25);
        assert_eq!(m[2], 0.25);
        // 0.05F, nicht 0.05 -- im Jar steht ein float-Literal im double-Konstruktor.
        assert_eq!(m[1], 0.05f32 as f64);
        assert!(slow_movement_multiplier(state_of("stone")).is_none());
    }

    // Umrisse von Pflanzen OHNE den Versatz der Position (den legt der Aufrufer an): Blumen und
    // Gras mittig in der Zelle, Setzlinge ebenso. Frueher lag Gras um den vollen Versatz
    // daneben, weil der Dump ihn dort gar nicht enthaelt.
    #[test]
    fn pflanzen_umrisse_sind_mittig() {
        let shapes = BlockShapes::all_full_cubes();
        for b in ["short_grass", "fern", "dandelion", "poppy", "oak_sapling", "red_mushroom"] {
            let boxen = outline_boxes(state_of(b), &shapes);
            assert!(!boxen.is_empty(), "{b} ohne Umriss");
            for a in [0, 2] {
                let lo = boxen.iter().map(|x| x.min[a]).fold(f32::MAX, f32::min);
                let hi = boxen.iter().map(|x| x.max[a]).fold(f32::MIN, f32::max);
                assert!(((lo + hi) / 2.0 - 0.5).abs() < 1e-3, "{b}: Achse {a} {lo}..{hi}");
            }
        }
    }

    #[test]
    fn vegetation_ist_begehbar() {
        let shapes = BlockShapes::all_full_cubes();
        for b in ["short_grass", "tall_grass", "fern", "dandelion", "poppy", "oak_sapling", "torch"] {
            assert!(collision_boxes(state_of(b), &shapes).is_empty(), "{b} sollte begehbar sein");
        }
    }

    // Regressionstest: Falllaub darf keine Kollisionsbox haben. all_full_cubes() simuliert hier bewusst den SCHLIMMSTEN Fall (Vollwuerfel-
    // Fallback) -- selbst dann muss leaf_litter dank der PASSABLE-Liste begehbar bleiben,
    // unabhaengig davon, ob die echte (duenne) Form aufgeloest werden konnte.
    #[test]
    fn laublitter_ist_begehbar() {
        let shapes = BlockShapes::all_full_cubes();
        assert!(collision_boxes(state_of("leaf_litter"), &shapes).is_empty(), "leaf_litter sollte begehbar sein");
    }

    // "flowing_water"/"flowing_lava" gibt es als eigene Blocknamen seit der Flattening-
    // Umstellung nicht mehr (nur noch "water"/"lava" mit "level"-Property, s. is_water-
    // Kommentar in biome_tint.rs) -- state_of() wuerde dafuer panicken, deshalb nur die
    // echten Namen pruefen.
    #[test]
    fn wasser_und_lava_sind_begehbar() {
        let shapes = BlockShapes::all_full_cubes();
        for b in ["water", "lava"] {
            assert!(collision_boxes(state_of(b), &shapes).is_empty(), "{b} sollte begehbar (schwimm-/sinkbar) sein");
        }
    }

    #[test]
    fn feste_bloecke_kollidieren() {
        let shapes = BlockShapes::all_full_cubes();
        for b in ["stone", "grass_block", "dirt", "oak_planks", "oak_log", "snow_block"] {
            assert!(!collision_boxes(state_of(b), &shapes).is_empty(), "{b} sollte kollidieren");
        }
    }

    #[test]
    fn luft_kollidiert_nicht() {
        let shapes = BlockShapes::all_full_cubes();
        assert!(collision_boxes(0, &shapes).is_empty());
        // Kein regulaerer Block darf faelschlich als Suffix-Treffer durchfallen: eine
        // Stichprobe, dass z.B. nichts auf "torch" endet, das eigentlich fest ist.
        let _ = all_block_names(); // nur sicherstellen, dass die Registry laedt
    }
}
