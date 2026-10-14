// Item-Icons fuer Hotbar und Inventar: aufgeloest beim Start (assets::build_ui_atlas),
// gezeichnet als texturierte Quads (s. sprite.wgsl).
//
// Zwei Sorten, genau wie in Vanilla:
//   * flache Sprites (Schwert, Apfel, ...) -- ein bis mehrere Lagen aus dem UI-Atlas
//   * Bloecke -- KEIN eigenes Bild, sondern derselbe Block-Atlas wie die Welt, als
//     isometrischer Wuerfel aus drei Parallelogrammen (oben/links/rechts) mit Vanillas
//     Flaechen-Schattierung. Deshalb kommen Blockitems ohne zusaetzliche Texturen aus und
//     folgen automatisch einem geladenen Resourcepack.
//
// Toenung (Gras, Blaetter, Trankfuellung) steckt schon in den IconLayer-Farben, die
// assets::ui_atlas aus der "tints"-Liste des Item-Modells aufgeloest hat -- hier wird sie
// nur noch mit der Flaechenhelligkeit und dem Alpha multipliziert.

use std::collections::HashMap;

use assets::{IconLayer, IconQuad, ItemDisplay, ItemIcon, UvRect, FACE_DOWN, FACE_EAST, FACE_NORTH, FACE_SOUTH, FACE_UP, FACE_WEST};

// Welcher Atlas fuer ein Quad gebunden werden muss.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SpriteAtlas {
    Ui,
    Block,
    // Skins (Koepfe in der Tab-Liste).
    Entity,
    // Verzauberungs-Glint ueber einem Icon aus dem UI- bzw. Blockatlas (s. sprite.wgsl::
    // fs_glint): das Quad sampelt den genannten Atlas nur als MASKE, die Farbe kommt aus der
    // Glint-Textur.
    GlintUi,
    GlintBlock,
    // Dasselbe fuer Icons aus dem Entity-Atlas: das Schild (und Koepfe) werden aus ihren
    // Entity-Texturen gezeichnet -- ohne diesen Zwilling glaenzte ein verzaubertes Schild im
    // Inventar nicht.
    GlintEntity,
    // Server-Icons der Server-Liste (eigener Atlas, s. GpuState::upload_server_icon).
    ServerIcons,
    // Pack-Bilder im Skin-Tab (eigener Atlas, s. GpuState::upload_pack_icon).
    PackIcons,
    // GitHub- und Discord-Logo der Kategorienleiste (eigener Atlas, s. GpuState::upload_logos).
    Logos,
}

impl SpriteAtlas {
    pub fn is_glint(self) -> bool {
        matches!(self, SpriteAtlas::GlintUi | SpriteAtlas::GlintBlock | SpriteAtlas::GlintEntity)
    }

    // Der Glint-Zwilling eines Icon-Quads aus diesem Atlas. Bestellt wird er nur fuer
    // verzauberte Items (s. App::push_item) -- Koepfe in der Tab-Liste glitzern also nie.
    pub fn glint(self) -> Option<SpriteAtlas> {
        match self {
            SpriteAtlas::Ui => Some(SpriteAtlas::GlintUi),
            SpriteAtlas::Block => Some(SpriteAtlas::GlintBlock),
            SpriteAtlas::Entity => Some(SpriteAtlas::GlintEntity),
            _ => None,
        }
    }
}

// Ein texturiertes Quad der Oberflaeche. Vier Ecken statt eines Rechtecks, weil die
// Wuerfelseiten Parallelogramme sind.
#[derive(Clone, Copy, Debug)]
pub struct UiSprite {
    // Bildschirmpixel, Reihenfolge: links oben, rechts oben, rechts unten, links unten
    // (im Uhrzeigersinn) -- die UV-Ecken werden in derselben Reihenfolge zugeordnet.
    pub corners: [[f32; 2]; 4],
    pub uv: UvRect,
    pub color: [f32; 4],
    pub atlas: SpriteAtlas,
}

impl UiSprite {
    pub fn quad(x: f32, y: f32, w: f32, h: f32, uv: UvRect, color: [f32; 4], atlas: SpriteAtlas) -> Self {
        Self { corners: [[x, y], [x + w, y], [x + w, y + h], [x, y + h]], uv, color, atlas }
    }
}

