// Vanillas "Weiche Beleuchtung" -- genauer: ihr Ambient-Occlusion-Anteil.
//
// Smooth Lighting ist in Vanilla ZWEI Dinge in einer Rechnung (BlockModelRenderer.
// AmbientOcclusionCalculator.apply, 1.21.11 nachgesehen): eine Interpolation der Lichtwerte
// ueber die Ecken UND eine Abdunklung der Ecken durch angrenzende Vollwuerfel. Dieser Client
// rendert absichtlich fullbright (keine Lichtkarte), also gibt es den ersten Teil nicht. Der
// zweite haengt an keinem Lichtwert, nur an der Geometrie der Nachbarn -- und er ist es, der
// Kanten, Ecken und Nischen ihre Tiefe gibt. Genau der steht hier.
//
// Die Rechnung, nach dem Vanilla-Code:
//
//   * Jeder Block hat einen AO-Wert: 0.2 fuer einen Vollwuerfel, sonst 1.0
//     (AbstractBlock.getAmbientOcclusionLightLevel).
//   * Fuer eine Flaeche mit Richtung `d` werden die vier Nachbarn IN DER EBENE vor der Flaeche
//     gelesen (NeighborData.faces, je Richtung eine feste Reihenfolge f0..f3), dazu der Block
//     direkt davor (w) und die vier Diagonalen.
//   * Die Diagonale wird hier IMMER gelesen -- bewusst anders als Vanilla. Dort steht an ihrer
//     Stelle der Wert von f0, sobald beide Seitennachbarn eine Lage weiter draussen die Sicht
//     versperren (`n = f`, fuer alle vier Ecken f0). Diese Ersatzregel haengt an der Flaeche,
//     nicht an der Ecke: zwei benachbarte Flaechen rechnen fuer ihre GEMEINSAME Ecke dann mit
//     verschiedenen Bloecken und bekommen verschiedene Werte -- eine harte Kante mitten in
//     einer glatten Wand.
//     Vanilla verdeckt das teilweise mit der Lichtkarte, die dieser Client nicht hat. Mit der
//     echten Diagonale mitteln alle Flaechen einer Ecke ueber dieselben vier Bloecke.
//   * Eckwert = (Seite A + Seite B + Diagonale + w) / 4.
//   * Deckt ein Quad die Flaeche nicht ganz ab (Stufe, Zaunpfosten), wichtet Vanilla die vier
//     Eckwerte mit Produkten aus der Quad-Ausdehnung -- das ist eine bilineare Interpolation,
//     ausgewertet an den Quad-Ecken (s. vertex_ao).
//
// Die Richtungs-Helligkeit (oben 1.0, unten 0.5, ...) multipliziert der Mesher danach wie
// bisher; Vanilla tut dasselbe (`fs[i] * world.getBrightness(direction, shade)`).

use crate::block_registry::block_name;
use crate::state_props::property;

// Richtungsvektoren nach Atlas-Flaechenindex (assets::FACE_*): unten, oben, Nord, Sued, West, Ost.
pub const FACE_DIRS: [[i32; 3]; 6] = [[0, -1, 0], [0, 1, 0], [0, 0, -1], [0, 0, 1], [-1, 0, 0], [1, 0, 0]];

// BlockModelRenderer.NeighborData.faces, je Flaechenrichtung, als Atlas-Flaechenindizes.
// Die REIHENFOLGE ist Teil der Rechnung (s. Modulkommentar zu f0), nicht beliebig.
const NEIGHBOR_FACES: [[usize; 4]; 6] = [
    [4, 5, 2, 3], // DOWN:  WEST, EAST, NORTH, SOUTH
    [5, 4, 2, 3], // UP:    EAST, WEST, NORTH, SOUTH
    [1, 0, 5, 4], // NORTH: UP, DOWN, EAST, WEST
    [4, 5, 0, 1], // SOUTH: WEST, EAST, DOWN, UP
    [1, 0, 2, 3], // WEST:  UP, DOWN, NORTH, SOUTH
    [0, 1, 2, 3], // EAST:  DOWN, UP, NORTH, SOUTH
];

// Je State vorberechnete Eigenschaften (s. BlockShapes::ao_flags). Bitmaske statt Struktur,
// weil der Mesher sie fuer jede sichtbare Flaeche bis zu zwoelfmal nachschlaegt.
pub const AO_FULL_CUBE: u8 = 1;
pub const AO_BLOCKS_VISION: u8 = 2;
// Dieser State bekommt selbst KEINE Abdunklung: sein Modell schaltet sie ab
// ("ambientocclusion": false) oder er leuchtet (Vanilla: `state.getLuminance() == 0`).
pub const AO_SKIP: u8 = 4;
// Dunkelt dieser State seine Nachbarn ab (AO-Wert 0.2 statt 1.0)? Meist dasselbe wie
// AO_FULL_CUBE, aber NICHT immer -- s. ao_darkens.
pub const AO_DARKENS: u8 = 8;

