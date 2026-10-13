// Wasser-/Lava-Fuellstand direkt aus der State-ID -- dieselbe Formel wie
// FlowingFluid.getLegacyLevel/getOwnHeight/getHeight im echten 1.21.11-Client (per
// Vineflower aus dem lokalen Fabric-Loom-Cache nachgesehen, s. app::physics-Kommentar).
// Fuer Anticheat-Kompatibilitaet muss die Schwimmphysik exakt denselben Fuellstand sehen
// wie der Server -- kein Runden/Vereinfachen der Werte.

use crate::block_registry::block_name;
use crate::chunk::World;
use crate::state_props::property;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FluidKind {
    Water,
    Lava,
}

// Bloecke, die IMMER von Wasser umgeben sind, aber KEINE "waterlogged"-Property tragen: in
// Vanilla existiert fuer diese Wasserpflanzen gar keine trockene Variante, deshalb kein
// Property-Flag, sondern Namens-Check. Gemeinsam genutzt von fluid_kind/own_height (fuer
// Schwimmphysik/Auftrieb) UND world::mesher (fuer die sichtbare Wasserfuellung) -- beide
// muessen exakt dieselben Bloecke als "nass" behandeln, sonst schwimmt man in Seegras, das
// optisch trocken aussieht, oder umgekehrt.
pub fn always_wet(name: &str) -> bool {
    matches!(name, "seagrass" | "tall_seagrass" | "kelp" | "kelp_plant")
}

// Blockname -> welche Fluessigkeit (None fuer alles andere, auch Luft). Erkennt auch
// waterlogged Bloecke (Zaun/Treppe/Stufe/... mit "waterlogged=true") als Wasser: in Vanilla
// traegt so ein Block ZUSAETZLICH zu seinem Block-State einen eigenen, unabhaengigen
// FluidState = Fluids.WATER.defaultFluidState() -- fuer Schwimmphysik/Auftrieb zaehlt das
// genauso wie ein echter Wasserblock, auch wenn die feste Geometrie (Zaunpfosten etc.)
// weiterhin separat kollidiert (s. collision.rs, das waterlogged bewusst NICHT begehbar
// macht -- der Pfosten selbst blockiert nach wie vor). Seegras/Kelp (s. always_wet) zaehlen
// ebenfalls als Wasser -- man kann bislang unbehelligt "trocken" durch ein Seegrasfeld
// laufen, obwohl es unter Wasser liegt.
pub fn fluid_kind(state: u32) -> Option<FluidKind> {
    if let Some(name) = block_name(state) {
        match name {
            "water" => return Some(FluidKind::Water),
            "lava" => return Some(FluidKind::Lava),
            // BubbleColumnBlock.getFluidState liefert wie Seegras/Seetang unbedingt
            // Fluids.WATER.getSource(false) (am 1.21.11-Jar nachgelesen). Bewusst HIER und
            // nicht in always_wet: das nutzt auch der Mesher, und fuer einen Block ohne
            // Modell ist dort nicht geprueft, welcher Zweig greift. Der Schub der Saeule
            // selbst (Entity.onInsideBubbleColumn) ist weiterhin nicht nachgebaut.
            "bubble_column" => return Some(FluidKind::Water),
            _ if always_wet(name) => return Some(FluidKind::Water),
            _ => {}
        }
    }
    if property(state, "waterlogged") == Some("true") {
        return Some(FluidKind::Water);
    }
    None
}

// "amount" (1..8, s. FlowingFluid.getAmount) aus der "level"-Blockstate-Property (0..15,
// dieselbe Kodierung wie FlowingFluid.getLegacyLevel: level = isSource ? 0 : 8 -
// min(amount,8) + (falling?8:0)): level=0 -> Quelle (amount=8), level 1..7 -> abnehmende
// Flusshoehe (amount=8-level), level 8..15 -> fallend (amount=16-level).
fn amount_from_level(level: u8) -> u8 {
    if level == 0 {
        8
    } else if level < 8 {
        8 - level
    } else {
        16 - level
    }
}