// Flaechenhelligkeit des Wuerfels. Das sind Vanillas Blockmodell-Konstanten (oben 1.0,
// Nord/Sued 0.8, Ost/West 0.6) -- NICHT die Zwei-Licht-Diffusbeleuchtung, mit der Vanilla
// Item-Modelle im Inventar tatsaechlich ausleuchtet (DiffuseLighting, Typ ITEMS_3D). Die
// Abstufung ist dieselbe und der vertraute Wuerfel-Look kommt heraus; exakt waere es erst
// mit den beiden Lichtvektoren und dem Shader-Term.
const SHADE_TOP: f32 = 1.0;
const SHADE_NORTH: f32 = 0.8;
const SHADE_EAST: f32 = 0.6;

// Die Flaechenhelligkeit gilt wie bei Vanilla im sRGB-Raum -- der Shader multipliziert aber mit
// der LINEAR gelesenen Textur (wie bei linear_tint). Unumgerechnet wirkte 0.6 wie gut 0.8 und die
// Seiten waren kaum dunkler als die Deckflaeche.
fn linear_shade(shade: f32) -> f32 {
    world::sky::srgb_to_linear(shade)
}

// Toenungen (Trankfarbe, Gras, Leder ...) stehen als sRGB-Werte in den Modellen. Der Shader
// multipliziert aber mit der schon LINEAR gelesenen Textur -- ungewandelt war der Faktor zu
// hell und jede Farbe wirkte ausgewaschen. Vanilla
// multipliziert im sRGB-Raum; linear(Textur) * linear(Farbe) ist genau das.
fn linear_tint(t: [f32; 3]) -> [f32; 3] {
    t.map(world::sky::srgb_to_linear)
}

pub struct ItemIcons {
    icons: HashMap<String, ItemIcon>,
    // Items, die Vanilla ueber ein Entity-Modell zeichnet statt ueber ein Item-Modell (Schild,
    // s. world::entity_model::shield_model). Flaechen im Item-Raum, die gui-Transformation kommt
    // beim Zeichnen dazu. Vorrang vor `icons` -- dort steht fuer diese Items nur ein Notbehelf.
    // Die gui-Haltung steht dabei, wo sie nicht aus dem Icon kommt (Truhe, Shulkerkiste und
    // Kupfergolem-Statue haben dort ein Block-Icon ohne Haltung).
    entity_models: HashMap<String, (Vec<EntityQuad>, Option<ItemDisplay>)>,
    // Item-Modell-Definitionen je ID ("mace", "uhc:storm_wall"), s. push_stack.
    definitions: HashMap<String, assets::ItemDefinition>,
}

// Eine Flaeche eines Entity-Modells im Item-Raum, UV im Entity-Atlas.
struct EntityQuad {
    // Reihenfolge wie UiSprite: die Ecken zu uv (u0,v0), (u1,v0), (u1,v1), (u0,v1).
    corners: [[f32; 3]; 4],
    uv: UvRect,
    // Mitte des Kastens, zu dem die Flaeche gehoert -- daraus die Aussennormale.
    box_center: [f32; 3],
}

impl ItemIcons {
    pub fn new((icons, definitions): (HashMap<String, ItemIcon>, HashMap<String, assets::ItemDefinition>)) -> Self {
        Self { icons, entity_models: HashMap::new(), definitions }
    }

    pub fn definition(&self, id: &str) -> Option<&assets::ItemDefinition> {
        self.definitions.get(id)
    }

    // Die Icon-Schluessel, die die Definition `id` fuer diesen Stapel zeigt (mehrere bei
    // composite), unten zuerst; None an einer Stelle = das im Buendel gewaehlte Item. None ohne
    // Definition oder wenn kein Zweig ein Icon hat -- dann bleibt es beim Icon unter dem Namen.
    pub fn stack_keys(&self, id: &str, input: &dyn assets::ItemModelInput) -> Option<Vec<Option<&str>>> {
        let def = self.definitions.get(id)?;
        let keys: Vec<Option<&str>> = def
            .evaluate(input)
            .into_iter()
            .filter_map(|i| match i {
                assets::SELECTED_ITEM => Some(None),
                i => def.icon_keys.get(i as usize).map(|k| Some(k.as_str())).filter(|k| k.is_some_and(|k| self.icons.contains_key(k))),
            })
            .collect();
        keys.iter().any(Option::is_some).then_some(keys)
    }

