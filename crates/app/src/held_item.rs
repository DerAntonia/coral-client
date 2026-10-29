// Items in den Haenden anderer Spieler (und Mobs): Texturverwaltung + Einbettung.
//
// Die Texturen landen in reservierten Plaetzen des ENTITY-Atlas (s.
// assets::entity_atlas::ITEM_SLOTS) und nicht in einem eigenen Atlas. Grund ist derselbe wie
// bei den Skins: die Items haengen geometrisch an den Entities und werden im selben
// Zeichenaufruf gezeichnet -- ein zweiter Atlas hiesse ein Texturwechsel mitten in der
// Entity-Schleife, also ein zusaetzlicher Draw-Call pro Spieler.
//
// Geladen wird ERST, wenn ein Item zum ersten Mal in einer Hand auftaucht. Vorher weiss der
// Client gar nicht, welche der ~1400 Item-Texturen er je brauchen wird, und alle vorsorglich
// einzupacken waere Speicher fuer nichts.

use std::collections::{HashMap, VecDeque};

use assets::{EntityAtlas, ItemDisplay, ItemWorldTextures, MinecraftInstallation};
use world::entity_model::Affine;
use world::item_model::{SpriteMask, TexRect};
use world::mesher::BlockVertex;

// Eine geladene Item-Textur in ihrem Atlas-Platz. Die Platznummer selbst steht NICHT hier,
// sondern nur in `order` -- sie wird ausschliesslich fuer die Verdraengung gebraucht, und
// zweimal gefuehrt koennten die beiden auseinanderlaufen.
struct LoadedTexture {
    // Der TATSAECHLICH belegte Teil des Platzes: eine 16x16-Textur fuellt einen 32er-Platz
    // nur zu einem Viertel, und das Rechteck muss genau darauf zeigen -- sonst haengt
    // ringsum der leere Rest des Platzes mit im Bild.
    rect: TexRect,
    // Nur fuer flache Items: die Silhouette, aus der der Rand entsteht (s.
    // world::item_model::emit_sprite_item).
    mask: Option<SpriteMask>,
}

// Was ein Item zum Zeichnen braucht -- einmal aufgeloest, dann gemerkt.
enum Resolved {
    // `layers`: alle Lagen, wenn es mehr als eine gibt oder eine davon die Trankfarbe traegt
    // (s. composite_key) -- sonst leer, und es gilt `texture` (layer0) allein.
    Sprite { texture: String, layers: Vec<(String, bool)>, display: Displays },
    Block { textures: Box<[String; 6]>, display: Displays },
    // Eigenes 3D-Modell aus Elementen (Resourcepack, s. assets::model_quads). `tints` je
    // tintindex: feste Farbe und ob die Lage die Trankfarbe traegt (s. assets::item_tints).
    Model { quads: Vec<assets::ModelQuad>, tints: Vec<(Option<[f32; 3]>, bool)>, display: Displays },
    // Kopf/Schaedel: das Item-Modell (item/template_skull) hat keine Geometrie, Vanilla
    // zeichnet den Kopf als Sondermodell (SkullBlockEntityModel). Die Textur haengt am
    // einzelnen Stack (Skin aus der profile-Komponente) und kommt ueber HeldItems::set_skull.
    Skull { display: Displays },
    // Truhe/Shulkerkiste: ebenfalls Sondermodelle ohne Item-Geometrie (ChestModelRenderer,
    // ShulkerBoxModelRenderer). `texture` ist die Entity-Textur, `rect` ihr Platz im
    // Entity-Atlas (in ensure nachgeschlagen).
    Special { kind: SpecialKind, texture: String, rect: Option<TexRect>, display: Displays },
}

#[derive(Clone, Copy)]
pub(crate) enum SpecialKind {
    Chest,
    ShulkerBox,
    // Kupfergolem-Statue (CopperGolemStatueSpecialRenderer), gezeigt in der Grundpose.
    CopperGolemStatue,
}

// Das Basismodell der Statuen-Items: items/*copper_golem_statue.json waehlt per select auf
// die Haltung, jeder Zweig mit demselben Template -- daraus kommen die Haltungen (display).
pub(crate) const STATUE_TEMPLATE: &str = "item/template_copper_golem_statue";

// Die "transformation" aus items/*copper_golem_statue.json, nach Vanillas translate(-0.5):
// translation (0.5, 1.5, 0.5), scale (1, -1, -1).
pub(crate) fn statue_item_transform() -> Affine {
    Affine::translation([0.5, 1.5, 0.5]).then(Affine::scale([1.0, -1.0, -1.0]))
}

// Sondermodell und Entity-Textur eines Items (items/<name>.json: "type": "special"). Die
// Truhen-Items kennen wie die Bloecke Weihnachtstruhen (select auf local_time), die
// Kupfertruhen und die Endertruhe nicht.
pub(crate) fn special_item(name: &str) -> Option<(SpecialKind, String)> {
    if let Some(texture) = world::copper_golem_statue_texture(name) {
        return Some((SpecialKind::CopperGolemStatue, texture.to_string()));
    }
    let name = name.strip_prefix("waxed_").unwrap_or(name);
    let truhe = match name {
        "chest" | "trapped_chest" if crate::is_christmas() => Some("christmas"),
        "chest" => Some("normal"),
        "trapped_chest" => Some("trapped"),
        "ender_chest" => Some("ender"),
        "copper_chest" => Some("copper"),
        "exposed_copper_chest" => Some("copper_exposed"),
        "weathered_copper_chest" => Some("copper_weathered"),
        "oxidized_copper_chest" => Some("copper_oxidized"),
        _ => None,
    };
    if let Some(t) = truhe {
        return Some((SpecialKind::Chest, format!("entity/chest/{t}")));
    }
    if name == "shulker_box" {
        return Some((SpecialKind::ShulkerBox, "entity/shulker/shulker".to_string()));
    }
    let farbe = name.strip_suffix("_shulker_box")?;
    Some((SpecialKind::ShulkerBox, format!("entity/shulker/shulker_{farbe}")))
}

// Das Sondermodell in Item-Koordinaten (`base` steht schon auf der Blockecke, nach Vanillas
// translate(-0.5)): die Truhe roh wie ChestModelRenderer, die Shulkerkiste ueber die
// Block-Einbettung mit Blickrichtung "up" wie ShulkerBoxModelRenderer.
fn emit_special(kind: SpecialKind, tex: TexRect, base: Affine, brightness: f32, out: &mut Vec<BlockVertex>) {
    use world::entity_model as em;
    match kind {
        SpecialKind::Chest => world::emit_entity_model(em::chest_model(world::ChestHalf::Single), &em::chest_pose(0.0), base, tex, brightness, out),
        SpecialKind::ShulkerBox => {
            world::emit_entity_model(em::shulker_box_model(), &em::shulker_box_pose(0.0), base.then(em::shulker_box_transform([0.0; 3], "up")), tex, brightness, out)
        }
        SpecialKind::CopperGolemStatue => {
            if let Some(model) = world::model_data::layer_model("copper_golem#main") {
                world::emit_entity_model(model, &em::copper_golem_statue_pose(model), base.then(statue_item_transform()), tex, brightness, out);
            }
        }
    }
}