// Eigene Hoehe (0..1, ohne Nachbar-Check) -- FlowingFluid.getOwnHeight: amount/9. Ein echter
// "water"/"lava"-Block traegt die Fuellstufe in seiner "level"-Property; ein waterlogged
// Block bzw. eine always_wet-Wasserpflanze hat die nicht (waterlogged nur die Boolean-
// Property, Wasserpflanzen gar keine Property) -- ihr FluidState ist in Vanilla aber IMMER
// eine volle Quelle (amount=8, s. fluid_kind-Kommentar), keine Teilmenge.
pub(crate) fn own_height(state: u32) -> Option<f64> {
    if let Some(level) = property(state, "level").and_then(|s| s.parse::<u8>().ok()) {
        return Some(amount_from_level(level) as f64 / 9.0);
    }
    // Die Blasensaeule ebenso: ihr FluidState ist eine volle Wasserquelle (s. fluid_kind).
    if property(state, "waterlogged") == Some("true") || block_name(state).is_some_and(|n| always_wet(n) || n == "bubble_column") {
        return Some(8.0 / 9.0);
    }
    None
}

// Tatsaechliche Hoehe (0..1) einer Fluessigkeits-Zelle inkl. Nachbar-Check: steht direkt
// darueber dieselbe Fluessigkeit, ist die Zelle voll (1.0) -- sonst die eigene Hoehe (meist
// 8/9 fuer eine Quelle). FlowingFluid.getHeight. None = keine Fluessigkeit an dieser Stelle.
pub fn height_at(world: &World, x: i32, y: i32, z: i32) -> Option<f64> {
    let state = world.block_at(x, y, z);
    let kind = fluid_kind(state)?;
    let h = own_height(state)?;
    let same_above = fluid_kind(world.block_at(x, y + 1, z)) == Some(kind);
    Some(if same_above { 1.0 } else { h })
}

// Ist `state` fuer uns "leer oder dieselbe Fluessigkeit"? -- FlowableFluid.isEmptyOrThis.
// Nur solche Nachbarn tragen ueberhaupt zur Stroemung bei: ein Lava-Nachbar schiebt Wasser
// nicht, ein fester Block auch nicht.
fn is_empty_or_same(state: u32, kind: FluidKind) -> bool {
    match fluid_kind(state) {
        None => true,
        Some(k) => k == kind,
    }
}

// Faellt diese Fluessigkeitszelle? -- Vanillas FALLING-Property. In der Level-Kodierung
// (s. amount_from_level) heisst level >= 8 genau das. Bloecke ohne "level"-Property
// (waterlogged, Seegras) sind nie fallend.
fn is_falling(state: u32) -> bool {
    property(state, "level").and_then(|s| s.parse::<u8>().ok()).is_some_and(|level| level >= 8)
}

// Blockiert `state` den Fluss in der WAAGERECHTEN Richtung `dir`? -- FlowableFluid.isFlowBlocked
// (Yarn-1.21.11-Jar): dieselbe Fluessigkeit sperrt nie, nach OBEN sperrt alles, sonst
// sperrt eine volle, solide Seitenflaeche (isSideSolidFullSquare) -- ausser bei Eis.
//
// ZWEI ECHTE FEHLER STANDEN HIER (beide fielen als Grim-Flag beim Durchschwimmen fliessenden
// Wassers auf, etwa in einem Wasserschacht):
//
// 1. Der UP-Zweig wurde ueberhaupt ausgeloest. getVelocity ruft (Zeile 88)
//        isFlowBlocked(world, mutable, direction2) || isFlowBlocked(world, mutable.up(), direction2)
//    -- `mutable.up()` verschiebt die POSITION, `direction2` bleibt beide Male WAAGERECHT. Der
//    UP-Zweig ist von dort also unerreichbar. Wir hatten den zweiten Aufruf mit "up=true"
//    uebersetzt und damit JEDEN Nicht-Wasser-Block ueber dem Seitennachbarn als Sperre gewertet.
//    Folge: in einer Wassersaeule mit Luft daneben (ein Wasserschacht) sprang der
//    -6-Abwaertsterm an, den Vanilla dort gar nicht setzt.
//
// 2. is_solid() ("nicht Luft") statt isSideSolidFullSquare(). Ein Zaun, eine Stufe oder eine
//    Treppe neben dem Wasserfall galt damit als volle Wand.
fn is_flow_blocked(state: u32, kind: FluidKind, shapes: &crate::BlockShapes, dir: (i32, i32, i32)) -> bool {
    if fluid_kind(state) == Some(kind) {
        return false;
    }
    // IceBlock ist ausdruecklich ausgenommen -- das ist in Vanilla `ice` und das davon
    // abgeleitete `frosted_ice`; packed_ice/blue_ice sind KEINE IceBlock-Instanzen.
    if matches!(crate::block_name(state), Some("ice") | Some("frosted_ice")) {
        return false;
    }
    is_side_solid_full_square(state, shapes, dir)
}