    // Entity-Modell als Icon fuer `item` hinterlegen. `boxes` sind die Vertices je Kasten, wie
    // world::emit_entity_model sie liefert (sechs je Flaeche: Dreiecke 0,1,2 und 0,2,3), mit der
    // Einbettung des Sondermodells in den Item-Raum schon angewandt.
    pub fn set_entity_model(&mut self, item: &str, boxes: &[Vec<world::BlockVertex>]) {
        self.entity_models.insert(item.to_string(), (entity_quads(boxes), None));
    }

    // Dasselbe mit eigener gui-Haltung (Sondermodelle, deren Icon keine traegt).
    pub fn set_entity_model_gui(&mut self, item: &str, boxes: &[Vec<world::BlockVertex>], gui: ItemDisplay) {
        self.entity_models.insert(item.to_string(), (entity_quads(boxes), Some(gui)));
    }
}

// Die Vertices je Kasten (sechs je Flaeche, wie world::emit_entity_model sie liefert) als
// Flaechen mit Aussennormale.
fn entity_quads(boxes: &[Vec<world::BlockVertex>]) -> Vec<EntityQuad> {
    let mut quads = Vec::new();
    for vertices in boxes {
        if vertices.is_empty() {
            continue;
        }
        let n = vertices.len() as f32;
        let box_center = [0, 1, 2].map(|a| vertices.iter().map(|v| v.position[a]).sum::<f32>() / n);
        for face in vertices.chunks_exact(6) {
            // Viereck 0..3 = Vertices 0, 1, 2, 5. Vanillas Quad: Ecke 1 traegt (u1,v1),
            // 0 (u2,v1), 3 (u2,v2), 2 (u1,v2) -- also im UiSprite-Umlauf 1, 0, 3, 2.
            let (a, b, c, d) = (&face[1], &face[0], &face[5], &face[2]);
            quads.push(EntityQuad {
                corners: [a.position, b.position, c.position, d.position],
                uv: [a.uv[0], a.uv[1], c.uv[0], c.uv[1]],
                box_center,
            });
        }
    }
    quads
}

impl ItemIcons {
    // Ein Item in das Quadrat (x, y, size) zeichnen. `alpha` blendet das ganze Icon aus
    // (z.B. das "Geisterbild" eines gerade aufgenommenen Stacks). `potion` ist die Farbe aus
    // der potion_contents des Stacks (world::potion::stack_tint) -- sie ersetzt auf den Lagen
    // mit Toenungsquelle "potion" die Vorgabe des Modells.
    pub fn push(&self, out: &mut Vec<UiSprite>, item_name: &str, potion: Option<[f32; 3]>, x: f32, y: f32, size: f32, alpha: f32) {
        if let Some((quads, eigene)) = self.entity_models.get(item_name) {
            let gui = eigene.or_else(|| match self.icons.get(item_name) {
                Some(ItemIcon::Sprite { gui, .. }) => Some(*gui),
                _ => None,
            });
            if let Some(gui) = gui {
                push_entity_model(out, quads, &gui, x, y, size, alpha);
                return;
            }
        }
        let Some(icon) = self.icons.get(item_name) else {
            return;
        };
        match icon {
            ItemIcon::Sprite { layers, gui } => {
                let corners = gui_corners(gui, x, y, size);
                for layer in layers {
                    let [r, g, b] = linear_tint(potion.filter(|_| layer.potion_tint).unwrap_or(layer.tint));
                    out.push(UiSprite { corners, uv: layer.uv, color: [r, g, b, alpha], atlas: SpriteAtlas::Ui });
                }
            }
            ItemIcon::Block { faces, overlay } => push_iso_cube(out, faces, overlay, x, y, size, alpha),
            ItemIcon::Model { quads, gui } => push_model(out, quads, gui, x, y, size, alpha),
        }
    }

    // Zeichnet das Item ueber ein Entity-Modell (Schild)?
    pub fn has_entity_model(&self, item_name: &str) -> bool {
        self.entity_models.contains_key(item_name)
    }