// Die Kopf-Items mit dem 8x8x8-Kopfmodell. Drachen- und Piglinkopf haben eigene Modelle und
// fehlen deshalb (noch).
pub const SKULL_ITEMS: [&str; 5] = ["player_head", "zombie_head", "skeleton_skull", "wither_skeleton_skull", "creeper_head"];

// Den Kopf als Sondermodell in den Raum des Item-Modells (0..1): die Einbettung aus
// SkullBlockEntityRenderer.renderSkull -- translate(0.5, 0, 0.5), scale(-1, -1, 1),
// Kopfdrehung 180 Grad -- und ein 8x8x8-Kopf, bei 64x64-Texturen mit Hutlage.
pub fn emit_skull_model(tex: TexRect, hut: bool, base: Affine, brightness: f32, out: &mut Vec<BlockVertex>) {
    emit_skull_at(tex, hut, base.then(Affine::translation([0.5, 0.0, 0.5])), 180.0, brightness, out);
}

// Ein PLATZIERTER Kopf (SkullBlockEntityRenderer.render): `base` steht schon auf dem Ankerpunkt
// (Blockmitte bzw. vor der Wand), danach scale(-1, -1, 1) und die Kopfdrehung um Y.
pub fn emit_skull_at(tex: TexRect, hut: bool, base: Affine, yaw_deg: f32, brightness: f32, out: &mut Vec<BlockVertex>) {
    use world::entity_model::{Cuboid, Model, Part};
    let mut cuboids = vec![Cuboid::new([0.0, 0.0], [-4.0, -8.0, -4.0], [8.0, 8.0, 8.0])];
    if hut {
        cuboids.push(Cuboid::new([32.0, 0.0], [-4.0, -8.0, -4.0], [8.0, 8.0, 8.0]).dilated(0.25));
    }
    let model = Model { texture: "", tex_size: [64.0, if hut { 64.0 } else { 32.0 }], parts: vec![Part::new("head", [0.0; 3], cuboids)] };
    let base = base.then(Affine::scale([-1.0, -1.0, 1.0])).then(Affine::rotation_y_deg(yaw_deg));
    world::emit_entity_model(&model, &world::Pose::new(), base, tex, brightness, out);
}

// Die beiden Haltungen eines Items. Vanilla fuehrt sie getrennt
// (ItemDisplayContext.THIRD_PERSON_RIGHT_HAND vs. FIRST_PERSON_RIGHT_HAND) -- ein Schwert
// liegt in der eigenen Hand anders als in der eines fremden Spielers, und ein Pack darf
// beides unabhaengig setzen.
#[derive(Clone, Copy)]
struct Displays {
    third_person: ItemDisplay,
    first_person: ItemDisplay,
    // Die Linkshand-Eintraege, noch ungespiegelt (s. emit_resolved_hand).
    third_person_left: ItemDisplay,
    first_person_left: ItemDisplay,
    fixed: ItemDisplay,
    ground: ItemDisplay,
    // Fuer item_display mit Anzeigemodus HEAD bzw. GUI (s. emit_display).
    head: ItemDisplay,
    gui: ItemDisplay,
}

pub struct HeldItems {
    // Texturpfad -> Platz. Der Pfad ist der Schluessel, nicht das Item: mehrere Items teilen
    // sich Texturen (jede Seite eines Obsidianwuerfels ist dieselbe), und so wird ein Platz
    // nur einmal belegt.
    textures: HashMap<String, LoadedTexture>,
    // Belegungsreihenfolge fuer die Verdraengung, wenn alle Plaetze voll sind.
    order: VecDeque<(String, usize)>,
    // Item-Id -> Darstellung. None heisst "schon versucht, gibt es nicht" -- damit ein
    // unbekanntes Item (Server mit Custom-Items) nicht in JEDEM Frame erneut die
    // Modell-JSONs durchsucht.
    items: HashMap<u32, Option<Resolved>>,
    // Dasselbe fuer ausdruecklich benannte MODELLE (item/bow_pulling_1, item/crossbow_arrow,
    // ...). Vanilla waehlt sie je nach Zustand des Stacks statt der Vorgabe des Items -- ein
    // gespannter Bogen ist ein anderes Modell, nicht dasselbe Modell anders gedreht.
    models: HashMap<String, Option<Resolved>>,
    // Textur des Kopfes, der als NAECHSTES gezeichnet wird (Atlas-Rechteck, mit Hutlage?).
    // Die emit-Wege kennen nur die Item-Id, der Skin haengt aber am Stack -- der Aufrufer
    // setzt ihn deshalb unmittelbar vor dem Zeichnen (s. set_skull).
    skull: std::cell::Cell<Option<(TexRect, bool)>>,
    // Geladene Texturen mit .mcmeta-Animation (Magmablock, Prismarin, Kompass-artige Packs,
    // ...): im Atlas liegt erst nur der erste Frame, tick_animations tauscht die Pixel aus.
    animations: Vec<ItemAnimation>,
    // Items in der Welt ueber Vanillas Core-Shader eines Resourcepacks zeichnen (s.
    // render::vanilla_draw): solange `vanilla_route` an ist, gehen flache und Modell-Items
    // nicht in den normalen Entity-Puffer, sondern als Vanilla-Vierecke nach `vanilla_out`.
    // Der Aufrufer schaltet es nur fuer die Welt-Entities ein -- Hand und Inventar zeichnen
    // weiter selbst. `vanilla_bypass` erzwingt kurz den normalen Weg (Masse eines Modells).
    vanilla_route: std::cell::Cell<bool>,
    vanilla_bypass: std::cell::Cell<bool>,
    vanilla_out: std::cell::RefCell<Vec<render::vanilla_draw::VanillaVertex>>,
}

struct ItemAnimation {
    path: String,
    anim: assets::TileAnimation,
    last: Option<assets::AnimationState>,
}

impl HeldItems {
    pub fn new() -> Self {
        Self {
            textures: HashMap::new(),
            order: VecDeque::new(),
            items: HashMap::new(),
            models: HashMap::new(),
            skull: std::cell::Cell::new(None),
            animations: Vec::new(),
            vanilla_route: std::cell::Cell::new(false),
            vanilla_bypass: std::cell::Cell::new(false),
            vanilla_out: std::cell::RefCell::new(Vec::new()),
        }
    }

    // Umleiten an/aus (s. vanilla_route). Beim Einschalten wird die Sammlung geleert.
    pub fn set_vanilla_route(&self, an: bool) {
        if an {
            self.vanilla_out.borrow_mut().clear();
        }
        self.vanilla_route.set(an);
    }

    pub fn take_vanilla_vertices(&self) -> Vec<render::vanilla_draw::VanillaVertex> {
        std::mem::take(&mut self.vanilla_out.borrow_mut())
    }