// BlockState.isSideSolidFullSquare(world, pos, direction): deckt die Kollisionsform des Blocks
// die betreffende Seite als VOLLES 1x1-Quadrat ab? Ueber die Kollisionsboxen ausgewertet -- eine
// Teilbox muss die Flaeche also ganz ausfuellen (Vollwuerfel, geschlossene Tuer, ...), waehrend
// Zaunpfosten, Stufen oder Treppen nur einen Teil abdecken und deshalb NICHT sperren.
fn is_side_solid_full_square(state: u32, shapes: &crate::BlockShapes, dir: (i32, i32, i32)) -> bool {
    let (achse, positiv) = match dir {
        (x, _, _) if x != 0 => (0usize, x > 0),
        (_, y, _) if y != 0 => (1usize, y > 0),
        (_, _, z) => (2usize, z > 0),
    };
    // Die beiden Achsen IN der Flaeche -- dort muss die Box von 0 bis 1 reichen.
    let (a, b) = match achse {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    };
    const EPS: f32 = 1e-4;
    crate::collision::collision_boxes(state, shapes).iter().any(|k| {
        let an_der_flaeche = if positiv { k.max[achse] >= 1.0 - EPS } else { k.min[achse] <= EPS };
        an_der_flaeche && k.min[a] <= EPS && k.max[a] >= 1.0 - EPS && k.min[b] <= EPS && k.max[b] >= 1.0 - EPS
    })
}

// Stroemungsvektor einer Fluessigkeitszelle (normiert, y meist 0) -- Nachbau von
// FlowableFluid.getVelocity aus dem 1.21.11-Client.
//
// Grundgedanke: fuer jede der vier Himmelsrichtungen wird das Hoehen-GEFAELLE zum Nachbarn
// gebildet und als Schub in diese Richtung aufaddiert. Ein Nachbar ohne Fluessigkeit, durch
// den man hindurchlaufen kann, zaehlt ueber die Zelle DARUNTER mit (Wasser, das ueber eine
// Kante faellt, zieht schon vor der Kante). Faellt die Zelle selbst und ist der Fluss
// seitlich blockiert, kommt ein kraeftiger Abwaertsanteil dazu -- das ist es, was einen
// Spieler in einem Wasserfall nach unten druckt.
//
// None = an dieser Stelle ist gar keine Fluessigkeit. Wird sowohl von der Physik (Schub auf
// den Spieler) als auch spaeter vom Renderer (Drehwinkel der Fliesstextur) gebraucht.
pub fn flow_velocity(world: &World, shapes: &crate::BlockShapes, x: i32, y: i32, z: i32) -> Option<[f64; 3]> {
    flow_velocity_at(&|x, y, z| world.block_at(x, y, z), shapes, x, y, z)
}

// Wie flow_velocity, aber ueber eine Block-Closure statt ueber &World. Der Mesher laeuft auf
// Worker-Threads und hat dort gar keine World, sondern nur seine Spalte samt Nachbarn (s.
// ColumnNeighbors) -- er braucht denselben Vektor aber fuer den Drehwinkel der Fliesstextur.
// Deshalb steckt die Logik hier und nicht in der Physik: eine Quelle fuer beide.
pub fn flow_velocity_at(block_at: &impl Fn(i32, i32, i32) -> u32, shapes: &crate::BlockShapes, x: i32, y: i32, z: i32) -> Option<[f64; 3]> {
    let state = block_at(x, y, z);
    let kind = fluid_kind(state)?;
    let own = own_height(state)?;

    let (mut dx, mut dz) = (0.0f64, 0.0f64);
    for (ox, oz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
        let nachbar = block_at(x + ox, y, z + oz);
        if !is_empty_or_same(nachbar, kind) {
            continue;
        }
        // Vanilla nutzt hier FluidState.getHeight() OHNE Welt/Position, also die reine
        // Eigenhoehe -- nicht height_at(), das zusaetzlich nach oben schaut.
        let h = own_height(nachbar).unwrap_or(0.0);
        let gefaelle = if h == 0.0 {
            // Kein Fluid im Nachbarn: nur wenn man da ueberhaupt hindurchkaeme, zaehlt die
            // Zelle darunter -- sonst steht dort eine Wand und es gibt kein Gefaelle.
            //
            // blocksMovement() -- exakt Vanillas Regel ueber die Kollisionsform, s.
            // collision::blocks_movement. Hier stand frueher is_solid() ("nicht Luft"): eine
            // Blume oder Fackel neben dem Wasser galt damit als Wand, die Zelle darunter fiel
            // aus der Rechnung, und der Stroemungsvektor wich von dem des Servers ab.
            if crate::collision::blocks_movement(nachbar, shapes) {
                continue;
            }
            let unten = block_at(x + ox, y - 1, z + oz);
            if !is_empty_or_same(unten, kind) {
                continue;
            }
            let hu = own_height(unten).unwrap_or(0.0);
            // 0.8888889 = 8/9, die Hoehe einer vollen Quelle: der Sprung ueber die Kante
            // wird gegen eine volle Zelle gerechnet, nicht gegen Null.
            if hu > 0.0 {
                own - (hu - 8.0 / 9.0)
            } else {
                0.0
            }
        } else {
            own - h
        };
        if gefaelle != 0.0 {
            dx += ox as f64 * gefaelle;
            dz += oz as f64 * gefaelle;
        }
    }

    let mut v = [dx, 0.0, dz];
    if is_falling(state) {
        // Fallendes Wasser, dessen Fluss seitlich (auf eigener Hoehe ODER eine darueber)
        // ansteht: kraeftig nach unten. Erst normieren, dann -6 auf y -- so herum macht es
        // Vanilla, das Ergebnis ist danach fast senkrecht.
        // Beide Aufrufe mit DERSELBEN waagerechten Richtung -- der zweite schaut nur eine Zelle
        // hoeher, nicht in eine andere Richtung (s. is_flow_blocked).
        let blockiert = [(1, 0), (-1, 0), (0, 1), (0, -1)].into_iter().any(|(ox, oz)| {
            let dir = (ox, 0, oz);
            is_flow_blocked(block_at(x + ox, y, z + oz), kind, shapes, dir)
                || is_flow_blocked(block_at(x + ox, y + 1, z + oz), kind, shapes, dir)
        });
        if blockiert {
            v = normiert(v);
            v[1] -= 6.0;
        }
    }
    Some(normiert(v))
}