// Vanillas AO-Wert eines Nachbarn.
pub fn ao_level(flags: u8) -> f32 {
    if flags & AO_DARKENS != 0 {
        0.2
    } else {
        1.0
    }
}

// Die Eckwerte einer Flaeche, in der Reihenfolge [f0&f2, f0&f3, f1&f2, f1&f3].
//
// `pos` ist der Block, `face` die Flaechenrichtung. `on_boundary` sagt, ob das Quad genau auf
// der Zellgrenze liegt (Vanillas field_58160): dann wird die Lage VOR dem Block gelesen, sonst
// die Lage des Blocks selbst -- eine innenliegende Stufenoberseite bei y=0.5 hat ihre
// Nachbarn ja neben sich, nicht darueber.
pub fn face_corners(pos: (i32, i32, i32), face: usize, on_boundary: bool, flags_at: impl Fn(i32, i32, i32) -> u8) -> [f32; 4] {
    let d = FACE_DIRS[face];
    let base = if on_boundary { (pos.0 + d[0], pos.1 + d[1], pos.2 + d[2]) } else { pos };
    let [n0, n1, n2, n3] = NEIGHBOR_FACES[face];
    let at = |o: [i32; 3]| flags_at(base.0 + o[0], base.1 + o[1], base.2 + o[2]);
    let add = |a: [i32; 3], b: [i32; 3]| [a[0] + b[0], a[1] + b[1], a[2] + b[2]];

    let (o0, o1, o2, o3) = (FACE_DIRS[n0], FACE_DIRS[n1], FACE_DIRS[n2], FACE_DIRS[n3]);
    let f = ao_level(at(o0));
    let g = ao_level(at(o1));
    let h = ao_level(at(o2));
    let m = ao_level(at(o3));

    // Immer die echte Diagonale, ohne Vanillas Ersatzregel (s. Modulkommentar).
    let diagonal = |a: [i32; 3], b: [i32; 3]| ao_level(at(add(a, b)));
    let n = diagonal(o0, o2);
    let p = diagonal(o0, o3);
    let r = diagonal(o1, o2);
    let t = diagonal(o1, o3);

    let w = ao_level(at([0, 0, 0]));
    [(h + f + n + w) * 0.25, (m + f + p + w) * 0.25, (h + g + r + w) * 0.25, (m + g + t + w) * 0.25]
}

// Der AO-Wert an einem Punkt der Flaeche -- `local` blocklokal 0..1. Bilinear zwischen den vier
// Eckwerten, gewichtet nach der Lage des Punkts Richtung f0/f1 bzw. f2/f3. Fuer ein Quad ueber
// die volle Flaeche kommen dabei genau die Eckwerte heraus; fuer ein kleineres entspricht es
// Vanillas Gewichten aus der Quad-Ausdehnung.
pub fn vertex_ao(face: usize, corners: [f32; 4], local: [f32; 3]) -> f32 {
    let [n0, _, n2, _] = NEIGHBOR_FACES[face];
    let gewicht = |richtung: usize| {
        let d = FACE_DIRS[richtung];
        let achse = if d[0] != 0 {
            0
        } else if d[1] != 0 {
            1
        } else {
            2
        };
        let q = local[achse].clamp(0.0, 1.0);
        if d[achse] > 0 {
            q
        } else {
            1.0 - q
        }
    };
    let a0 = gewicht(n0);
    let b2 = gewicht(n2);
    let (a1, b3) = (1.0 - a0, 1.0 - b2);
    let [c02, c03, c12, c13] = corners;
    (a0 * b2 * c02 + a0 * b3 * c03 + a1 * b2 * c12 + a1 * b3 * c13).clamp(0.0, 1.0)
}