    // Wuerde dieses Item gerade umgeleitet? (Dann landet in `out` nichts.)
    pub fn routes_to_vanilla(&self, item_id: u32) -> bool {
        self.vanilla_route.get() && !self.vanilla_bypass.get() && matches!(self.items.get(&item_id), Some(Some(Resolved::Sprite { .. } | Resolved::Model { .. })))
    }

    // `f` auf dem normalen Weg, auch wenn gerade umgeleitet wird.
    pub fn without_vanilla<R>(&self, f: impl FnOnce() -> R) -> R {
        let vorher = self.vanilla_bypass.replace(true);
        let r = f();
        self.vanilla_bypass.set(vorher);
        r
    }

    pub fn set_skull(&self, tex: Option<(TexRect, bool)>) {
        self.skull.set(tex);
    }

    // Gehoert zur Verbindung: ein Resourcepack-Wechsel oder ein neuer Server kann andere
    // Texturen bedeuten, und die Atlas-Plaetze werden ohnehin neu beschrieben.
    pub fn clear(&mut self) {
        self.textures.clear();
        self.order.clear();
        self.items.clear();
        self.models.clear();
        self.animations.clear();
    }

    // Animierte Item-Texturen auf den Stand von Spieltick `tick` bringen (dieselbe Uhr wie die
    // Block-Animationen). Hochgeladen wird nur, was sich seit dem letzten Mal geaendert hat.
    pub fn tick_animations(&mut self, tick: u64, upload: &mut dyn FnMut((u32, u32), u32, u32, &[u8])) {
        for a in &mut self.animations {
            let state = a.anim.state_at(tick);
            if a.last == Some(state) {
                continue;
            }
            a.last = Some(state);
            if let Some(pixels) = a.anim.render(state).first() {
                upload(a.anim.origin, a.anim.tile_size, a.anim.tile_size, pixels);
            }
        }
    }

    // Sicherstellen, dass Modell und Texturen dieses Items geladen sind. Muss VOR dem Bauen
    // der Vertices laufen (es braucht &mut self und die GPU-Warteschlange).
    // `potion` ist die Trankfarbe des Stacks (world::potion::stack_color).
    pub fn ensure(
        &mut self,
        item_id: u32,
        potion: Option<u32>,
        mc: &mut MinecraftInstallation,
        atlas: &EntityAtlas,
        upload: &mut dyn FnMut(usize, (u32, u32), u32, u32, &[u8]),
    ) {
        self.items.entry(item_id).or_insert_with(|| {
            let resolved = Self::resolve(item_id, mc);
            // Diagnose (CORAL_ITEM_DEBUG=1): EINMAL pro Item, wenn es zum ersten Mal in
            // einer Hand auftaucht. Zeigt, als was das Item aufgeloest wurde und mit welcher
            // Haltung es gezeichnet wird -- die Groesse in der Hand haengt allein an `scale`,
            // und ob Wuerfel oder flaches Sprite entscheidet sich hier.
            if std::env::var_os("CORAL_ITEM_DEBUG").is_some() {
                let name = world::item_registry::item_name(item_id).unwrap_or("?");
                match &resolved {
                    Some(Resolved::Block { textures, display }) => eprintln!(
                        "[item] {name} (id {item_id}): BLOCK scale={:?} rot={:?} trans={:?} tex={}",
                        display.third_person.scale, display.third_person.rotation, display.third_person.translation, textures[0]
                    ),
                    Some(Resolved::Sprite { texture, display, .. }) => eprintln!(
                        "[item] {name} (id {item_id}): SPRITE scale={:?} rot={:?} trans={:?} tex={texture}",
                        display.third_person.scale, display.third_person.rotation, display.third_person.translation
                    ),
                    Some(Resolved::Model { quads, display, .. }) => eprintln!(
                        "[item] {name} (id {item_id}): MODELL {} Flächen scale={:?} rot={:?} trans={:?}",
                        quads.len(),
                        display.third_person.scale,
                        display.third_person.rotation,
                        display.third_person.translation
                    ),
                    Some(Resolved::Skull { .. }) => eprintln!("[item] {name} (id {item_id}): KOPF"),
                    Some(Resolved::Special { texture, .. }) => eprintln!("[item] {name} (id {item_id}): SONDERMODELL {texture}"),
                    None => eprintln!("[item] {name} (id {item_id}): NICHT AUFGELÖST"),
                }
            }
            resolved
        });
        if let Some(Some(Resolved::Special { texture, rect, .. })) = self.items.get_mut(&item_id) {
            if rect.is_none() {
                *rect = atlas.rect(texture);
            }
            return;
        }
        if let Some(Some(Resolved::Sprite { layers, .. })) = self.items.get(&item_id) {
            if let Some(key) = composite_key(layers, potion) {
                let layers = layers.clone();
                self.load_composite(&key, &layers, potion, mc, atlas, upload);
                return;
            }
        }
        // Die Texturliste getrennt einsammeln: `self.items` bleibt dabei unangetastet,
        // waehrend load_texture unten &mut self braucht.
        let paths: Vec<String> = match self.items.get(&item_id) {
            Some(Some(Resolved::Sprite { texture, .. })) => vec![texture.clone()],
            Some(Some(Resolved::Block { textures, .. })) => {
                let mut list = textures.to_vec();
                list.sort_unstable();
                list.dedup();
                list
            }
            Some(Some(Resolved::Model { quads, .. })) => {
                let mut list: Vec<String> = quads.iter().map(|q| q.texture.clone()).collect();
                list.sort_unstable();
                list.dedup();
                list
            }
            // Die Kopftexturen liegen fest im Atlas bzw. in den Skin-Plaetzen.
            _ => return,
        };
        for path in paths {
            self.load_texture(&path, mc, atlas, upload);
        }
    }

    // Wie ensure(), nur fuer ein benanntes Modell (s. HeldItems::models).
    pub fn ensure_model(
        &mut self,
        model: &str,
        mc: &mut MinecraftInstallation,
        atlas: &EntityAtlas,
        upload: &mut dyn FnMut(usize, (u32, u32), u32, u32, &[u8]),
    ) {
        if !self.models.contains_key(model) {
            let resolved = Self::resolve_model(model, mc);
            self.models.insert(model.to_string(), resolved);
        }
        let paths = match self.models.get(model) {
            Some(Some(resolved)) => Self::texture_paths(resolved),
            _ => return,
        };
        for path in paths {
            self.load_texture(&path, mc, atlas, upload);
        }
    }

    fn texture_paths(resolved: &Resolved) -> Vec<String> {
        let mut list = match resolved {
            Resolved::Sprite { texture, .. } => vec![texture.clone()],
            Resolved::Block { textures, .. } => textures.to_vec(),
            Resolved::Model { quads, .. } => quads.iter().map(|q| q.texture.clone()).collect(),
            Resolved::Skull { .. } | Resolved::Special { .. } => Vec::new(),
        };
        list.sort_unstable();
        list.dedup();
        list
    }