fn normiert(v: [f64; 3]) -> [f64; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    // Vanillas Vec3d.normalize() gibt bei Laenge ~0 den Nullvektor zurueck, nicht NaN.
    if len < 1e-8 {
        [0.0; 3]
    } else {
        [v[0] / len, v[1] / len, v[2] / len]
    }
}

// Nachbau von FluidRenderer.getFluidHeight (Yarn-1.21.11-Jar nachgesehen) fuer die
// Eck-Hoehen-Mittelung der sichtbaren Fluessigkeitsoberflaeche (s. world::mesher::
// push_fluid_quads): dieselbe Fluessigkeit an `pos` -> voll (1.0), falls direkt darueber
// wieder dieselbe Fluessigkeit steht, sonst die eigene Fuellhoehe; andere Fluessigkeit/Block
// -> -1.0 bei "solide" (nimmt nicht an der Mittelung teil), sonst 0.0 (Luft, zieht die Ecke
// nach unten).
// Vanillas "isSolid()" ist eine Materialeigenschaft (nicht exakt "kein Luft") -- hier per
// is_solid() angenaehert (alles ausser Luft gilt als "solide" genug, um KEIN Nullhoehen-
// Nachbar zu sein).
//
// `kind` statt fest Wasser: Vanillas FluidRenderer.render zeichnet Lava mit EXAKT derselben
// Geometrie wie Wasser (dieselbe Methode, die Frage "ist das Lava?"
// entscheidet nur ueber Sprites, Tint und die water_overlay-Ausnahme) -- vorher stand hier
// FluidKind::Water fest verdrahtet und Lava blieb beim Vollwuerfel-Fallback, also einen
// ganzen Block hoch statt 8/9.
pub(crate) fn render_height_at(state: u32, above_state: u32, kind: FluidKind, is_solid: impl Fn(u32) -> bool) -> f32 {
    if fluid_kind(state) == Some(kind) {
        if fluid_kind(above_state) == Some(kind) {
            1.0
        } else {
            own_height(state).unwrap_or(0.0) as f32
        }
    } else if is_solid(state) {
        -1.0
    } else {
        0.0
    }
}

// FluidRenderer.addHeight: gewichtete Summierung, Gewicht 10 statt 1 fuer "fast volle"
// Nachbarn (>=0.8) -- laesst eine einzelne Quellzelle (8/9=0.888..) staerker in die
// Eck-Mittelung einfliessen als mehrere duenne Fliess-Nachbarn, sonst wirkte eine Quelle
// neben einem duennen Rinnsal optisch zu schwach verwaesserst.
fn add_height(weighted: &mut [f32; 2], height: f32) {
    if height >= 0.8 {
        weighted[0] += height * 10.0;
        weighted[1] += 10.0;
    } else if height >= 0.0 {
        weighted[0] += height;
        weighted[1] += 1.0;
    }
}