// getAmbientOcclusionLightLevel: `isFullCube ? 0.2 : 1.0` -- ausser bei den Bloecken, die es
// ueberschreiben (per `grep getAmbientOcclusionLightLevel` ueber alle Klassen des Jars):
//
//   TransparentBlock -> 1.0   Glas, und ueber Vererbung StainedGlassBlock, TintedGlassBlock,
//                             GrateBlock/OxidizableGrateBlock (alle Kupfergitter)
//   BarrierBlock, LightBlock, StructureVoidBlock -> 1.0
//   MudBlock, SoulSandBlock -> 0.2 (obwohl ihre Kollisionsform niedriger ist)
//   SnowBlock -> 0.2 nur bei layers == 8
//
// Bewusst ANDERS als Vanilla: Pulverschnee dunkelt ab. In Vanilla tut er es nicht -- seine
// Kollisionsform ist ohne Entity leer (PowderSnowBlock.getCollisionShape), also kein
// Vollwuerfel -- und ein Hang aus Pulverschnee ist dort ganz flach. Gewuenscht ist der Look
// aus Voxy (dessen Bildschirm-Abdunklung zeigt die Stufen), so sieht der Hang aus wie einer
// aus Schneebloecken.
//
// Frueherer Fehler: Glas zaehlte hier als
// Vollwuerfel und warf deshalb AO-Schatten auf alles daneben -- in Vanilla tut es das nicht.
pub fn ao_darkens(state: u32, full_cube: bool) -> bool {
    let Some(name) = block_name(state) else { return full_cube };
    if name == "glass" || name == "tinted_glass" || name.ends_with("_stained_glass") || name.ends_with("copper_grate") {
        return false;
    }
    match name {
        "barrier" | "light" | "structure_void" => false,
        "mud" | "soul_sand" | "powder_snow" => true,
        "snow" => property(state, "layers") == Some("8"),
        _ => full_cube,
    }
}

// Leuchtet dieser State? Nur die Frage "0 oder nicht" zaehlt. Aus Blocks.java (1.21.11)
// herausgezogen: feste Werte ueber `.luminance(state -> N)`, zustandsabhaengige ueber `lit`,
// Ladungen, Beeren usw. Gefaerbte und gewachste Varianten erben ihre Einstellungen dort per
// Settings.copy und stehen deshalb hier ueber Praefix/Suffix.
pub fn emits_light(state: u32) -> bool {
    let Some(name) = block_name(state) else { return false };
    const FEST: &[&str] = &[
        "lava",
        "brown_mushroom",
        "torch",
        "wall_torch",
        "fire",
        "soul_fire",
        "soul_torch",
        "soul_wall_torch",
        "copper_torch",
        "copper_wall_torch",
        "glowstone",
        "nether_portal",
        "jack_o_lantern",
        "enchanting_table",
        "brewing_stand",
        "end_portal",
        "end_portal_frame",
        "dragon_egg",
        "ender_chest",
        "beacon",
        "sea_lantern",
        "end_rod",
        "end_gateway",
        "magma_block",
        "conduit",
        "lantern",
        "soul_lantern",
        "shroomlight",
        "crying_obsidian",
        "amethyst_cluster",
        "large_amethyst_bud",
        "medium_amethyst_bud",
        "small_amethyst_bud",
        "sculk_sensor",
        "calibrated_sculk_sensor",
        "sculk_catalyst",
        "ochre_froglight",
        "verdant_froglight",
        "pearlescent_froglight",
        "firefly_bush",
        "lava_cauldron",
        "glow_lichen",
        "trial_spawner",
        "vault",
    ];
    if FEST.contains(&name) || name.ends_with("copper_lantern") {
        return true;
    }
    let lit = || property(state, "lit") == Some("true");
    match name {
        "furnace" | "smoker" | "blast_furnace" | "redstone_ore" | "deepslate_redstone_ore" | "redstone_torch"
        | "redstone_wall_torch" | "redstone_lamp" | "campfire" | "soul_campfire" => lit(),
        "light" => property(state, "level").is_some_and(|l| l != "0"),
        "sea_pickle" => property(state, "waterlogged") == Some("true"),
        "respawn_anchor" => property(state, "charges").is_some_and(|c| c != "0"),
        "cave_vines" | "cave_vines_plant" => property(state, "berries") == Some("true"),
        _ if name.ends_with("copper_bulb") => lit(),
        _ if name.ends_with("candle") || name.ends_with("candle_cake") => lit(),
        _ => false,
    }
}

