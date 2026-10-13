// Exakte Kollisionsformen fuer Zaun/Mauer/Zauntor -- per Yarn-Mappings aus dem echten
// 1.21.11-Client nachgesehen (FenceBlock/HorizontalConnectingBlock, WallBlock,
// FenceGateBlock -- javap+Vineflower gegen den Fabric-Loom-Cache).
//
// Diese drei Blockfamilien sind ein Sonderfall: ihre COLLISION-Shape ist in Vanilla bewusst
// GROESSER als ihr Render-Modell (damit man nicht ueber einen Zaun/eine Mauer springen kann,
// obwohl der Pfosten optisch nur 1 Block hoch wirkt) -- die generische, aus den echten
// Modell-Elementen abgeleitete Form (s. assets::shapes) trifft hier NICHT zu, Rendering und
// Kollision sind in Vanilla fuer diese Bloecke absichtlich zwei verschiedene Shapes. Fuer
// alle anderen Blockfamilien (Stufen, Treppen, Teppiche, ...) stimmen beide ueberein, dort
// bleibt der generische Weg richtig.
//
// Herkunft der Zahlen (Block.createColumnShape/createCuboidZShape, s. Block.java):
//   createColumnShape(sizeXz, minY, maxY): Mittelpunkt 8,8 minus/plus sizeXz/2 in X/Z.
//   createCuboidZShape(sizeX, minY, maxY, minZ, maxZ): Mittelpunkt 8 minus/plus sizeX/2 in X,
//     Z direkt minZ..maxZ (vor der Rotation auf die jeweilige Himmelsrichtung).
// Rotation um 90 Grad je Schritt: (x,z) -> (16-z, x) -- dieselbe Formel wie beim
// Elemente-Rotieren in assets::shapes, hier von Hand fuer die vier Richtungen ausgewertet
// und als Konstanten hinterlegt (spart die Rotation zur Laufzeit).

use crate::block_shapes::ShapeBox;
use crate::state_props::property;

fn box16(x0: f32, y0: f32, z0: f32, x1: f32, y1: f32, z1: f32) -> ShapeBox {
    ShapeBox {
        min: [x0 / 16.0, y0 / 16.0, z0 / 16.0],
        max: [x1 / 16.0, y1 / 16.0, z1 / 16.0],
        faces: [true; 6],
        content: [false; 6],
        uv: [None; 6],
    }
}

fn is_true(state: u32, key: &str) -> bool {
    property(state, key) == Some("true")
}

// None = keine der drei Sonderfamilien -- Aufrufer nimmt den generischen Weg.
pub fn fence_wall_gate_boxes(name: &str, state: u32) -> Option<Vec<ShapeBox>> {
    if name.ends_with("_fence_gate") {
        return Some(fence_gate_boxes(state));
    }
    if name.ends_with("_fence") || name == "fence" {
        return Some(fence_boxes(state));
    }
    if name.ends_with("_wall") {
        return Some(wall_boxes(state));
    }
    None
}

// Leiter: LadderBlock.getOutlineShape ist Block.createCuboidZShape(16, 13, 16) -- eine 3/16
// tiefe Platte an der Wand, fuer NORTH bei z 13..16, die uebrigen Richtungen per
// VoxelShapes.createHorizontalFacingShapeMap gedreht ((x,z) -> (16-z, x), wie oben). Eine
// eigene getCollisionShape hat der Block NICHT, Umriss und Kollision sind also dasselbe.
//
// Warum das hier stehen muss: das Leiter-MODELL ist eine Ebene ohne Dicke (from/to identisch
// auf einer Achse). Aus dem Modell abgeleitet bekaeme die Leiter eine entartete Kollisionsbox,
// und der Spieler stuende bis zu 3/16 anders als beim Server -- genau die Art Abweichung, die
// beim Klettern jeden Tick in die Vorhersage eingeht.
//
// `facing` ist die Richtung, in die die Leiter ZEIGT; die Platte liegt auf der
// gegenueberliegenden Seite, also an der trage nden Wand.
pub fn ladder_boxes(state: u32) -> Vec<ShapeBox> {
    match property(state, "facing") {
        Some("south") => vec![box16(0.0, 0.0, 0.0, 16.0, 16.0, 3.0)],
        Some("west") => vec![box16(13.0, 0.0, 0.0, 16.0, 16.0, 16.0)],
        Some("east") => vec![box16(0.0, 0.0, 0.0, 3.0, 16.0, 16.0)],
        // north und alles Unbekannte: die Grundform aus dem Jar.
        _ => vec![box16(0.0, 0.0, 13.0, 16.0, 16.0, 16.0)],
    }
}