    // Gibt es fuer diesen Namen ein Icon? (z.B. fuer die item_model-Komponente eines Stacks)
    pub fn has(&self, item_name: &str) -> bool {
        self.icons.contains_key(item_name)
    }

    // Ein Schaedel/Kopf als Icon: Vanilla zeichnet ihn ueber das Sondermodell "head"
    // (SkullBlockEntityRenderer) -- ein 8x8x8-Kopf aus der Entity-Textur, bei Spieler- und
    // Zombiekopf mit Hutlage. Vorher stand hier der Notbehelf aus template_skull (flach die
    // Seelensand-Partikeltextur). `tex` ist das Atlas-Rechteck der ganzen Textur (beim
    // Spielerkopf der Skin aus der profile-Komponente), `hut` = 64x64-Textur mit Hutlage.
    #[allow(clippy::too_many_arguments)]
    pub fn push_skull(&self, out: &mut Vec<UiSprite>, item_name: &str, tex: UvRect, hut: bool, x: f32, y: f32, size: f32, alpha: f32) {
        use world::entity_model::Affine;
        let gui = match self.icons.get(item_name) {
            Some(ItemIcon::Sprite { gui, .. }) => *gui,
            _ => ItemDisplay { rotation: [30.0, 45.0, 0.0], translation: [0.0, 3.0 / 16.0, 0.0], scale: [1.0; 3] },
        };
        // Einbettung jedes Sondermodells (translate -0.5), dann der Kopf wie in der Hand.
        let mut vertices = Vec::new();
        crate::held_item::emit_skull_model(tex, hut, Affine::translation([-0.5, -0.5, -0.5]), 1.0, &mut vertices);
        let quads = entity_quads(&[vertices]);
        push_entity_model(out, &quads, &gui, x, y, size, alpha);
    }

    // Ein Entity-Modell in Lagen (Schild mit Banner): jede Lage ihre Boxen und ihre Farbe
    // (sRGB 0..1), eingebettet mit dem gui-Eintrag von `item_name`.
    pub fn push_layers(&self, out: &mut Vec<UiSprite>, item_name: &str, layers: &[(Vec<Vec<world::BlockVertex>>, [f32; 3])], x: f32, y: f32, size: f32, alpha: f32) {
        let Some(ItemIcon::Sprite { gui, .. }) = self.icons.get(item_name) else { return };
        let quads: Vec<(Vec<EntityQuad>, [f32; 3])> = layers.iter().map(|(boxes, tint)| (entity_quads(boxes), *tint)).collect();
        let refs: Vec<(&[EntityQuad], [f32; 3])> = quads.iter().map(|(q, t)| (q.as_slice(), *t)).collect();
        push_entity_layers(out, &refs, gui, x, y, size, alpha);
    }

        // Wie push(), aber mit freier Breite/Hoehe und einer zusaetzlichen Farbe, die MIT der
    // Item-Eigenfarbe multipliziert wird. Gebraucht fuer Anzeigen, bei denen die Farbe die
    // Information traegt (s. modules::TotemHand::tint) und das Bild nicht quadratisch ist
    // (die Totem-Animation staucht es waagerecht, um die Drehung nachzubilden).
    //
    // Nur fuer flache Sprites: ein Block-Icon ist ein zusammengesetztes Sechseck, dessen
    // Seiten schon eigene Schattierungen tragen -- da waere ein weiterer Farbfaktor sinnlos.
    // Bisher braucht das auch niemand (Totems sind flache Sprites).
    pub fn push_tinted(&self, out: &mut Vec<UiSprite>, item_name: &str, x: f32, y: f32, w: f32, h: f32, tint: [f32; 3], alpha: f32) {
        let Some(ItemIcon::Sprite { layers, .. }) = self.icons.get(item_name) else {
            return;
        };
        // BEWUSST OHNE die gui-Transformation: dieser Weg staucht das Bild selbst (die
        // Totem-Drehung), eine zweite Verzerrung darueber ergaebe keinen Sinn.
        for layer in layers {
            let t = linear_tint(layer.tint);
            let color = [t[0] * tint[0], t[1] * tint[1], t[2] * tint[2], alpha];
            out.push(UiSprite::quad(x, y, w, h, layer.uv, color, SpriteAtlas::Ui));
        }
    }
}