    // Die Haltungen eines MODELLS (statt eines Items).
    fn model_displays(model: &str, mc: &mut MinecraftInstallation) -> Displays {
        Displays {
            third_person: assets::model_hand_display(mc, model, "thirdperson_righthand"),
            first_person: assets::model_hand_display(mc, model, "firstperson_righthand"),
            third_person_left: assets::model_hand_display(mc, model, "thirdperson_lefthand"),
            first_person_left: assets::model_hand_display(mc, model, "firstperson_lefthand"),
            fixed: assets::model_hand_display(mc, model, "fixed"),
            ground: assets::model_hand_display(mc, model, "ground"),
            head: assets::model_hand_display(mc, model, "head"),
            gui: assets::model_hand_display(mc, model, "gui"),
        }
    }

    fn resolve_model(model: &str, mc: &mut MinecraftInstallation) -> Option<Resolved> {
        let display = Self::model_displays(model, mc);
        match assets::model_world_textures(mc, model)? {
            ItemWorldTextures::Sprite(texture) => Some(Resolved::Sprite { texture, layers: Vec::new(), display }),
            ItemWorldTextures::Block(textures) => Some(Resolved::Block { textures, display }),
            ItemWorldTextures::Model(quads) => Some(Resolved::Model { quads, tints: Vec::new(), display }),
        }
    }

    fn resolve(item_id: u32, mc: &mut MinecraftInstallation) -> Option<Resolved> {
        // Item-Definitionen aus Resourcepacks (item_model "ns:pfad") haben eine eigene
        // Nummer (s. world::custom_model_id) und laufen ueber denselben Weg wie Vanilla-Items,
        // nur mit ihrer ID statt des Itemnamens.
        let eigener = world::entity::custom_model_name(item_id);
        let name: &str = match &eigener {
            Some(id) => id.as_str(),
            None => world::item_registry::item_name(item_id)?,
        };
        let display =
            Displays {
                third_person: assets::item_hand_display(mc, name),
                first_person: assets::item_first_person_display(mc, name),
                third_person_left: assets::item_hand_display_left(mc, name),
                first_person_left: assets::item_first_person_display_left(mc, name),
                fixed: assets::item_fixed_display(mc, name),
                ground: assets::item_ground_display(mc, name),
                head: assets::item_named_display(mc, name, "head"),
                gui: assets::item_named_display(mc, name, "gui"),
            };
        if eigener.is_none() && SKULL_ITEMS.contains(&name) {
            return Some(Resolved::Skull { display });
        }
        if let Some((kind, texture)) = special_item(name) {
            // Die Statue haengt hinter einem select auf die Haltung; ihre Haltungen stehen im
            // gemeinsamen Template.
            let display = if matches!(kind, SpecialKind::CopperGolemStatue) { Self::model_displays(STATUE_TEMPLATE, mc) } else { display };
            return Some(Resolved::Special { kind, texture, rect: None, display });
        }
        match assets::item_world_textures(mc, name)? {
            ItemWorldTextures::Sprite(texture) => {
                let mut layers = assets::item_sprite_layers(mc, name);
                if layers.len() < 2 && !layers.iter().any(|l| l.1) {
                    layers.clear();
                }
                Some(Resolved::Sprite { texture, layers, display })
            }
            ItemWorldTextures::Block(textures) => Some(Resolved::Block { textures, display }),
            ItemWorldTextures::Model(quads) => Some(Resolved::Model { quads, tints: assets::item_tints(mc, name), display }),
        }
    }

    // Alle Lagen uebereinander in EINE Textur, die Trank-Lagen vorher mit der Farbe
    // multipliziert. Vanilla zeichnet die Lagen als eigene Flaechen am selben Ort; fuer
    // Ausschnitt-Texturen (jedes Pixel ganz deckend oder ganz leer) ist das Zusammensetzen
    // dasselbe Bild. Unter `key` abgelegt, also einmal je Farbe.
    fn load_composite(
        &mut self,
        key: &str,
        layers: &[(String, bool)],
        potion: Option<u32>,
        mc: &mut MinecraftInstallation,
        atlas: &EntityAtlas,
        upload: &mut dyn FnMut(usize, (u32, u32), u32, u32, &[u8]),
    ) {
        if self.textures.contains_key(key) {
            return;
        }
        let farbe = potion.unwrap_or(world::potion::BASE_POTION_COLOR);
        let faktor = [(farbe >> 16) & 0xff, (farbe >> 8) & 0xff, farbe & 0xff];
        let mut bild: Option<(u32, u32, Vec<u8>)> = None;
        for (path, getoent) in layers {
            let png = assets::resource_path("textures", path, ".png");
            let Ok(tex) = mc.load_texture(&png) else { continue };
            let groesse = assets::frame_size(assets::animation_meta(mc, &png).as_ref(), tex.width, tex.height);
            let Some((w, h, mut rgba)) = fit_to_slot(tex.width, groesse, &tex.rgba) else { continue };
            if *getoent {
                for px in rgba.chunks_exact_mut(4) {
                    for (c, f) in px.iter_mut().zip(faktor) {
                        *c = ((*c as u32 * f) / 255) as u8;
                    }
                }
            }
            match &mut bild {
                None => bild = Some((w, h, rgba)),
                // Eine Lage in anderer Groesse (Resourcepack) laesst sich nicht Pixel fuer
                // Pixel darueberlegen -- weglassen statt verzerren.
                Some((bw, bh, _)) if (*bw, *bh) != (w, h) => {}
                Some((_, _, unten)) => {
                    for (d, o) in unten.chunks_exact_mut(4).zip(rgba.chunks_exact(4)) {
                        let a = o[3] as u32;
                        for i in 0..3 {
                            d[i] = ((o[i] as u32 * a + d[i] as u32 * (255 - a)) / 255) as u8;
                        }
                        d[3] = (a + d[3] as u32 * (255 - a) / 255) as u8;
                    }
                }
            }
        }
        let Some((width, height, rgba)) = bild else { return };
        self.install_texture(key, width, height, rgba, atlas, upload);
    }

    fn load_texture(
        &mut self,
        path: &str,
        mc: &mut MinecraftInstallation,
        atlas: &EntityAtlas,
        upload: &mut dyn FnMut(usize, (u32, u32), u32, u32, &[u8]),
    ) {
        if self.textures.contains_key(path) {
            return;
        }
        let png = assets::resource_path("textures", path, ".png");
        let Ok(tex) = mc.load_texture(&png) else { return };
        let meta = assets::animation_meta(mc, &png);
        let bild = assets::frame_size(meta.as_ref(), tex.width, tex.height);
        let Some((width, height, rgba)) = fit_to_slot(tex.width, bild, &tex.rgba) else { return };
        let Some(origin) = self.install_texture(path, width, height, rgba, atlas, upload) else { return };
        // Animation: im Platz in der eingepassten Groesse weiterspielen (der Entity-Atlas hat
        // keine Mip-Stufen).
        if meta.is_some() {
            if let Some(anim) = assets::TileAnimation::load(mc, path, origin, (width, height), 1) {
                self.animations.push(ItemAnimation { path: path.to_string(), anim, last: None });
            }
        }
    }