// Hebel (LeverBlock.getOutlineShape). Der Hebel hat KEINE Kollisionsbox, aber einen Umriss --
// und nur gegen den Umriss zielt der Rechtsklick. Aus dem MODELL laesst er sich nicht
// ableiten: der Schaltgriff ist ein gedrehtes Element (lever_on/lever_off), und gedrehte
// Elemente sind keine achsenparallelen Boxen (s. assets::shapes-Modulkommentar) -- der State
// bekaeme gar keine Form und faellt damit auf "umrisslos" zurueck. Deshalb wie Leiter und
// Bett von Hand.
//
// Aus dem 1.21.11-Jar:
//   VoxelShapes.createBlockFaceHorizontalFacingShapeMap(Block.createCuboidZShape(6, 8, 10, 16))
//   -> createCuboidZShape(sizeX=6, sizeY=8, minZ=10, maxZ=16)
//      = createCuboidShape(8-3, 8-4, 10, 8+3, 8+4, 16) = (5, 4, 10) .. (11, 12, 16)
// Das ist die WAND-Form fuer facing=north (die Platte klebt an z=16, also an der Wand im
// Sueden -- `facing` ist die Richtung, in die der Hebel ZEIGT, wie bei der Leiter).
// Die uebrigen Waende sind die Y-Drehungen davon; FLOOR/CEILING entstehen im Jar aus
// derselben Grundform per DirectionTransformation (Kippen um X bzw. zusaetzliche Spiegelung).
// Gegenprobe: die so berechneten Boxen sind exakt die frueher in Vanilla ausgeschriebenen
// Konstanten (FLOOR_Z_AXIS_SHAPE (5,0,4)-(11,6,12), CEILING_Z_AXIS_SHAPE (5,10,4)-(11,16,12),
// X-Achse jeweils mit vertauschtem x/z) -- bis auf die Wandhoehe, die von 3..13 auf 4..12
// geschrumpft ist.
pub fn lever_boxes(state: u32) -> Vec<ShapeBox> {
    let facing = property(state, "facing").unwrap_or("north");
    // Bei Boden/Decke zaehlt nur die ACHSE, nicht die Richtung -- der Hebel liegt laengs.
    let z_achse = matches!(facing, "north" | "south");
    match property(state, "face") {
        Some("floor") if z_achse => vec![box16(5.0, 0.0, 4.0, 11.0, 6.0, 12.0)],
        Some("floor") => vec![box16(4.0, 0.0, 5.0, 12.0, 6.0, 11.0)],
        Some("ceiling") if z_achse => vec![box16(5.0, 10.0, 4.0, 11.0, 16.0, 12.0)],
        Some("ceiling") => vec![box16(4.0, 10.0, 5.0, 12.0, 16.0, 11.0)],
        // "wall" und alles Unbekannte: die Grundform aus dem Jar, um Y gedreht.
        _ => match facing {
            "south" => vec![box16(5.0, 4.0, 0.0, 11.0, 12.0, 6.0)],
            "west" => vec![box16(10.0, 4.0, 5.0, 16.0, 12.0, 11.0)],
            "east" => vec![box16(0.0, 4.0, 5.0, 6.0, 12.0, 11.0)],
            _ => vec![box16(5.0, 4.0, 10.0, 11.0, 12.0, 16.0)],
        },
    }
}

// Schienen (AbstractRailBlock.getOutlineShape). Ebenfalls ohne Kollision, aber mit Umriss.
// Aus dem 1.21.11-Jar, und erfreulich kurz:
//   STRAIGHT_SHAPE  = Block.createColumnShape(16, 0, 2)  -> (0,0,0)..(16,2,16)
//   ASCENDING_SHAPE = Block.createColumnShape(16, 0, 8)  -> (0,0,0)..(16,8,16)
//   getOutlineShape = shape.isAscending() ? ASCENDING : STRAIGHT
// Die Kurven-/Steigungsformen des MODELLS sind gedrehte Elemente und loesen deshalb nicht auf
// (s. lever_boxes) -- Vanilla benutzt fuer den Umriss ohnehin nur diese beiden Kaesten, die
// Handfassung ist hier also nicht nur ein Ersatz, sondern das Original.
pub fn rail_boxes(state: u32) -> Vec<ShapeBox> {
    let steigend = property(state, "shape").is_some_and(|s| s.starts_with("ascending_"));
    let hoehe = if steigend { 8.0 } else { 2.0 };
    vec![box16(0.0, 0.0, 0.0, 16.0, hoehe, 16.0)]
}