// Blockiert dieser State die Sicht (shouldBlockVision)? Vanillas Vorgabe ist
// `blocksMovement && isFullCube` -- `full_cube` uebergibt der Aufrufer. Die Ausnahmen stehen
// in Blocks.java einzeln an den Bloecken (`.blockVision(...)`).
pub fn blocks_vision(state: u32, full_cube: bool) -> bool {
    let Some(name) = block_name(state) else { return full_cube };
    // Blocks::never
    if name == "glass"
        || name == "tinted_glass"
        || name.ends_with("_stained_glass")
        || name.ends_with("_leaves")
        || name == "mangrove_roots"
        || name.ends_with("copper_grate")
        || name == "moving_piston"
        || name == "trial_spawner"
        || name == "vault"
        || name == "firefly_bush"
        || name == "test_instance_block"
    {
        return false;
    }
    // Blocks::always -- Bloecke, die niedriger als ein Wuerfel sind, aber trotzdem die Sicht
    // nehmen.
    if matches!(name, "farmland" | "soul_sand" | "dirt_path" | "mud") {
        return true;
    }
    match name {
        "snow" => property(state, "layers") == Some("8"),
        // PISTON_SUFFOCATES_PREDICATE: nur eingefahren.
        "piston" | "sticky_piston" => property(state, "extended") != Some("true") && full_cube,
        _ => full_cube,
    }
}

#[cfg(test)]
mod tests {

    #[test]
    // Zwei nebeneinanderliegende Flaechen derselben Wand muessen an ihrer gemeinsamen Kante
    // denselben Wert haben -- sonst sieht man mitten in der Wand eine harte Kante. Geprueft
    // in einer zufaellig bebauten Welt, fuer alle sechs Richtungen und beide Kantenachsen.
    fn benachbarte_flaechen_treffen_sich_ohne_kante() {
        let flags = |x: i32, y: i32, z: i32| -> u8 {
            let h = ((x.wrapping_mul(73_856_093) ^ y.wrapping_mul(19_349_663) ^ z.wrapping_mul(83_492_791)) as u32).wrapping_mul(2_654_435_761);
            if (h >> 28) < 8 {
                AO_FULL_CUBE | AO_BLOCKS_VISION | AO_DARKENS
            } else {
                0
            }
        };
        let voll = |p: [i32; 3]| flags(p[0], p[1], p[2]) & AO_FULL_CUBE != 0;
        let mut geprueft = 0;
        for face in 0..6 {
            let d = FACE_DIRS[face];
            let achse_d = (0..3).find(|&a| d[a] != 0).unwrap();
            let quer: Vec<usize> = (0..3).filter(|&a| d[a] == 0).collect();
            for (a0, a1) in [(quer[0], quer[1]), (quer[1], quer[0])] {
                for i in -6..6 {
                    for j in -6..6 {
                        let mut p = [0; 3];
                        p[a0] = i;
                        p[a1] = j;
                        let mut q = p;
                        q[a0] += 1;
                        let davor = |b: [i32; 3]| [b[0] + d[0], b[1] + d[1], b[2] + d[2]];
                        if !voll(p) || !voll(q) || voll(davor(p)) || voll(davor(q)) {
                            continue;
                        }
                        let (ep, eq) = (face_corners((p[0], p[1], p[2]), face, true, flags), face_corners((q[0], q[1], q[2]), face, true, flags));
                        for t in [0.0, 1.0] {
                            let mut lp = [0.5; 3];
                            lp[achse_d] = if d[achse_d] > 0 { 1.0 } else { 0.0 };
                            lp[a1] = t;
                            let mut lq = lp;
                            lp[a0] = 1.0;
                            lq[a0] = 0.0;
                            assert_eq!(vertex_ao(face, ep, lp), vertex_ao(face, eq, lq), "Flaeche {face} bei {p:?}");
                            geprueft += 1;
                        }
                    }
                }
            }
        }
        assert!(geprueft > 100);
    }
    use super::*;

    fn state_of(block: &str) -> u32 {
        (0..crate::block_registry::state_count() as u32).find(|&s| block_name(s) == Some(block)).expect("Block unbekannt")
    }

    const STEIN: u8 = AO_FULL_CUBE | AO_BLOCKS_VISION | AO_DARKENS;
    const LUFT: u8 = 0;

    // Ein freistehender Block auf freier Flaeche: keine Abdunklung, alle Ecken 1.0.
    // Vanillas Ueberschreibungen von getAmbientOcclusionLightLevel (s. ao_darkens).
    #[test]
    fn glas_dunkelt_nicht_ab_seelensand_schon() {
        assert!(!ao_darkens(state_of("glass"), true));
        assert!(!ao_darkens(state_of("light_gray_stained_glass"), true));
        assert!(!ao_darkens(state_of("tinted_glass"), true));
        assert!(!ao_darkens(state_of("waxed_oxidized_copper_grate"), true));
        assert!(!ao_darkens(state_of("barrier"), true));
        assert!(ao_darkens(state_of("soul_sand"), false));
        assert!(ao_darkens(state_of("powder_snow"), false));
        assert!(ao_darkens(state_of("stone"), true));
        assert!(ao_darkens(state_of("oak_leaves"), true), "Laub ist KEIN TransparentBlock");
        assert_eq!(ao_level(AO_FULL_CUBE | AO_BLOCKS_VISION), 1.0, "nur AO_DARKENS zaehlt");
    }