// Ein 3D-Item-Modell im Quadrat (x, y, size), wie Vanilla es im Inventar zeichnet:
// orthografisch, Modell um 0.5 zentriert, dann der "gui"-Eintrag des Modells (Verschiebung,
// Drehung XYZ, Skalierung) und auf die Kachel gestreckt (scale(16, -16, 16) -- Y zeigt auf
// dem Bildschirm nach unten). Sichtbar ist, was zum Betrachter (+Z) zeigt; von hinten nach
// vorn gezeichnet, damit vordere Flaechen die hinteren verdecken.
// Die vier Ecken eines FLACHEN Item-Bildes im Quadrat (x, y, size), nachdem die
// gui-Transformation des Modells darauf gewirkt hat -- dieselbe Kette wie beim 3D-Modell
// (s. push_model), nur auf das eine Viereck bei z = 0 angewandt. Ohne eigene gui-Angaben
// kommt genau das Quadrat heraus, das vorher fest gesetzt war.
fn gui_corners(gui: &ItemDisplay, x: f32, y: f32, size: f32) -> [[f32; 2]; 4] {
    use world::entity_model::Affine;
    let m = Affine::translation(gui.translation)
        .then(Affine::rotation_xyz_deg(gui.rotation[0], gui.rotation[1], gui.rotation[2]))
        .then(Affine::scale(gui.scale))
        .then(Affine::translation([-0.5, -0.5, -0.5]));
    let (cx, cy) = (x + size / 2.0, y + size / 2.0);
    // Reihenfolge wie UiSprite::quad: links oben, rechts oben, rechts unten, links unten.
    [[0.0, 1.0, 0.0], [1.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 0.0]].map(|c| {
        let v = m.apply(c);
        [cx + v[0] * size, cy - v[1] * size]
    })
}

fn push_model(out: &mut Vec<UiSprite>, quads: &[IconQuad], gui: &ItemDisplay, x: f32, y: f32, size: f32, alpha: f32) {
    use world::entity_model::Affine;
    let m = Affine::translation(gui.translation)
        .then(Affine::rotation_xyz_deg(gui.rotation[0], gui.rotation[1], gui.rotation[2]))
        .then(Affine::scale(gui.scale))
        .then(Affine::translation([-0.5, -0.5, -0.5]));
    let (cx, cy) = (x + size / 2.0, y + size / 2.0);
    let mut sichtbar: Vec<(f32, UiSprite)> = Vec::new();
    for q in quads {
        let n = m.apply_direction(q.normal);
        if n[2] <= 1e-4 {
            continue;
        }
        let p = q.corners.map(|c| m.apply(c));
        let tiefe = p.iter().map(|v| v[2]).sum::<f32>() / 4.0;
        // Feste Flaechenhelligkeit wie beim Wuerfel-Icon, nach der Richtung VOR dem Drehen.
        let shade = match q.face {
            f if f == FACE_UP => SHADE_TOP,
            f if f == FACE_DOWN => 0.5,
            f if f == FACE_NORTH || f == FACE_SOUTH => SHADE_NORTH,
            f if f == FACE_EAST || f == FACE_WEST => SHADE_EAST,
            _ => 1.0,
        };
        sichtbar.push((
            tiefe,
            UiSprite {
                corners: p.map(|v| [cx + v[0] * size, cy - v[1] * size]),
                uv: q.uv,
                color: {
                    let shade = linear_shade(shade);
                    [shade, shade, shade, alpha]
                },
                atlas: SpriteAtlas::Ui,
            },
        ));
    }
    sichtbar.sort_by(|a, b| a.0.total_cmp(&b.0));
    out.extend(sichtbar.into_iter().map(|(_, s)| s));
}