// Stolperdraht-Haken (TripwireHookBlock.getOutlineShape). Auch hier ist der Haken selbst ein
// gedrehtes Element, das Modell loest also nicht auf. Aus dem Jar:
//   VoxelShapes.createHorizontalFacingShapeMap(Block.createCuboidZShape(6, 0, 10, 10, 16))
//   -> createCuboidZShape(sizeX=6, minY=0, maxY=10, minZ=10, maxZ=16)
//      = createCuboidShape(5, 0, 10, 11, 10, 16)
// Grundform ist wie beim Hebel facing=north (Platte an z=16), Rest sind die Y-Drehungen.
pub fn tripwire_hook_boxes(state: u32) -> Vec<ShapeBox> {
    match property(state, "facing") {
        Some("south") => vec![box16(5.0, 0.0, 0.0, 11.0, 10.0, 6.0)],
        Some("west") => vec![box16(10.0, 0.0, 5.0, 16.0, 10.0, 11.0)],
        Some("east") => vec![box16(0.0, 0.0, 5.0, 6.0, 10.0, 11.0)],
        _ => vec![box16(5.0, 0.0, 10.0, 11.0, 10.0, 16.0)],
    }
}

// Geruest (ScaffoldingBlock.getCollisionShape) -- die einzige Form im Spiel, die vom ZUSTAND
// DES SPIELERS abhaengt, nicht nur vom Block:
//
//   if (context.isAbove(fullCube, pos, true) && !context.isDescending()) -> Deckplatte + Pfosten
//   else if (distance != 0 && bottom && context.isAbove(OUTLINE_SHAPE, pos, true)) -> Bodenplatte
//   else -> gar keine Kollision
//
// OUTLINE_SHAPE ist der um -1 verschobene Vollwuerfel, seine Oberkante liegt also bei y-1+1 = y;
// `isAbove(shape, pos, _)` ist `feet_y > pos.y + shape.getMax(Y) - 1.0E-5`. Daraus werden die
// beiden Schwellen `feet_y > y + 1 - 1e-5` und `feet_y > y - 1e-5`. GrimAC rechnet exakt
// dieselben zwei Zeilen (CollisionData.SCAFFOLDING, mit player.lastY).
//
// Genau daran haengt das Heruntersteigen: schleichend (isDescending) faellt die Deckplatte weg,
// und man sinkt durchs Geruest, statt darauf zu stehen.
pub fn scaffolding_boxes(state: u32, feet_y: f64, descending: bool, block_y: i32) -> Vec<ShapeBox> {
    if feet_y > block_y as f64 + 1.0 - 1.0e-5 && !descending {
        return vec![
            box16(0.0, 14.0, 0.0, 16.0, 16.0, 16.0),
            box16(0.0, 0.0, 0.0, 2.0, 16.0, 2.0),
            box16(14.0, 0.0, 0.0, 16.0, 16.0, 2.0),
            box16(0.0, 0.0, 14.0, 2.0, 16.0, 16.0),
            box16(14.0, 0.0, 14.0, 16.0, 16.0, 16.0),
        ];
    }
    let distanz_ungleich_null = property(state, "distance").is_none_or(|d| d != "0");
    if distanz_ungleich_null && is_true(state, "bottom") && feet_y > block_y as f64 - 1.0e-5 {
        return vec![box16(0.0, 0.0, 0.0, 16.0, 2.0, 16.0)];
    }
    Vec::new()
}