    fn install_texture(
        &mut self,
        path: &str,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
        atlas: &EntityAtlas,
        upload: &mut dyn FnMut(usize, (u32, u32), u32, u32, &[u8]),
    ) -> Option<(u32, u32)> {
        let slot = self.claim_slot(atlas.item_slot_count(), path)?;
        let origin = atlas.item_slot_origin(slot)?;
        upload(slot, origin, width, height, &rgba);

        // Rechteck auf den TATSAECHLICH belegten Teil des Platzes (s. LoadedTexture::rect).
        let size = atlas.size as f32;
        let rect = [origin.0 as f32 / size, origin.1 as f32 / size, (origin.0 + width) as f32 / size, (origin.1 + height) as f32 / size];
        let mask = SpriteMask::from_rgba(width, height, &rgba);
        self.textures.insert(path.to_string(), LoadedTexture { rect, mask: Some(mask) });
        Some(origin)
    }

    // Freien Platz nehmen, sonst den am laengsten belegten verdraengen. Der Verdraengte wird
    // aus der Tabelle GELOESCHT, nicht nur ueberschrieben: sein Item findet die Textur dann
    // beim naechsten Auftauchen nicht mehr und laedt sie neu, statt weiter auf einen Platz zu
    // zeigen, in dem inzwischen etwas anderes liegt.
    fn claim_slot(&mut self, capacity: usize, path: &str) -> Option<usize> {
        if capacity == 0 {
            return None;
        }
        if self.order.len() < capacity {
            let slot = self.order.len();
            self.order.push_back((path.to_string(), slot));
            return Some(slot);
        }
        let (evicted, slot) = self.order.pop_front()?;
        self.textures.remove(&evicted);
        // Sonst spielte die Animation des Verdraengten im Platz des Nachfolgers weiter.
        self.animations.retain(|a| a.path != evicted);
        self.order.push_back((path.to_string(), slot));
        Some(slot)
    }

    // Das Item in die Hand zeichnen. `arm` ist die Transformation des Armteils (s.
    // world::entity_model::part_transform), `right_hand` unterscheidet Haupt- von Nebenhand.
    //
    // Die Kette danach ist HeldItemFeatureRenderer.renderItem aus dem 1.21.11-Client, Schritt
    // fuer Schritt:
    //     1. um X um -90 Grad drehen
    //     2. um Y um 180 Grad drehen
    //     3. verschieben um (-1/16 links bzw. +1/16 rechts, 0.125, -0.625)
    // und anschliessend die Halte-Transformation des Item-Modells (Transformation.apply):
    //     translate(display.translation) -> rotateXYZ(display.rotation) -> scale -> translate(-0.5)
    // `pre` ist hand_pre_transform(arm) plus etwaige Zusatzbewegung (der Speerstich, s.
    // world::entity_model::third_person_stab_item_offset). `local` = lokale UVs fuer den
    // Glint, wie bei den `*_local`-Varianten unten.
    #[allow(clippy::too_many_arguments)]
    pub fn emit_pre(&self, item_id: u32, potion: Option<u32>, pre: Affine, right_hand: bool, brightness: f32, local: bool, out: &mut Vec<BlockVertex>) {
        self.emit_with(item_id, potion, pre, right_hand, false, brightness, local, out);
    }

    // Die `*_local`-Varianten geben DIESELBE Geometrie aus, aber mit lokalen UVs (0..1 je
    // Textur statt Atlas-Koordinaten) -- die Vorlage fuer den Verzauberungs-Glint (s.
    // render::glint::glint_vertices). Vertex fuer Vertex deckungsgleich mit der normalen
    // Ausgabe desselben Aufrufs.
    // Dasselbe Item in der EIGENEN Hand (Ego-Ansicht). `pre` ist hier nicht der Arm, sondern
    // Vanillas Ego-Kette im Sichtraum (s. world::first_person_item_transform) -- das Item
    // haengt dort NICHT am Armmodell, sondern wird unabhaengig gesetzt.
    pub fn emit_first_person(
        &self,
        item_id: u32,
        potion: Option<u32>,
        pre: Affine,
        right_hand: bool,
        brightness: f32,
        out: &mut Vec<BlockVertex>,
    ) {
        self.emit_with(item_id, potion, pre, right_hand, true, brightness, false, out);
    }

    pub fn emit_first_person_local(&self, item_id: u32, potion: Option<u32>, pre: Affine, right_hand: bool, out: &mut Vec<BlockVertex>) {
        self.emit_with(item_id, potion, pre, right_hand, true, 1.0, true, out);
    }

    // Dieselben Wege, nur mit einem ausdruecklich gewaehlten Modell (s. ensure_model).
    // Ein benanntes Modell ohne Haltung: `base` setzt es direkt in die Welt (Blockecke bei 0).
    // Fuer Modelle, die keine Items sind -- den Gegenstandsrahmen.
    pub fn emit_model_raw(&self, model: &str, base: Affine, brightness: f32, out: &mut Vec<BlockVertex>) {
        let Some(Some(resolved)) = self.models.get(model) else { return };
        self.emit_resolved(resolved, None, base, brightness, false, out);
    }

    // Ein benanntes Modell in der Hand eines Entities (Vorkette wie emit_pre).
    #[allow(clippy::too_many_arguments)]
    pub fn emit_model_pre(&self, model: &str, pre: Affine, right_hand: bool, brightness: f32, local: bool, out: &mut Vec<BlockVertex>) {
        self.emit_model_with(model, pre, right_hand, false, brightness, local, out);
    }

    pub fn emit_first_person_model(&self, model: &str, pre: Affine, right_hand: bool, brightness: f32, out: &mut Vec<BlockVertex>) {
        self.emit_model_with(model, pre, right_hand, true, brightness, false, out);
    }

    pub fn emit_first_person_model_local(&self, model: &str, pre: Affine, right_hand: bool, out: &mut Vec<BlockVertex>) {
        self.emit_model_with(model, pre, right_hand, true, 1.0, true, out);
    }