// FluidRenderer.calculateFluidHeight: eine der vier Eckhoehen aus der eigenen Zelle
// (`origin`), den beiden orthogonalen Nachbarn (`ns`/`ew`) und -- falls beide Nachbarn
// ueberhaupt Wasser fuehren -- der Diagonalzelle (`diag`) mitteln. >=1.0 bei irgendeinem
// vollen orthogonalen Nachbarn (die Ecke haengt dann automatisch an einer vollen Kante).
pub(crate) fn calculate_corner_height(origin: f32, ns: f32, ew: f32, diag: f32) -> f32 {
    if ew >= 1.0 || ns >= 1.0 {
        return 1.0;
    }
    let mut weighted = [0.0f32; 2];
    if ew > 0.0 || ns > 0.0 {
        if diag >= 1.0 {
            return 1.0;
        }
        add_height(&mut weighted, diag);
    }
    add_height(&mut weighted, origin);
    add_height(&mut weighted, ew);
    add_height(&mut weighted, ns);
    weighted[0] / weighted[1]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block_registry::state_count;

    // Beide orthogonalen Nachbarn (Nord/Sued ODER Ost/West -- hier per ns/ew simuliert) voll
    // -> die Ecke haengt an einer vollen Kante und muss unabhaengig von der eigenen Hoehe
    // voll sein (calculateFluidHeight()s frueher Ausstieg, s. Funktionskommentar).
    #[test]
    fn ecke_an_voller_kante_ist_immer_voll() {
        assert_eq!(calculate_corner_height(0.5, 1.0, 0.5, 0.0), 1.0);
        assert_eq!(calculate_corner_height(0.5, 0.5, 1.0, 0.0), 1.0);
    }

    // Eine isolierte Quelle (8/9) ohne Wassernachbarn: die Diagonale traegt nichts bei (ew
    // und ns sind beide 0), nur origin + zwei Null-Nachbarn mitteln -- die Ecke sackt unter
    // die Quellhoehe ab (Vanillas "Quelle ragt an ihren Raendern nicht randscharf", s.
    // addHeight-Gewichtung: Quelle zaehlt 10-fach, die beiden Luft-Nachbarn 1-fach je).
    #[test]
    fn isolierte_quelle_sackt_an_den_ecken_ab() {
        let source = 8.0 / 9.0;
        let corner = calculate_corner_height(source, 0.0, 0.0, 0.0);
        assert!(corner < source, "Ecke ({corner}) muss unter der Quellhoehe ({source}) liegen");
        assert!(corner > 0.0, "Ecke darf nicht auf Null fallen (die Quelle selbst zieht kraeftig)");
        // Exakter Erwartungswert (10*8/9)/(10+1+1) -- per Hand nachgerechnet, kein Rateswert.
        assert!((corner - (10.0 * source) / 12.0).abs() < 1e-6);
    }

    // render_height_at: dieselbe Fluessigkeit direkt darueber -> voll (1.0), unabhaengig von
    // der eigenen Level-Property (deckt sich mit fluid::height_at() fuer die Physik).
    #[test]
    fn render_height_voll_wenn_gleiche_fluessigkeit_darueber() {
        let water = (0..state_count() as u32).find(|&s| block_name(s) == Some("water") && property(s, "level") == Some("0")).unwrap();
        assert_eq!(render_height_at(water, water, FluidKind::Water, |_| true), 1.0);
    }

    // render_height_at: kein Wasser an der Stelle selbst -> solide zaehlt -1 (nimmt nicht an
    // der Eck-Mittelung teil), Luft zaehlt 0 (zieht die Ecke nach unten, s.
    // calculate_corner_height-Kommentar).
    #[test]
    fn render_height_unterscheidet_solide_und_luft() {
        let stone = (0..state_count() as u32).find(|&s| block_name(s) == Some("stone")).unwrap();
        let air = 0u32; // STATE_AIR ist per Konvention 0, s. world::chunk
        assert_eq!(render_height_at(stone, air, FluidKind::Water, |s| s == stone), -1.0);
        assert_eq!(render_height_at(air, air, FluidKind::Water, |_| false), 0.0);
    }

    // --- Stroemung (flow_velocity) -------------------------------------------------------

    // Welt aus einer Spalte, in die man einzelne States setzen kann. Wasser-Levels bewusst
    // von Hand gesetzt statt Fliess-Simulation -- der Server schickt uns ohnehin fertige
    // States, die Ausbreitungslogik ist gar nicht unsere Aufgabe.
    fn welt_mit(bloecke: &[(i32, i32, i32, u32)]) -> World {
        let mut w = World::new();
        for cx in -1..=1 {
            for cz in -1..=1 {
                let mut sections = Vec::new();
                for i in 0..24 {
                    let mut states = vec![0u32; crate::chunk::SECTION_VOLUME];
                    if cx == 0 && cz == 0 && i == 4 {
                        // Section 4 beginnt bei MIN_Y + 4*16 = 0.
                        for &(x, y, z, s) in bloecke {
                            states[((y * 16 + z) * 16 + x) as usize] = s;
                        }
                    }
                    sections.push(crate::chunk::ChunkSection::from_states(states));
                }
                w.insert_column(cx, cz, sections);
            }
        }
        w
    }

    fn wasser_mit_level(level: u8) -> u32 {
        (0..state_count() as u32)
            .find(|&s| block_name(s) == Some("water") && property(s, "level") == Some(&level.to_string()))
            .unwrap_or_else(|| panic!("water level={level} nicht gefunden"))
    }

    // Stehendes Wasser ohne Gefaelle darf niemanden schieben -- sonst triebe man in einem
    // ruhigen See dauernd zur Seite.
    #[test]
    fn ruhiges_wasser_hat_keine_stroemung() {
        let quelle = wasser_mit_level(0);
        let bloecke: Vec<_> = (0..3).flat_map(|x| (0..3).map(move |z| (x, 1, z, quelle))).collect();
        let w = welt_mit(&bloecke);
        let v = flow_velocity(&w, &crate::BlockShapes::all_full_cubes(), 1, 1, 1).expect("Wasser erwartet");
        assert!(v[0].abs() < 1e-9 && v[2].abs() < 1e-9, "ruhiges Wasser darf nicht schieben, war {v:?}");
    }

    // Quelle links, duennerer Fluss rechts: das Gefaelle zeigt VON der Quelle WEG, der Schub
    // muss also nach +X gehen. Das ist die Grundrichtung, an der man merkt, ob das Vorzeichen
    // stimmt -- ein Dreher hier truege den Spieler flussaufwaerts.
    #[test]
    fn gefaelle_schiebt_vom_hohen_zum_niedrigen_stand() {
        // x=0 volle Quelle, x=1 level=1 (etwas niedriger), x=2 level=2 (noch niedriger).
        let w = welt_mit(&[(0, 1, 1, wasser_mit_level(0)), (1, 1, 1, wasser_mit_level(1)), (2, 1, 1, wasser_mit_level(2))]);
        let v = flow_velocity(&w, &crate::BlockShapes::all_full_cubes(), 1, 1, 1).expect("Wasser erwartet");
        assert!(v[0] > 0.5, "Schub muss deutlich nach +X (weg von der Quelle) zeigen, war {v:?}");
        assert!(v[2].abs() < 1e-9, "ohne Gefaelle in Z darf kein Z-Anteil entstehen, war {v:?}");
    }

    // DER Fall, an dem die alte Naeherung hing: neben dem Wasser steht etwas, das man
    // DURCHQUEREN kann (Fackel, Blume). Vanilla fragt blocksMovement() und zaehlt deshalb die
    // Zelle DARUNTER mit -- das Wasser zieht Richtung Kante. Unser frueheres is_solid() war
    // bloss "nicht Luft": die Fackel galt als Wand, der Zug fiel weg, und die Position lief
    // gegenueber dem Server auseinander (Grim-Flag beim Durchschwimmen
    // fliessenden Wassers).
    //
    // Geprueft wird gegen die BEIDEN Gegenproben: mit Luft (dort MUSS gezogen werden) und mit
    // Stein (dort darf nicht gezogen werden). Die Fackel muss sich wie Luft verhalten.
    #[test]
    fn durchquerbarer_nachbar_zaehlt_wie_luft() {
        let quelle = wasser_mit_level(0);
        let shapes = crate::BlockShapes::all_full_cubes();
        let stein = (0..state_count() as u32).find(|&s| block_name(s) == Some("stone")).expect("stone");
        let fackel = (0..state_count() as u32).find(|&s| block_name(s) == Some("torch")).expect("torch");

        // Wasser bei (1,1,1); oestlich daneben (2,1,1) der Testblock, darunter (2,0,1) Wasser
        // -- das ist die Kante, ueber die Vanilla den Sog rechnet.
        let zug = |nachbar: Option<u32>| {
            let mut bloecke = vec![(1, 1, 1, quelle), (2, 0, 1, quelle)];
            if let Some(n) = nachbar {
                bloecke.push((2, 1, 1, n));
            }
            let w = welt_mit(&bloecke);
            flow_velocity(&w, &shapes, 1, 1, 1).expect("Wasser erwartet")[0]
        };

        let mit_luft = zug(None);
        assert!(mit_luft > 0.5, "ueber eine offene Kante muss deutlich nach +X gezogen werden, war {mit_luft}");
        assert!(zug(Some(stein)).abs() < 1e-9, "hinter Stein gibt es keine Kante -- kein Zug");
        assert!(
            (zug(Some(fackel)) - mit_luft).abs() < 1e-9,
            "eine Fackel blockiert die Bewegung nicht, muss also wie Luft wirken (war {}, Luft {mit_luft})",
            zug(Some(fackel))
        );
    }

    // DER Fall aus dem Grim-Debug-Paste: ein senkrechter Schacht aus FALLENDEM Wasser
    // (level=8), daneben Luft. Vanilla setzt hier KEINEN -6-Abwaertsterm, weil Luft keine
    // volle Seitenflaeche hat -- weder neben der Zelle noch eine Zelle darueber.
    //
    // Wir taten es trotzdem: der zweite isFlowBlocked-Aufruf war faelschlich als "Richtung
    // OBEN" uebersetzt und meldete damit fuer JEDEN Nicht-Wasser-Block darueber eine Sperre.
    // Ergebnis war ein kraeftiger senkrechter Schub, den der Server nicht rechnet.
    #[test]
    fn fallendes_wasser_neben_luft_bekommt_keinen_abwaertsterm() {
        let fallend = wasser_mit_level(8);
        let shapes = crate::BlockShapes::all_full_cubes();
        // 3x3-Saeule fallenden Wassers bei y=1; die Zelle (1,1,0) hat noerdlich Luft, und
        // ueber dieser Luft ist ebenfalls Luft -- exakt die Lage aus dem Paste.
        let bloecke: Vec<_> = (0..3).flat_map(|x| (1..3).map(move |z| (x, 1, z, fallend))).collect();
        let w = welt_mit(&bloecke);
        let v = flow_velocity(&w, &shapes, 1, 1, 1).expect("Wasser erwartet");
        assert!(v[1].abs() < 1e-9, "ohne volle Seitenflaeche daneben darf es keinen senkrechten Anteil geben, war {v:?}");
    }
    // Die Gegenprobe (echte Wand -> der -6-Term MUSS greifen) steht weiter unten als
    // fallendes_wasser_an_einer_wand_zieht_nach_unten -- zusammen belegen die beiden, dass der
    // Term korrigiert und nicht einfach abgeschaltet wurde.

    // Und die Feinheit, die is_solid() vorher verschluckt hat: eine STUFE deckt die Seite nur
    // halb ab, ist also keine volle Seitenflaeche -- kein Abwaertsterm. Vorher galt sie als
    // Wand, weil sie schlicht "nicht Luft" ist.
    #[test]
    fn halbhohe_nachbarn_sperren_den_fluss_nicht() {
        let fallend = wasser_mit_level(8);
        let stufe = (0..state_count() as u32)
            .find(|&s| block_name(s) == Some("oak_slab") && property(s, "type") == Some("bottom"))
            .expect("oak_slab");
        let untere_haelfte = crate::block_shapes::ShapeBox {
            min: [0.0, 0.0, 0.0],
            max: [1.0, 0.5, 1.0],
            faces: [true; 6],
            content: [false; 6],
            uv: [None; 6],
        };
        let mut per_state = vec![None; state_count()];
        per_state[stufe as usize] = Some(vec![untere_haelfte]);
        let shapes = crate::BlockShapes::new(per_state);

        let w = welt_mit(&[(1, 1, 1, fallend), (0, 1, 1, stufe), (2, 1, 1, stufe), (1, 1, 0, stufe), (1, 1, 2, stufe)]);
        let v = flow_velocity(&w, &shapes, 1, 1, 1).expect("Wasser erwartet");
        assert!(v[1].abs() < 1e-9, "eine halbhohe Stufe ist keine volle Seitenflaeche, war {v:?}");
    }

    // Das Ergebnis ist immer normiert (Vanillas getVelocity endet auf .normalize()) -- die
    // Staerke kommt erst spaeter ueber den Schub-Faktor 0.014 dazu, nicht aus dem Gefaelle.
    #[test]
    fn stroemung_ist_normiert() {
        let w = welt_mit(&[(0, 1, 1, wasser_mit_level(0)), (1, 1, 1, wasser_mit_level(1))]);
        let v = flow_velocity(&w, &crate::BlockShapes::all_full_cubes(), 1, 1, 1).expect("Wasser erwartet");
        let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        assert!((len - 1.0).abs() < 1e-9, "Laenge muss 1 sein, war {len}");
    }

    // Ohne Fluessigkeit gibt es keinen Vektor -- der Aufrufer (Physik) darf sich darauf
    // verlassen, dass None wirklich "hier ist nichts" heisst und nicht "Stroemung null".
    #[test]
    fn ohne_wasser_kein_vektor() {
        let w = welt_mit(&[]);
        assert_eq!(flow_velocity(&w, &crate::BlockShapes::all_full_cubes(), 1, 1, 1), None);
    }

    // Fallendes Wasser (level >= 8) an einer Wand zieht kraeftig nach unten -- das ist der
    // Anteil, der einen im Wasserfall nach unten drueckt.
    #[test]
    fn fallendes_wasser_an_einer_wand_zieht_nach_unten() {
        let stein = (0..state_count() as u32).find(|&s| block_name(s) == Some("stone")).unwrap();
        // Fallendes Wasser bei (1,1,1), daneben eine Steinwand -> Fluss seitlich blockiert.
        let w = welt_mit(&[(1, 1, 1, wasser_mit_level(8)), (2, 1, 1, stein), (2, 2, 1, stein)]);
        let v = flow_velocity(&w, &crate::BlockShapes::all_full_cubes(), 1, 1, 1).expect("Wasser erwartet");
        assert!(v[1] < -0.9, "fallendes Wasser an einer Wand muss stark nach unten ziehen, war {v:?}");
    }

    #[test]
    fn amount_aus_level_stimmt() {
        assert_eq!(amount_from_level(0), 8); // Quelle
        assert_eq!(amount_from_level(1), 7);
        assert_eq!(amount_from_level(7), 1);
        assert_eq!(amount_from_level(8), 8); // fallende Quelle
        assert_eq!(amount_from_level(15), 1);
    }

    // Waterlogged-Bloecke (Zaun/Treppe/Stufe/...) muessen fuer die Schwimmphysik als Wasser
    // zaehlen -- auch ohne "level"-Property, s. fluid_kind-Kommentar.
    #[test]
    fn waterlogged_zaunstate_zaehlt_als_wasser() {
        let waterlogged_fence = (0..state_count() as u32)
            .find(|&s| block_name(s) == Some("oak_fence") && property(s, "waterlogged") == Some("true"))
            .expect("waterlogged oak_fence-State nicht gefunden");
        assert_eq!(fluid_kind(waterlogged_fence), Some(FluidKind::Water));
        assert!((own_height(waterlogged_fence).unwrap() - 8.0 / 9.0).abs() < 1e-9);

        let dry_fence = (0..state_count() as u32)
            .find(|&s| block_name(s) == Some("oak_fence") && property(s, "waterlogged") == Some("false"))
            .expect("trockener oak_fence-State nicht gefunden");
        assert_eq!(fluid_kind(dry_fence), None);
    }

    // BubbleColumnBlock.getFluidState liefert unbedingt eine Wasserquelle, genau wie Seegras.
    // Ohne Eintrag hielt die Schwimmphysik eine Blasensaeule fuer Luft.
    #[test]
    fn blasensaeule_zaehlt_als_wasser() {
        let state = (0..state_count() as u32).find(|&s| block_name(s) == Some("bubble_column")).expect("bubble_column");
        assert_eq!(fluid_kind(state), Some(FluidKind::Water));        assert_eq!(fluid_kind(state), Some(FluidKind::Water));
        // Volle Quelle -- sonst haette sie keine Oberflaeche (Loch im Wasser).
        assert!((own_height(state).unwrap() - 8.0 / 9.0).abs() < 1e-9);
    }

    // Regressionstest: im Seegras lief man, als sei da kein Wasser. Seegras/Kelp haben KEINE "waterlogged"-
    // Property (existieren in Vanilla nie trocken) -- ohne always_wet blieb fluid_kind() fuer
    // sie None, die Schwimmphysik hielt die Zelle faelschlich fuer Luft.
    #[test]
    fn seegras_und_kelp_zaehlen_als_wasser() {
        for name in ["seagrass", "kelp"] {
            let state =
                (0..state_count() as u32).find(|&s| block_name(s) == Some(name)).unwrap_or_else(|| panic!("{name}-State nicht gefunden"));
            assert_eq!(fluid_kind(state), Some(FluidKind::Water), "{name} muss als Wasser zaehlen");
            assert!((own_height(state).unwrap() - 8.0 / 9.0).abs() < 1e-9, "{name}: falsche Fuellhoehe");
        }
    }
}