// Bett: BedBlock.SHAPES_BY_DIRECTION ist die Vereinigung aus der Matratze
// Block.createColumnShape(16, 3, 9) -- also X/Z voll, Y 3..9 -- und ZWEI Pfosten
// Block.createCuboidShape(0,0,0, 3,3,3), der zweite davon in X gespiegelt. Die Karte wird per
// createHorizontalFacingShapeMap gedreht; der Schluessel ist
// `getOppositePartDirection(state).getOpposite()`, ausgerechnet also
// `part == HEAD ? facing : facing.getOpposite()` -- die Pfosten stehen am AEUSSEREN Ende jeder
// Betthaelfte. GrimAC hat exakt dieselben vier Faelle (CollisionData.BED).
//
// Ohne das hier bekaeme das Bett den Vollwuerfel-Fallback: das Bettmodell wird in Vanilla von
// einem BlockEntity-Renderer gezeichnet, aus den Modelldateien laesst sich also gar keine Form
// ableiten. Der Spieler stuende auf 1.0 statt auf 9/16 -- 7/16 Unterschied zum Server bei jedem
// Schritt auf ein Bett.
pub fn bed_boxes(state: u32) -> Vec<ShapeBox> {
    let matratze = box16(0.0, 3.0, 0.0, 16.0, 9.0, 16.0);
    let kopfteil = property(state, "part") == Some("head");
    let facing = property(state, "facing").unwrap_or("north");
    // part == HEAD ? facing : facing.getOpposite()
    let seite = match (facing, kopfteil) {
        ("south", true) | ("north", false) => "south",
        ("west", true) | ("east", false) => "west",
        ("east", true) | ("west", false) => "east",
        _ => "north",
    };
    let pfosten = match seite {
        "south" => [box16(0.0, 0.0, 13.0, 3.0, 3.0, 16.0), box16(13.0, 0.0, 13.0, 16.0, 3.0, 16.0)],
        "west" => [box16(13.0, 0.0, 0.0, 16.0, 3.0, 3.0), box16(13.0, 0.0, 13.0, 16.0, 3.0, 16.0)],
        "east" => [box16(0.0, 0.0, 0.0, 3.0, 3.0, 3.0), box16(0.0, 0.0, 13.0, 3.0, 3.0, 16.0)],
        _ => [box16(0.0, 0.0, 0.0, 3.0, 3.0, 3.0), box16(13.0, 0.0, 0.0, 16.0, 3.0, 3.0)],
    };
    vec![matratze, pfosten[0], pfosten[1]]
}

// Zweite Familie mit abweichender Kollisionsform, diesmal KLEINER als das Render-Modell:
// Bloecke, in die man ein Stueck einsinkt. Beide sehen aus wie ein Vollwuerfel, haben aber
// eine eigene getCollisionShape (aus dem 1.21.11-Jar nachgesehen):
//   SoulSandBlock: Block.createColumnShape(16.0, 0.0, 14.0)  -> X/Z voll, Y 0..14
//   HoneyBlock:    Block.createColumnShape(14.0, 0.0, 15.0)  -> X/Z 1..15, Y 0..15
// createColumnShape(sizeXz, minY, maxY) zentriert die Breite um 8, s. Block.java.
//
// Die 1/16 seitliche Einrueckung des Honigblocks ist KEIN Schoenheitsfehler, sondern die
// Voraussetzung fuer das Herunterrutschen: wer sich seitlich an einen Honigblock drueckt,
// steht dadurch 1/16 IN dessen Zelle -- nur so erreicht ihn ueberhaupt die Blockkollisions-
// Schleife (die die Spielerbox um 1e-5 schrumpft), und nur so ist der Mittelpunktsabstand in
// HoneyBlock.isSliding() gross genug (0.7375 gegen die Schwelle 0.4375 + Breite/2).
//
// NUR die Kollision ist kleiner. Der Umriss (getOutlineShape, also das Zielkreuz beim Bauen)
// bleibt bei beiden der Vollwuerfel, und Seelensand setzt zusaetzlich getCameraCollisionShape
// auf den Vollwuerfel -- s. collision::outline_boxes/camera_collision_boxes.
pub fn sinking_collision_boxes(name: &str) -> Option<Vec<ShapeBox>> {
    match name {
        "soul_sand" => Some(vec![box16(0.0, 0.0, 0.0, 16.0, 14.0, 16.0)]),
        "honey_block" => Some(vec![box16(1.0, 0.0, 1.0, 15.0, 15.0, 15.0)]),
        _ => None,
    }
}

// FenceBlock: Pfosten X/Z 6..10 (wie das Render-Modell), aber Y 0..24 (1.5 Bloecke) statt
// 0..16 -- der Pfosten ist IMMER da, unabhaengig von Verbindungen. Je verbundener Richtung
// kommt ein Riegel-Ersatzblock dazu, der bis zur Zellengrenze reicht.
fn fence_boxes(state: u32) -> Vec<ShapeBox> {
    let mut out = vec![box16(6.0, 0.0, 6.0, 10.0, 24.0, 10.0)];
    if is_true(state, "north") {
        out.push(box16(6.0, 0.0, 0.0, 10.0, 24.0, 8.0));
    }
    if is_true(state, "south") {
        out.push(box16(6.0, 0.0, 8.0, 10.0, 24.0, 16.0));
    }
    if is_true(state, "east") {
        out.push(box16(8.0, 0.0, 6.0, 16.0, 24.0, 10.0));
    }
    if is_true(state, "west") {
        out.push(box16(0.0, 0.0, 6.0, 8.0, 24.0, 10.0));
    }
    out
}