// Entity-Modell (Schild) im Quadrat (x, y, size): dieselbe gui-Kette wie push_model, danach
// sichtbare Flaechen von hinten nach vorn. Die Helligkeit folgt der Blickrichtung: das Schild
// hat gui_light "front", Vanilla leuchtet es also von vorn aus -- die zum Betrachter gewandte
// Platte voll hell, schraeg stehende Kanten dunkler.
fn push_entity_model(out: &mut Vec<UiSprite>, quads: &[EntityQuad], gui: &ItemDisplay, x: f32, y: f32, size: f32, alpha: f32) {
    push_entity_layers(out, &[(quads, [1.0; 3])], gui, x, y, size, alpha);
}

// Mehrere Lagen desselben Modells (z.B. Schild + Bannerlagen), jede mit eigener Farbe. Die
// Tiefensortierung ist stabil: deckungsgleiche Flaechen bleiben in Lagenreihenfolge, die
// spaetere liegt oben.
//
// Sortiert wird KASTENWEISE (nach der Tiefe der Kastenmitte), nicht nach der Mitte jeder
// Flaeche: die Kaesten eines Modells durchdringen sich nicht, und innerhalb eines Kastens
// ueberdecken sich die sichtbaren Flaechen nicht. Flaechenweise lag die grosse Vorderseite
// des Truhendeckels "naeher" als das Schloss davor -- ihre Mitte sitzt hoeher und ist im
// Blick von oben deshalb vorn --, und der Deckel malte ueber das Schloss.
fn push_entity_layers(out: &mut Vec<UiSprite>, layers: &[(&[EntityQuad], [f32; 3])], gui: &ItemDisplay, x: f32, y: f32, size: f32, alpha: f32) {
    use world::entity_model::Affine;
    let m = Affine::translation(gui.translation)
        .then(Affine::rotation_xyz_deg(gui.rotation[0], gui.rotation[1], gui.rotation[2]))
        .then(Affine::scale(gui.scale));
    let (cx, cy) = (x + size / 2.0, y + size / 2.0);
    let mut sichtbar: Vec<((f32, f32), UiSprite)> = Vec::new();
    for (q, tint) in layers.iter().flat_map(|(quads, tint)| quads.iter().map(move |q| (q, *tint))) {
        let p = q.corners.map(|c| m.apply(c));
        let mitte = [0, 1, 2].map(|a| p.iter().map(|v| v[a]).sum::<f32>() / 4.0);
        let kasten = m.apply(q.box_center);
        let aussen = [mitte[0] - kasten[0], mitte[1] - kasten[1], mitte[2] - kasten[2]];
        let laenge = (aussen[0] * aussen[0] + aussen[1] * aussen[1] + aussen[2] * aussen[2]).sqrt();
        if laenge < 1e-6 || aussen[2] / laenge <= 1e-4 {
            continue;
        }
        // Helligkeit wie beim Wuerfel-Icon nach der Richtung VOR dem Drehen (s. push_model):
        // oben hell, Nord/Sued mittel, Ost/West dunkel. Vorher hing sie an der Blickrichtung,
        // und beide Seiten des Kopfes kamen gleich hell heraus -- "kein richtiger Schatten".
        let roh = [0, 1, 2].map(|a| q.corners.iter().map(|v| v[a]).sum::<f32>() / 4.0 - q.box_center[a]);
        let (ax, ay, az) = (roh[0].abs(), roh[1].abs(), roh[2].abs());
        let richtung = if ay >= ax && ay >= az {
            if roh[1] > 0.0 { SHADE_TOP } else { 0.5 }
        } else if az >= ax {
            SHADE_NORTH
        } else {
            SHADE_EAST
        };
        let shade = linear_shade(richtung);
        sichtbar.push((
            (kasten[2], mitte[2]),
            UiSprite {
                corners: p.map(|v| [cx + v[0] * size, cy - v[1] * size]),
                uv: q.uv,
                color: [shade * linear_shade(tint[0]), shade * linear_shade(tint[1]), shade * linear_shade(tint[2]), alpha],
                atlas: SpriteAtlas::Entity,
            },
        ));
    }
    sichtbar.sort_by(|a, b| a.0 .0.total_cmp(&b.0 .0).then(a.0 .1.total_cmp(&b.0 .1)));
    out.extend(sichtbar.into_iter().map(|(_, s)| s));
}