    // Kennt die Verwaltung dieses Modell schon (und hat es aufgeloest)? Ohne das wuerde ein
    // gerade erst angefordertes Modell einen Frame lang gar nichts zeichnen, statt so lange
    // beim Grundmodell des Items zu bleiben.
    pub fn has_model(&self, model: &str) -> bool {
        matches!(self.models.get(model), Some(Some(_)))
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_model_with(
        &self,
        model: &str,
        pre: Affine,
        right_hand: bool,
        first_person: bool,
        brightness: f32,
        local: bool,
        out: &mut Vec<BlockVertex>,
    ) {
        let Some(Some(resolved)) = self.models.get(model) else { return };
        self.emit_resolved_hand(resolved, None, pre, right_hand, first_person, brightness, local, out);
    }

    // Das Item frei im Raum mit seiner FIXED-Haltung (ItemRenderState mit
    // ItemDisplayContext.FIXED) -- fuer Vanillas schwebendes Pop-Totem.
    pub fn emit_fixed(&self, item_id: u32, pre: Affine, brightness: f32, out: &mut Vec<BlockVertex>) {
        let Some(Some(resolved)) = self.items.get(&item_id) else { return };
        let display = match resolved {
            Resolved::Sprite { display, .. } | Resolved::Block { display, .. } | Resolved::Model { display, .. } | Resolved::Skull { display } | Resolved::Special { display, .. } => display.fixed,
        };
        let base = pre
            .then(Affine::translation(display.translation))
            .then(Affine::rotation_xyz_deg(display.rotation[0], display.rotation[1], display.rotation[2]))
            .then(Affine::scale(display.scale))
            .then(Affine::translation([-0.5, -0.5, -0.5]));
        self.emit_resolved(resolved, None, base, brightness, false, out);
    }

    // Das Item mit seiner GROUND-Haltung (ItemDisplayContext.GROUND) in den Item-eigenen Raum:
    // liegende Items und Wurfgeschosse. `pre` setzt es in die Welt; die Aufrufer brauchen oft
    // erst die Modellbox (s. App::emit_item_entity) und geben deshalb IDENTITY und
    // verschieben die Vertices danach selbst. `local`: UVs fuer den Glint (s. emit_local).
    pub fn emit_ground(&self, item_id: u32, potion: Option<u32>, pre: Affine, brightness: f32, local: bool, out: &mut Vec<BlockVertex>) {
        let Some(Some(resolved)) = self.items.get(&item_id) else { return };
        let display = match resolved {
            Resolved::Sprite { display, .. } | Resolved::Block { display, .. } | Resolved::Model { display, .. } | Resolved::Skull { display } | Resolved::Special { display, .. } => display.ground,
        };
        let base = pre
            .then(Affine::translation(display.translation))
            .then(Affine::rotation_xyz_deg(display.rotation[0], display.rotation[1], display.rotation[2]))
            .then(Affine::scale(display.scale))
            .then(Affine::translation([-0.5, -0.5, -0.5]));
        self.emit_resolved(resolved, potion, base, brightness, local, out);
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_with(
        &self,
        item_id: u32,
        potion: Option<u32>,
        pre: Affine,
        right_hand: bool,
        first_person: bool,
        brightness: f32,
        local: bool,
        out: &mut Vec<BlockVertex>,
    ) {
        let Some(Some(resolved)) = self.items.get(&item_id) else { return };
        self.emit_resolved_hand(resolved, potion, pre, right_hand, first_person, brightness, local, out);
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_resolved_hand(
        &self,
        resolved: &Resolved,
        potion: Option<u32>,
        pre: Affine,
        right_hand: bool,
        first_person: bool,
        brightness: f32,
        local: bool,
        out: &mut Vec<BlockVertex>,
    ) {
        let displays = match resolved {
            Resolved::Sprite { display, .. } | Resolved::Block { display, .. } | Resolved::Model { display, .. } | Resolved::Skull { display } | Resolved::Special { display, .. } => display,
        };
        // Die linke Hand nimmt ihren eigenen Eintrag und spiegelt ihn erst beim Anwenden
        // (Transformation.apply mit leftHanded).
        let display = match (first_person, right_hand) {
            (true, true) => displays.first_person,
            (false, true) => displays.third_person,
            (true, false) => displays.first_person_left.mirrored(),
            (false, false) => displays.third_person_left.mirrored(),
        };
        let base = pre
            .then(Affine::translation(display.translation))
            .then(Affine::rotation_xyz_deg(display.rotation[0], display.rotation[1], display.rotation[2]))
            .then(Affine::scale(display.scale))
            // Zentriert das Modell, dessen Koordinaten von 0 bis 1 laufen.
            .then(Affine::translation([-0.5, -0.5, -0.5]));
        self.emit_resolved(resolved, potion, base, brightness, local, out);
    }

    // Dasselbe Modell als FREISTEHENDE Geometrie in der Welt -- fuer die Display-Entities
    // (item_display / block_display), die weder an einem Arm noch an einer Kamera haengen.
    //
    // Zwei Unterschiede zur Hand-Fassung, beide beabsichtigt:
    //   - KEINE Halte-Transformation aus dem Modell-JSON. Vanillas Vorgabe fuer ein
    //     item_display ist ItemDisplayContext.NONE, und deren Transformation ist die
    //     Einheitsmatrix -- ein Schwert im Display haengt also gerade im Raum, nicht schraeg
    //     in einer unsichtbaren Faust.
    //   - `centered` unterscheidet die beiden Typen: ein item_display sitzt MITTIG auf seiner
    //     Entity-Position, ein block_display fuellt von dort aus den Wuerfel 0..1 (genau wie
    //     ein echter Block in seiner Zelle).
    // Ein item_display: ItemRenderState mit dem ItemDisplayContext des Displays (`mode`, s.
    // packets::ITEM_DISPLAY_INDEX_MODE) -- erst die display-Transformation dieses Kontexts
    // (NONE: keine), dann translate(-0.5). Die Hand-Kontexte links spiegeln wie Vanilla.
    pub fn emit_display(&self, item_id: u32, potion: Option<u32>, base: Affine, mode: u8, brightness: f32, out: &mut Vec<BlockVertex>) {
        let Some(Some(resolved)) = self.items.get(&item_id) else { return };
        let d = match resolved {
            Resolved::Sprite { display, .. } | Resolved::Block { display, .. } | Resolved::Model { display, .. } | Resolved::Skull { display } | Resolved::Special { display, .. } => display,
        };
        let t = match mode {
            1 => Some(d.third_person_left.mirrored()),
            2 => Some(d.third_person),
            3 => Some(d.first_person_left.mirrored()),
            4 => Some(d.first_person),
            5 => Some(d.head),
            6 => Some(d.gui),
            7 => Some(d.ground),
            8 => Some(d.fixed),
            _ => None,
        };
        let base = match t {
            Some(t) => base
                .then(Affine::translation(t.translation))
                .then(Affine::rotation_xyz_deg(t.rotation[0], t.rotation[1], t.rotation[2]))
                .then(Affine::scale(t.scale)),
            None => base,
        };
        self.emit_resolved(resolved, potion, base.then(Affine::translation([-0.5, -0.5, -0.5])), brightness, false, out);
    }

    // `local`: UVs 0..1 je Textur statt im Atlas (s. emit_local).
    fn emit_resolved(
        &self,
        resolved: &Resolved,
        potion: Option<u32>,
        base: Affine,
        brightness: f32,
        local: bool,
        out: &mut Vec<BlockVertex>,
    ) {
        const GANZ: [f32; 4] = [0.0, 0.0, 1.0, 1.0];
        if !local && self.vanilla_route.get() && !self.vanilla_bypass.get() && self.emit_vanilla(resolved, potion, base, brightness) {
            return;
        }
        match resolved {
            Resolved::Sprite { texture, layers, .. } => {
                let key = composite_key(layers, potion);
                let Some(loaded) = self.textures.get(key.as_deref().unwrap_or(texture)) else { return };
                let Some(mask) = &loaded.mask else { return };
                if !mask.any_opaque() {
                    return;
                }
                world::item_model::emit_sprite_item(mask, if local { GANZ } else { loaded.rect }, base, brightness, out);
            }
            Resolved::Block { textures, .. } => {
                // Fehlt auch nur eine Flaeche, wird gar nicht gezeichnet: ein Wuerfel mit
                // fuenf Seiten sieht kaputter aus als ein fehlendes Item.
                let mut rects = [[0.0f32; 4]; 6];
                for (slot, path) in rects.iter_mut().zip(textures.iter()) {
                    let Some(loaded) = self.textures.get(path) else { return };
                    *slot = if local { GANZ } else { loaded.rect };
                }
                world::item_model::emit_block_item(&rects, base, brightness, out);
            }
            Resolved::Model { quads, tints, .. } => {
                // Wie beim Wuerfel: fehlt eine Textur, lieber gar nichts als ein halbes Modell.
                let mut fertig = Vec::with_capacity(quads.len());
                for q in quads {
                    let Some(loaded) = self.textures.get(&q.texture) else { return };
                    let r = if local { GANZ } else { loaded.rect };
                    let an = |t: f32, a: f32, b: f32| a + (b - a) * t / 16.0;
                    let (u0, v0, u1, v1) = (an(q.uv[0], r[0], r[2]), an(q.uv[1], r[1], r[3]), an(q.uv[2], r[0], r[2]), an(q.uv[3], r[1], r[3]));
                    fertig.push((q.corners, q.normal, [[u0, v0], [u1, v0], [u1, v1], [u0, v1]], tint_color(q.tint_index, tints, potion)));
                }
                world::item_model::emit_model_quads_tinted(&fertig, base, brightness, out);
            }
            Resolved::Skull { .. } => {
                // Ohne Glint-Fassung: die lokalen UVs eines Kopfes gaebe es nur je Flaeche.
                if let (Some((tex, hut)), false) = (self.skull.take(), local) {
                    emit_skull_model(tex, hut, base, brightness, out);
                }
            }
            Resolved::Special { kind, rect: Some(tex), .. } => {
                if !local {
                    emit_special(*kind, *tex, base, brightness, out);
                }
            }
            Resolved::Special { rect: None, .. } => {}
        }
    }
}

impl HeldItems {
    // Flache und Modell-Items als Vanilla-Vierecke (s. vanilla_route). false = nicht diese Art,
    // der normale Weg zeichnet sie.
    //
    // Eckreihenfolge: die Geometrie hier laeuft von aussen gesehen im Uhrzeigersinn (s.
    // world::item_model::push_quad), Vanilla gegen ihn -- daher q0, q3, q2, q1. Die Diagonale
    // bleibt q0-q2, und darauf rechnen Pack-Shader, die ihre Ecken ueber gl_VertexID finden.
    fn emit_vanilla(&self, resolved: &Resolved, potion: Option<u32>, base: Affine, brightness: f32) -> bool {
        use render::vanilla_draw::{VanillaVertex, OVERLAY_NONE};
        let mut out = self.vanilla_out.borrow_mut();
        let mut viereck = |ecken: [[f32; 3]; 4], uvs: [[f32; 2]; 4], normale: [f32; 3], farbe: [f32; 3]| {
            let n = glam::Vec3::from(normale).normalize_or_zero().to_array();
            for i in [0usize, 3, 2, 1] {
                out.push(VanillaVertex {
                    position: ecken[i],
                    color: [farbe[0], farbe[1], farbe[2], 1.0],
                    uv0: uvs[i],
                    uv1: OVERLAY_NONE,
                    // Licht traegt der Aufrufer nach (s. App::vanilla_item_lights).
                    uv2: [-1, -1],
                    normal: n,
                    ..Default::default()
                });
            }
        };
        match resolved {
            Resolved::Sprite { texture, layers, .. } => {
                let key = composite_key(layers, potion);
                let Some(loaded) = self.textures.get(key.as_deref().unwrap_or(texture)) else { return true };
                let Some(mask) = &loaded.mask else { return true };
                if !mask.any_opaque() {
                    return true;
                }
                // Dieselbe Geometrie wie der normale Weg (Vorder-, Rueckseite, Kantenstreifen),
                // dort als Dreieckspaare 0,2,1 / 0,3,2 -- zurueck zu Vierecken.
                let mut tris = Vec::new();
                world::item_model::emit_sprite_item(mask, loaded.rect, base, brightness, &mut tris);
                for t in tris.chunks_exact(6) {
                    let q = [t[0], t[2], t[1], t[4]];
                    let p = q.map(|v| glam::Vec3::from(v.position));
                    let n = (p[2] - p[0]).cross(p[1] - p[0]);
                    viereck(q.map(|v| v.position), q.map(|v| v.uv), n.to_array(), [1.0; 3]);
                }
                true
            }
            Resolved::Model { quads, tints, .. } => {
                for q in quads {
                    let Some(loaded) = self.textures.get(&q.texture) else { continue };
                    let r = loaded.rect;
                    let an = |t: f32, a: f32, b: f32| a + (b - a) * t / 16.0;
                    let (u0, v0, u1, v1) = (an(q.uv[0], r[0], r[2]), an(q.uv[1], r[1], r[3]), an(q.uv[2], r[0], r[2]), an(q.uv[3], r[1], r[3]));
                    let ecken = q.corners.map(|c| base.apply(c));
                    viereck(ecken, [[u0, v0], [u1, v0], [u1, v1], [u0, v1]], base.apply_direction(q.normal), tint_color(q.tint_index, tints, potion));
                }
                true
            }
            _ => false,
        }
    }
}

// Die Farbe einer Flaeche mit tintindex (ItemModel: tintSource je Index). Die Trankfarbe
// (potion) gilt nur, wenn der Stapel eine hat, sonst der Vorgabewert der Quelle; ohne Eintrag
// in der Liste bleibt die Flaeche weiss (Vanilla: -1).
fn tint_color(index: Option<i64>, tints: &[(Option<[f32; 3]>, bool)], potion: Option<u32>) -> [f32; 3] {
    let Some(i) = index.and_then(|i| usize::try_from(i).ok()) else { return [1.0; 3] };
    let Some((fest, trank)) = tints.get(i) else { return [1.0; 3] };
    if *trank {
        if let Some(c) = potion {
            return [((c >> 16) & 0xff) as f32 / 255.0, ((c >> 8) & 0xff) as f32 / 255.0, (c & 0xff) as f32 / 255.0];
        }
    }
    fest.unwrap_or([1.0; 3])
}

// Die Kette vom Armteil bis kurz vor die Halte-Transformation des Modells -- Vanillas
// HeldItemFeatureRenderer.renderItem (s. HeldItems::emit). Eigene Funktion, weil das SCHILD
// sie ebenso braucht, aber danach einen anderen Weg geht (Entity-Modell statt Item-Modell,
// s. App::emit_shield): liefen die beiden Ketten auseinander, saesse das Schild neben der
// Hand statt darin.
pub fn hand_pre_transform(arm: Affine, right_hand: bool) -> Affine {
    let hand_offset = if right_hand { 1.0 / 16.0 } else { -1.0 / 16.0 };
    arm.then(world::entity_model::rotation_x_deg(-90.0)).then(Affine::rotation_y_deg(180.0)).then(Affine::translation([
        hand_offset,
        0.125,
        -0.625,
    ]))
}

// Eine Textur auf einen Item-Platz bringen: von einer animierten nur das erste Bild (`bild`,
// s. assets::frame_size), genau wie bei den Block- und Inventar-Texturen. Was danach noch
// groesser als der Platz ist, wird je Achse ganzzahlig heruntergerechnet (Nearest, damit
// Pixelkunst Pixelkunst bleibt; die UVs sind ohnehin relativ, ein schmales hohes Bild wie
// cpvpwars' 12x1974-Zonenwaende behaelt so seine Breite).
fn fit_to_slot(tex_width: u32, (width, height): (u32, u32), rgba: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    if width == 0 || height == 0 || rgba.len() < (tex_width * height * 4) as usize {
        return None;
    }
    let frame = assets_frame(rgba, tex_width, (width, height));
    let max = assets::ITEM_SLOT_SIZE;
    if width <= max && height <= max {
        return Some((width, height, frame));
    }
    // Ganzzahlige Verkleinerungsfaktoren, aufgerundet -- danach passt beides sicher hinein.
    let (fx, fy) = (width.div_ceil(max).max(1), height.div_ceil(max).max(1));
    let (out_w, out_h) = ((width / fx).max(1), (height / fy).max(1));
    let mut out = vec![0u8; (out_w * out_h * 4) as usize];
    for y in 0..out_h {
        for x in 0..out_w {
            let src = (((y * fy) * width + x * fx) * 4) as usize;
            let dst = ((y * out_w + x) * 4) as usize;
            out[dst..dst + 4].copy_from_slice(&frame[src..src + 4]);
        }
    }
    Some((out_w, out_h, out))
}

fn assets_frame(rgba: &[u8], tex_width: u32, bild: (u32, u32)) -> Vec<u8> {
    assets::frame_pixels(rgba, tex_width, bild, 0)
}

// Schluessel der zusammengesetzten Textur (s. HeldItems::load_composite), oder None fuer ein
// einlagiges Item. Die Farbe gehoert nur dann dazu, wenn eine Lage sie auch traegt -- sonst
// bekaeme jede Trankfarbe einen eigenen, identischen Atlas-Platz.
fn composite_key(layers: &[(String, bool)], potion: Option<u32>) -> Option<String> {
    if layers.is_empty() {
        return None;
    }
    let pfade: Vec<&str> = layers.iter().map(|l| l.0.as_str()).collect();
    let mut key = pfade.join("+");
    if layers.iter().any(|l| l.1) {
        key.push_str(&format!("#{:06x}", potion.unwrap_or(world::potion::BASE_POTION_COLOR)));
    }
    Some(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Truhen- und Shulker-Items bekommen ihr Sondermodell samt Entity-Textur; jede Textur
    // steht in den Listen, die der Entity-Atlas laedt.
    #[test]
    fn sondermodelle_der_truhen_und_kisten() {
        let textur = |n: &str| special_item(n).map(|(_, t)| t);
        if !crate::is_christmas() {
            assert_eq!(textur("chest").as_deref(), Some("entity/chest/normal"));
            assert_eq!(textur("trapped_chest").as_deref(), Some("entity/chest/trapped"));
        }
        assert_eq!(textur("ender_chest").as_deref(), Some("entity/chest/ender"));
        assert_eq!(textur("waxed_weathered_copper_chest").as_deref(), Some("entity/chest/copper_weathered"));
        assert_eq!(textur("shulker_box").as_deref(), Some("entity/shulker/shulker"));
        assert_eq!(textur("light_blue_shulker_box").as_deref(), Some("entity/shulker/shulker_light_blue"));
        assert!(special_item("oak_planks").is_none());
        for name in ["chest", "trapped_chest", "ender_chest", "copper_chest", "oxidized_copper_chest", "shulker_box", "red_shulker_box"] {
            let t = textur(name).unwrap();
            assert!(world::CHEST_TEXTURES.contains(&t.as_str()) || world::SHULKER_TEXTURES.contains(&t.as_str()), "{t}");
        }
    }

    // Ein Animationsstreifen (16 breit, 64 hoch = vier Bilder) darf nur sein ERSTES Bild
    // liefern -- sonst quetscht sich die ganze Animation in die Silhouette des Items.
    #[test]
    fn animationsstreifen_liefert_nur_das_erste_bild() {
        let rgba = vec![255u8; (16 * 64 * 4) as usize];
        let (w, h, out) = fit_to_slot(16, (16, 16), &rgba).unwrap();
        assert_eq!((w, h), (16, 16));
        assert_eq!(out.len(), (16 * 16 * 4) as usize);
    }

    // Ein nicht-quadratisches, nicht teilbares Bild ist KEIN Streifen und bleibt ganz.
    #[test]
    fn unteilbares_bild_bleibt_unveraendert() {
        let rgba = vec![255u8; (16 * 20 * 4) as usize];
        let (w, h, _) = fit_to_slot(16, (16, 20), &rgba).unwrap();
        assert_eq!((w, h), (16, 20));
    }

    // Ein sehr hochaufloesendes Resourcepack (256x256) passt nicht in einen 128er-Platz und
    // wird ganzzahlig verkleinert, statt zu verschwinden.
    #[test]
    fn zu_grosse_textur_wird_verkleinert() {
        let rgba = vec![128u8; (256 * 256 * 4) as usize];
        let (w, h, out) = fit_to_slot(256, (256, 256), &rgba).unwrap();
        assert!(w <= assets::ITEM_SLOT_SIZE && h <= assets::ITEM_SLOT_SIZE, "passt nicht in den Platz: {w}x{h}");
        assert_eq!(out.len(), (w * h * 4) as usize);
    }

    // 64er-Packs (CPVP Aesthetic) muessen UNVERAENDERT hineinpassen: halbiert fielen die
    // ungeraden Zeilen weg und mit ihnen der rechte und untere 1-Pixel-Rand.
    #[test]
    fn vierfache_aufloesung_bleibt_unveraendert() {
        let rgba: Vec<u8> = (0..64 * 64 * 4).map(|i| (i % 251) as u8).collect();
        let (w, h, out) = fit_to_slot(64, (64, 64), &rgba).unwrap();
        assert_eq!((w, h), (64, 64));
        assert_eq!(out, rgba);
    }

    // Die Verdraengung muss den alten Eintrag ENTFERNEN, nicht bloss den Platz neu vergeben.
    // Bliebe er stehen, zeigte sein Item auf einen Platz, in dem inzwischen etwas anderes
    // liegt -- ein Totem mit der Textur eines Schwertes.
    #[test]
    fn verdraengter_platz_wird_vergessen() {
        let mut items = HeldItems::new();
        items.textures.insert("a".into(), LoadedTexture { rect: [0.0; 4], mask: None });
        items.order.push_back(("a".into(), 0));
        let slot = items.claim_slot(1, "b").expect("Platz");
        assert_eq!(slot, 0, "der einzige Platz wird wiederverwendet");
        assert!(!items.textures.contains_key("a"), "der verdrängte Eintrag muss weg sein");
    }
}