// WallBlock: deutlich breiterer Pfosten (X/Z 4..12) als das duenne Render-Modell, nur bei
// up=true vorhanden. Riegel sind ebenfalls breiter (5..11) und reichen NICHT bis zur
// Zellengrenze (nur bis 11 von 16) -- kein Fehler: der Nachbarblock steuert seinen eigenen
// Riegel von der anderen Seite bei, beide zusammen schliessen die Luecke exakt (nachgerechnet). "low"/"tall" (WallShape) unterscheiden sich nur optisch, fuer die
// Kollision zaehlt jede Verbindung ungeachtet der Hoehe voll (immer bis Y=24).
fn wall_boxes(state: u32) -> Vec<ShapeBox> {
    let mut out = Vec::new();
    if property(state, "up") != Some("false") {
        out.push(box16(4.0, 0.0, 4.0, 12.0, 24.0, 12.0));
    }
    let connected = |dir: &str| property(state, dir).map(|v| v != "none").unwrap_or(false);
    if connected("north") {
        out.push(box16(5.0, 0.0, 0.0, 11.0, 24.0, 11.0));
    }
    if connected("south") {
        out.push(box16(5.0, 0.0, 5.0, 11.0, 24.0, 16.0));
    }
    if connected("east") {
        out.push(box16(5.0, 0.0, 5.0, 16.0, 24.0, 11.0));
    }
    if connected("west") {
        out.push(box16(0.0, 0.0, 5.0, 11.0, 24.0, 11.0));
    }
    out
}