// Vanillas Wuerfel-Silhouette im Quadrat (x, y, size).
//
// Nicht frei gewaehlt, sondern nachgerechnet: das Item-Modell eines Blocks wird im Inventar
// mit rotation [30, 225, 0] und scale 0.625 orthografisch gezeichnet (minecraft:block/block,
// display.gui). Projiziert man den Einheitswuerfel damit, ergibt sich ein Sechseck mit
//   halbe Breite  = cos(45 Grad) * 0.625        = 0.4419 * size
//   Schulterhoehe = cos(30 Grad) / 2 * 0.625    = 0.2706 * size
//   Spitzenhoehe  = Schulter + halbe Breite / 2 = 0.4916 * size
// Es ist also MERKLICH HOEHER ALS BREIT (Verhaeltnis 1.112 : 1). Die fruehere Fassung
// spannte stattdessen ein exaktes Quadrat auf und legte die Schultern auf size/4 -- daher
// der "zusammengequetschte" Eindruck: zu breit und die Deckflaeche zu flach.
//
//        T           T     = hintere obere Ecke (hoechster Punkt)
//    L       R       L/R   = linke/rechte Schulter
//        F           F     = vordere Ecke der Deckflaeche
//    BL      BR      BL/BR = linke/rechte untere Schulter
//        B           B     = vordere untere Ecke (tiefster Punkt)
const CUBE_HALF_WIDTH: f32 = 0.441_941_74;
const CUBE_SHOULDER: f32 = 0.270_632_94;

fn push_iso_cube(out: &mut Vec<UiSprite>, faces: &[IconLayer; 6], overlay: &[Option<IconLayer>; 6], x: f32, y: f32, size: f32, alpha: f32) {
    let (cx, cy) = (x + size / 2.0, y + size / 2.0);
    let hw = CUBE_HALF_WIDTH * size;
    let shoulder = CUBE_SHOULDER * size;
    // Halbe Hoehe der Deckflaechen-Raute: die Raute ist doppelt so breit wie hoch.
    let rhombus = hw / 2.0;

    let t = [cx, cy - shoulder - rhombus];
    let l = [cx - hw, cy - shoulder];
    let r = [cx + hw, cy - shoulder];
    let f = [cx, cy - shoulder + rhombus];
    let bl = [cx - hw, cy + shoulder];
    let br = [cx + hw, cy + shoulder];
    let b = [cx, cy + shoulder + rhombus];

    // Welche Wuerfelseite wo landet, folgt aus derselben Rechnung: bei 225 Grad Gierung
    // zeigt die OST-Flaeche nach links und die NORD-Flaeche nach rechts. Vorher war das
    // vertauscht -- bei einfarbigen Bloecken unsichtbar, bei einem Ofen oder Kuerbis nicht.
    // Die Eckreihenfolge je Flaeche ist so gewaehlt, dass die Textur genauso liegt wie an
    // einem Block in der Welt (Deckflaeche: u laeuft nach +X, v nach +Z; Seitenflaechen:
    // v laeuft nach unten).
    let quad = |layer: &IconLayer, corners: [[f32; 2]; 4], shade: f32| UiSprite {
        corners,
        uv: layer.uv,
        color: {
            let t = linear_tint(layer.tint);
            let shade = linear_shade(shade);
            [t[0] * shade, t[1] * shade, t[2] * shade, alpha]
        },
        atlas: SpriteAtlas::Block,
    };

    let top = [r, f, l, t];
    let left = [l, f, b, bl];
    let right = [f, r, br, b];
    let visible = [(FACE_UP, top, SHADE_TOP), (FACE_EAST, left, SHADE_EAST), (FACE_NORTH, right, SHADE_NORTH)];

    for (index, corners, shade) in visible {
        out.push(quad(&faces[index], corners, shade));
    }
    // Getoente Overlay-Lage auf derselben Flaeche (Grasblock-Seiten): erst die Erde, dann
    // der gruene Ueberzug darueber -- ohne das waere ein Grasblock im Inventar nur oben
    // gruen und ringsum reine Erde. Nach ALLEN Basisflaechen gezeichnet, damit die
    // Zeichenreihenfolge innerhalb einer Kachel nicht von der Flaechenreihenfolge abhaengt.
    for (index, corners, shade) in visible {
        if let Some(layer) = &overlay[index] {
            out.push(quad(layer, corners, shade));
        }
    }
}