    #[test]
    fn freie_flaeche_bleibt_hell() {
        let ecken = face_corners((0, 64, 0), 1, true, |_, _, _| LUFT);
        assert_eq!(ecken, [1.0; 4]);
    }

    // Der klassische Fall: Boden mit einer Wand an der Ostseite. Die beiden Ecken an der Wand
    // dunkeln ab, die gegenueberliegenden nicht.
    #[test]
    fn eine_wand_dunkelt_die_anliegenden_ecken() {
        // Oberseite des Blocks bei (0,64,0); die Wand steht in der Lage darueber bei x=1.
        let welt = |x: i32, y: i32, _z: i32| if x == 1 && y == 65 { STEIN } else { LUFT };
        let ecken = face_corners((0, 64, 0), 1, true, welt);
        // UP: f0 = EAST (Wand), f1 = WEST.  Ecken [E&N, E&S, W&N, W&S].
        assert!(ecken[0] < 1.0 && ecken[1] < 1.0, "Ostecken muessen dunkler sein: {ecken:?}");
        assert_eq!(ecken[2], 1.0, "Westecken bleiben hell: {ecken:?}");
        assert_eq!(ecken[3], 1.0);
        // (0.2 Wand + 1.0 Nord + Diagonale 0.2 + 1.0 davor) / 4 = 0.6
        assert!((ecken[0] - 0.6).abs() < 1e-6, "{}", ecken[0]);
    }

    // Innenecke: zwei Waende treffen sich. Beide Seiten blockieren die Sicht, also liest Vanilla
    // die Diagonale NICHT, sondern nimmt f0 -- das ergibt die typisch dunkle Ecke.
    #[test]
    fn innenecke_nimmt_f0_statt_der_diagonale() {
        let welt = |x: i32, y: i32, z: i32| {
            // Ost- und Nordwand in der Lage ueber dem Boden, und darueber noch einmal
            // (die Durchsichtigkeit prueft Vanilla eine Lage weiter draussen).
            if (y == 65 || y == 66) && (x == 1 || z == -1) {
                STEIN
            } else {
                LUFT
            }
        };
        let ecken = face_corners((0, 64, 0), 1, true, welt);
        // E&N: f=0.2 (Ost), h=0.2 (Nord), n = f = 0.2, w = 1.0 -> 0.4
        assert!((ecken[0] - 0.4).abs() < 1e-6, "{ecken:?}");
    }

    // Ein Quad ueber die volle Flaeche bekommt an seinen Ecken genau die Eckwerte.
    #[test]
    fn volle_flaeche_trifft_die_eckwerte() {
        let ecken = [0.1, 0.2, 0.3, 0.4];
        // UP: f0 = EAST (+x), f2 = NORTH (-z). Ecke E&N liegt bei x=1, z=0.
        assert!((vertex_ao(1, ecken, [1.0, 1.0, 0.0]) - 0.1).abs() < 1e-6);
        assert!((vertex_ao(1, ecken, [1.0, 1.0, 1.0]) - 0.2).abs() < 1e-6);
        assert!((vertex_ao(1, ecken, [0.0, 1.0, 0.0]) - 0.3).abs() < 1e-6);
        assert!((vertex_ao(1, ecken, [0.0, 1.0, 1.0]) - 0.4).abs() < 1e-6);
        // Die Mitte ist der Mittelwert.
        assert!((vertex_ao(1, ecken, [0.5, 1.0, 0.5]) - 0.25).abs() < 1e-6);
    }

    #[test]
    fn leuchtende_bloecke_werden_erkannt() {
        assert!(emits_light(state_of("glowstone")));
        assert!(emits_light(state_of("sea_lantern")));
        assert!(!emits_light(state_of("stone")));
    }

    #[test]
    fn glas_und_laub_nehmen_keine_sicht() {
        assert!(!blocks_vision(state_of("glass"), true));
        assert!(!blocks_vision(state_of("oak_leaves"), true));
        assert!(blocks_vision(state_of("stone"), true));
        assert!(blocks_vision(state_of("farmland"), false), "Ackerboden nimmt die Sicht trotz 15/16 Hoehe");
    }
}