// FenceGateBlock: offen = keine Kollision (man kann durchlaufen); geschlossen = eine duenne
// Wand quer zur Facing-Achse, volle Breite (0..16) auf der offenen Achse, 6..10 auf der
// Facing-Achse, Y 0..24.
fn fence_gate_boxes(state: u32) -> Vec<ShapeBox> {
    if property(state, "open") == Some("true") {
        return Vec::new();
    }
    match property(state, "facing") {
        Some("north") | Some("south") => vec![box16(0.0, 0.0, 6.0, 16.0, 24.0, 10.0)],
        _ => vec![box16(6.0, 0.0, 0.0, 10.0, 24.0, 16.0)],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block_registry::{block_name, state_count};

    fn state_with(block: &str, props: &[(&str, &str)]) -> u32 {
        (0..state_count() as u32)
            .find(|&s| block_name(s) == Some(block) && props.iter().all(|&(k, v)| property(s, k) == Some(v)))
            .unwrap_or_else(|| panic!("kein State fuer {block} mit {props:?} gefunden"))
    }

    // Die Formen von Hebel/Schiene/Stolperdraht-Haken stammen aus dem 
    // 1.21.11-Jar (s. Kommentare an den Funktionen). Die Zahlen hier sind die Gegenprobe:
    // wer sie aendert, muss vorher wieder ins Jar geschaut haben.
    #[test]
    fn hebel_wand_grundform_stimmt_mit_dem_jar_ueberein() {
        // createCuboidZShape(6, 8, 10, 16) = (5,4,10)..(11,12,16), Grundform ist facing=north.
        let north = state_with("lever", &[("face", "wall"), ("facing", "north")]);
        assert_eq!(lever_boxes(north), vec![box16(5.0, 4.0, 10.0, 11.0, 12.0, 16.0)]);
        // Y-Drehung: die Platte wandert auf die gegenueberliegende Wand.
        let south = state_with("lever", &[("face", "wall"), ("facing", "south")]);
        assert_eq!(lever_boxes(south), vec![box16(5.0, 4.0, 0.0, 11.0, 12.0, 6.0)]);
    }

    // Ein Hebel am Boden darf nicht die Wandform behalten -- sonst schwebt sein Klickziel
    // mitten in der Luft an der Wand, statt am Boden zu liegen.
    #[test]
    fn hebel_am_boden_liegt_am_boden_und_laengs_seiner_achse() {
        let nord = state_with("lever", &[("face", "floor"), ("facing", "north")]);
        assert_eq!(lever_boxes(nord), vec![box16(5.0, 0.0, 4.0, 11.0, 6.0, 12.0)]);
        // Ost/West ist dieselbe Form mit vertauschten x/z.
        let ost = state_with("lever", &[("face", "floor"), ("facing", "east")]);
        assert_eq!(lever_boxes(ost), vec![box16(4.0, 0.0, 5.0, 12.0, 6.0, 11.0)]);
        let decke = state_with("lever", &[("face", "ceiling"), ("facing", "north")]);
        assert_eq!(lever_boxes(decke), vec![box16(5.0, 10.0, 4.0, 11.0, 16.0, 12.0)]);
    }

    // AbstractRailBlock: STRAIGHT_SHAPE = createColumnShape(16, 0, 2),
    // ASCENDING_SHAPE = createColumnShape(16, 0, 8) -- mehr Faelle kennt Vanilla nicht.
    #[test]
    fn schiene_ist_flach_und_nur_beim_steigen_hoch() {
        let flach = state_with("rail", &[("shape", "north_south")]);
        assert_eq!(rail_boxes(flach), vec![box16(0.0, 0.0, 0.0, 16.0, 2.0, 16.0)]);
        let steigend = state_with("rail", &[("shape", "ascending_east")]);
        assert_eq!(rail_boxes(steigend), vec![box16(0.0, 0.0, 0.0, 16.0, 8.0, 16.0)]);
        // Auch die Kurve bleibt flach -- Vanilla hat dafuer keine eigene Umrissform.
        let kurve = state_with("rail", &[("shape", "south_east")]);
        assert_eq!(rail_boxes(kurve), vec![box16(0.0, 0.0, 0.0, 16.0, 2.0, 16.0)]);
    }

    // createCuboidZShape(6, 0, 10, 10, 16) = (5,0,10)..(11,10,16), Grundform facing=north --
    // dieselbe Drehkonvention wie beim Hebel.
    #[test]
    fn stolperdraht_haken_haengt_an_der_wand_hinter_sich() {
        let north = state_with("tripwire_hook", &[("facing", "north")]);
        assert_eq!(tripwire_hook_boxes(north), vec![box16(5.0, 0.0, 10.0, 11.0, 10.0, 16.0)]);
        let west = state_with("tripwire_hook", &[("facing", "west")]);
        assert_eq!(tripwire_hook_boxes(west), vec![box16(10.0, 0.0, 5.0, 16.0, 10.0, 11.0)]);
    }

    #[test]
    fn zaun_pfosten_ist_anderthalb_bloecke_hoch() {
        let s = state_with("oak_fence", &[("north", "false"), ("east", "false"), ("south", "false"), ("west", "false")]);
        let boxes = fence_boxes(s);
        assert_eq!(boxes.len(), 1, "ohne Verbindung nur der Pfosten");
        assert!((boxes[0].max[1] - 1.5).abs() < 1e-6, "Pfosten muss bis y=1.5 reichen, war {}", boxes[0].max[1]);
    }

    #[test]
    fn zaun_mit_allen_vier_verbindungen_hat_fuenf_boxen() {
        let s = state_with("oak_fence", &[("north", "true"), ("east", "true"), ("south", "true"), ("west", "true")]);
        assert_eq!(fence_boxes(s).len(), 5);
    }

    #[test]
    fn offenes_zauntor_kollidiert_nicht() {
        let s = state_with("oak_fence_gate", &[("open", "true")]);
        assert!(fence_gate_boxes(s).is_empty());
    }

    #[test]
    fn geschlossenes_zauntor_kollidiert() {
        let s = state_with("oak_fence_gate", &[("open", "false"), ("facing", "north")]);
        let boxes = fence_gate_boxes(s);
        assert_eq!(boxes.len(), 1);
        assert!((boxes[0].max[1] - 1.5).abs() < 1e-6);
    }

    #[test]
    fn mauer_pfosten_ist_breiter_als_zaunpfosten() {
        let s = state_with("cobblestone_wall", &[("up", "true"), ("north", "none"), ("east", "none"), ("south", "none"), ("west", "none")]);
        let boxes = wall_boxes(s);
        assert_eq!(boxes.len(), 1);
        assert!((boxes[0].min[0] - 0.25).abs() < 1e-6, "Mauerpfosten min.x sollte 4/16=0.25 sein, war {}", boxes[0].min[0]);
        assert!((boxes[0].max[0] - 0.75).abs() < 1e-6);
    }
}
