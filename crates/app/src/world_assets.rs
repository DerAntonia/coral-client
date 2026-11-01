// Welt-Assets und Laden der Ressourcenpakete.

use crate::*;

// Schiebt eine Farbe von ihrer Grauwert-(Luma-)Projektion weg, um `amount` verstaerkt --
// > 1.0 saettigt, 1.0 = unveraendert. Reiner Darstellungs-Feinschliff (auf Nutzerwunsch:
// die echten Colormap-/Registry-Farben wirkten zu blass), keine Vanilla-Farbe.
pub(crate) fn saturate(rgb: [f32; 3], amount: f32) -> [f32; 3] {
    let luma = 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2];
    std::array::from_fn(|i| (luma + (rgb[i] - luma) * amount).clamp(0.0, 1.0))
}

// Dreht ein 6er-Feld Flaechen-UVs um x_deg (zuerst) dann y_deg (danach), je in 90-Grad-
// Schritten -- dieselbe Permutation, die assets::shapes intern fuer die Geometrie nutzt
// (permute_faces_x/y), hier auf UvRect statt auf ShapeBox/Face angewendet. Siehe
// assets::build_rotations-Kommentar: die Atlas-UVs pro Blockname sind kanonisch
// (ungedreht), das hier ist der fehlende Schritt, der sie pro State richtig ausrichtet.
pub(crate) fn rotate_face_uvs(uvs: [assets::UvRect; 6], x_deg: i32, y_deg: i32) -> [assets::UvRect; 6] {
    let mut uvs = uvs;
    for _ in 0..(x_deg / 90).rem_euclid(4) {
        uvs = assets::permute_faces_x(uvs);
    }
    for _ in 0..(y_deg / 90).rem_euclid(4) {
        uvs = assets::permute_faces_y(uvs);
    }
    uvs
}

// Gegenstueck zu block/cube_mirrored (s. assets::build_random_rotations-Kommentar): dessen
// einziger Unterschied zu block/cube ist "uv": [16,0,0,16] statt [0,0,16,16] auf JEDER der
// sechs Flaechen -- eine reine horizontale U-Spiegelung derselben Textur, kein zweites
// Bild. u0/u1 vertauschen bildet das exakt nach (v bleibt unangetastet). Wird VOR
// rotate_face_uvs angewendet, die Reihenfolge ist aber egal: Spiegeln aendert nur den
// INHALT jedes der sechs UV-Rechtecke gleich, rotate_face_uvs vertauscht nur, WELCHES
// Rechteck an welcher Flaeche landet -- beide Operationen kommutieren.
pub(crate) fn mirror_face_uvs(uvs: [assets::UvRect; 6]) -> [assets::UvRect; 6] {
    uvs.map(|[u0, v0, u1, v1]| [u1, v0, u0, v1])
}

// Alles, was aus einer MinecraftInstallation (Jar + optionaler Resourcepack-Ordner, s.
// assets::MinecraftInstallation::resource_pack_dir) gebaut wird und sich bei einem
// Resourcepack-Wechsel (Skin-Tab "Laden", s. App::reload_resource_pack) mit-aendern kann:
// der Textur-Atlas selbst UND alles, was von seinen UVs abhaengt (block_uvs), UND die
// Formen/Kreuz-Bloecke (technisch ebenfalls aus Modell-/Blockstate-JSONs, die ein Pack
// theoretisch mit-ueberschreiben kann, auch wenn das in der Praxis selten vorkommt).
// Ausgelagert aus App::new, damit App::new (beim Start) und reload_resource_pack (beim
// Skin-Wechsel) exakt dieselbe Bau-Logik nutzen, statt zwei Kopien synchron halten zu
// muessen. biome_palette bleibt bewusst AUSSEN vor: die ist rein statisch (world::
// BiomePalette::fallback()), haengt nicht von der Installation ab.
pub(crate) struct WorldAssets {
    pub(crate) atlas: assets::BlockAtlas,
    // 2D-Oberflaeche: GUI-Sheets (Hotbar/Inventar) und flache Item-Sprites in EINEM Atlas,
    // dazu pro Itemname sein Icon (Sprite-UVs bzw. Blockflaechen, s. assets::ui_atlas).
    // Wird zusammen mit dem Blockatlas gebaut, weil Blockitems dessen UVs mitbenutzen --
    // und beim Resourcepack-Wechsel genauso neu.
    pub(crate) ui_atlas: assets::UiAtlas,
    // Dazu die Item-Modell-Definitionen, deren Zweige je Stapel gewaehlt werden.
    pub(crate) item_icons: (std::collections::HashMap<String, assets::ItemIcon>, std::collections::HashMap<String, assets::ItemDefinition>),
    // Entity-Texturatlas + die einmal gebauten Modelle (s. build()).
    pub(crate) entity_atlas: assets::EntityAtlas,
    pub(crate) particle_atlas: assets::ParticleAtlas,
    pub(crate) entity_models: Vec<(&'static str, world::Model, world::entity_model::ModelKind)>,
    pub(crate) player_wide: world::Model,
    pub(crate) player_slim: world::Model,
    // PlayerCapeModel -- reine Daten, einmal gebaut (s. world::cape_model).
    pub(crate) cape_model: world::Model,
    // Zauberschrift fuer die Zierzeilen des Zaubertischs (s. App::push_alt_text).
    pub(crate) alt_font: assets::AltFontGlyphs,
    // Bett-BlockEntity-Modelle (s. world::entity_model::bed_head_model).
    pub(crate) bed_head_model: world::Model,
    pub(crate) bed_foot_model: world::Model,
    pub(crate) armor_models: [world::Model; 4],
    pub(crate) shield: ShieldRender,
    pub(crate) grass_colormap: Option<assets::Colormap>,
    pub(crate) foliage_colormap: Option<assets::Colormap>,
    pub(crate) dry_foliage_colormap: Option<assets::Colormap>,
    pub(crate) block_uvs: std::sync::Arc<world::BlockUvs>,
    pub(crate) block_shapes: std::sync::Arc<world::BlockShapes>,
    pub(crate) cross_blocks: std::sync::Arc<world::CrossBlocks>,
    pub(crate) cross_extra_shapes: std::sync::Arc<world::BlockShapes>,
}

impl WorldAssets {
    // Baut alles, meldet unterwegs den Fortschritt in Prozent (0..=90 -- den Rest belegen beim
    // Pack-Wechsel Klaenge und Glint, s. App::reload_resource_pack; beim Start die GPU, s.
    // loading_screen).
    pub(crate) fn build_with_progress(installation: &mut assets::MinecraftInstallation, fortschritt: &mut dyn FnMut(u32)) -> Result<Self, assets::AssetsError> {
        fortschritt(0);
        let atlas = assets::BlockAtlas::build(installation, &world::block_registry::all_block_names())?;
        fortschritt(25);
        // Gras-/Laub-Colormaps: reine Bilddaten, kein Bezug zu einem Server. Kein hartes
        // Scheitern wie beim Atlas: ohne Colormap bleibt die Welt einfach ungetoent
        // (weiss/Textur pur) statt den Bau zu verweigern.
        let grass_colormap = assets::Colormap::load(installation, "grass").ok();
        let foliage_colormap = assets::Colormap::load(installation, "foliage").ok();
        let dry_foliage_colormap = assets::Colormap::load(installation, "dry_foliage").ok();

        // Formen (Kollisions-/Render-Boxen) aus den echten Vanilla-Blockstate-/Modell-JSONs:
        // pro State Blockname + Properties sammeln (Backing-Storage fuer die &str-Slices,
        // die assets::build_shapes erwartet), dann aufloesen und in world::BlockShapes
        // uebersetzen (Vollwuerfel-Fallback fuer alles Unaufgeloeste, s. dortigen Kommentar).
        let state_count = world::block_registry::state_count();
        let props_storage: Vec<Vec<(&str, &str)>> =
            (0..state_count as u32).map(|s| world::state_props::properties_of(s).collect()).collect();
        let state_entries: Vec<(u32, &str, &[(&str, &str)])> = (0..state_count as u32)
            .filter_map(|s| {
                let name = world::block_name(s)?;
                Some((s, name, props_storage[s as usize].as_slice()))
            })
            .collect();
        // state-ID -> 6 Flaechen-UVs: fuer jede State-ID den Blocknamen nachschlagen und
        // dessen (KANONISCHE, ungedrehte -- s. assets::blocks::resolve_block_faces-
        // Kommentar) Atlas-UVs uebernehmen, dann per State tatsaechlich drehen. Ohne die
        // zweite Zeile wuerde jeder State desselben Blocknamens (z.B. jeder Holzstamm,
        // unabhaengig von "axis") dieselbe, von first_model() zufaellig gewaehlte
        // Ausrichtung zeigen.
        let rotations = assets::build_rotations(installation, &state_entries);
        fortschritt(35);
        // Hat ein State ein ANDERES Modell als das erste seines Blocks (geladener Anker,
        // brennender Ofen, verschneites Gras), gelten dessen Texturen (s.
        // assets::BlockAtlas::variant_uvs).
        let variant_models = assets::state_variant_models(installation, &state_entries);
        let variante = |state: u32| variant_models.get(&state).and_then(|m| atlas.variant_uvs.get(m));
        let per_state: Vec<[assets::UvRect; 6]> = (0..state_count as u32)
            .map(|state| {
                let base = variante(state)
                    .map(|(faces, _)| *faces)
                    .or_else(|| world::block_name(state).and_then(|name| atlas.uvs.get(name).copied()))
                    .unwrap_or(atlas.missing);
                match rotations.get(&state) {
                    Some(&(x_deg, y_deg)) => rotate_face_uvs(base, x_deg, y_deg),
                    None => base,
                }
            })
            .collect();
        // state-ID -> sechs Inhaltsflaechen-UVs (z.B. Kesselinhalt), falls der Block welche
        // hat -- s. assets::shapes::ElementBox::content. Keine Rotation noetig: keiner der
        // betroffenen Bloecke (Kessel) hat eine achsenabhaengige Ausrichtung.
        let content_per_state: Vec<[Option<assets::UvRect>; 6]> = (0..state_count as u32)
            .map(|state| world::block_name(state).and_then(|name| atlas.content_uvs.get(name).copied()).unwrap_or([None; 6]))
            .collect();
        // state-ID -> sechs Overlay-Flaechen-UVs (z.B. grass_block-Seiten, s. assets::
        // BlockAtlas::overlay_uvs-Kommentar). Keine Rotation noetig: grass_block hat keine
        // achsenabhaengige Ausrichtung.
        let overlay_per_state: Vec<[Option<assets::UvRect>; 6]> = (0..state_count as u32)
            .map(|state| match variante(state) {
                Some((_, overlay)) => *overlay,
                None => world::block_name(state).and_then(|name| atlas.overlay_uvs.get(name).copied()).unwrap_or([None; 6]),
            })
            .collect();
        // state-ID -> alle Rotationsvarianten fuer Vanillas pseudozufaellige Block-Rotation
        // (Erde/Sand/Stein/Grasblock/Podzol/Bedrock/..., s. world::render_rng-Modulkommentar).
        // random_rotations (State+Property-bewusst, aus dem world-Crate) entscheidet NUR,
        // WELCHE States ueberhaupt rotieren (z.B. grass_block mit snowy=true NICHT) und wie
        // viele Varianten es gibt -- die eigentlichen UV-Werte kommen aus
        // atlas.rng_face_uvs (Namen-basiert, s. dortigen Feldkommentar zu den vorab
        // pixelweise gedrehten Oben/Unten-Kacheln). Seitenflaechen (Nord/Sued/West/Ost)
        // reichen dagegen mit reiner UV-Umsortierung + Spiegel-Flag (rotate_face_uvs/
        // mirror_face_uvs), deshalb hier kombiniert: Seiten aus der Rect-Operation, Oben/
        // Unten aus der Atlas-Pixelkachel ueberschrieben (x ist bei allen bekannten
        // RNG-Bloecken immer 0, permute_faces_x/y lassen Oben/Unten dabei ohnehin
        // unangetastet, s. permute_faces_y-Kommentar).
        let random_rotations = assets::build_random_rotations(installation, &state_entries);
        let rng_variants_per_state: Vec<Vec<[assets::UvRect; 6]>> = (0..state_count as u32)
            .map(|state| match random_rotations.get(&state) {
                Some(variants) => {
                    let Some(name) = world::block_name(state) else {
                        return Vec::new();
                    };
                    let base = atlas.uvs.get(name).copied().unwrap_or(atlas.missing);
                    let baked = atlas.rng_face_uvs.get(name);
                    variants
                        .iter()
                        .enumerate()
                        .map(|(i, &(x_deg, y_deg, mirrored))| {
                            let sides = if mirrored { mirror_face_uvs(base) } else { base };
                            let mut sides = rotate_face_uvs(sides, x_deg, y_deg);
                            if let Some(top_bottom) = baked.and_then(|b| b.get(i)) {
                                sides[assets::FACE_UP] = top_bottom[assets::FACE_UP];
                                sides[assets::FACE_DOWN] = top_bottom[assets::FACE_DOWN];
                            }
                            sides
                        })
                        .collect()
                }
                None => Vec::new(),
            })
            .collect();
        let water_uv = atlas.uvs.get("water").copied().unwrap_or(atlas.missing);
        let block_uvs = std::sync::Arc::new(
            world::BlockUvs::with_content_and_overlay(
                per_state,
                content_per_state,
                overlay_per_state,
                rng_variants_per_state,
                water_uv,
                atlas.missing,
            )
            .with_water_flow(atlas.water_flow_uv)
            .with_water_overlay(atlas.water_overlay_uv)
            .with_lava_flow(atlas.lava_flow_uv)
            .with_destroy_stages(atlas.destroy_stage_uvs)
            // Animierte Kacheln je Block zeitversetzt, s. assets::ANIM_VARIANTS.
            .with_anim_variants(atlas.tiles_per_row, &atlas.anim_variant_tiles),
        );

        let shapes_raw = assets::build_shapes(installation, &state_entries);
        // Kreuz-Bloecke (Zuckerrohr, Gras, Blumen, ...): eigener Zeichenpfad im Mesher (zwei
        // diagonale Ebenen statt Vollwuerfel-Fallback, s. world::cross_shapes). Ihre
        // shapes_raw-Box-Liste ist leer (bzw. fehlt, s. assets::shapes-Kommentar zu
        // Element::rotation) -- das MUSS als "keine Box" statt als "Vollwuerfel-Fallback"
        // ankommen, sonst wuerden sie faelschlich Nachbar-Flaechen anderer Bloecke verdecken
        // (s. Nachbar-Deckungstest in world::mesher).
        let cross_raw = assets::build_cross(installation, &state_entries);
        let shapes_per_state: Vec<Option<Vec<world::ShapeBox>>> = (0..state_count as u32)
            .map(|s| {
                if cross_raw.contains(&s) {
                    return Some(Vec::new());
                }
                // Schilder haben im Blockmodell keine Geometrie (nur die Partikeltextur) -- sie
                // zeichnet allein der Schild-Renderer (s. emit_signs). Ohne diese Zeile fiel der
                // Mesher auf den Vollwuerfel mit Brettertextur zurueck, der im Schild steckte. Umriss und Kollision
                // kommen unabhaengig davon aus Vanillas Formtabelle.
                if world::block_name(s).and_then(world::entity_model::sign_kind).is_some() {
                    return Some(Vec::new());
                }
                let boxes = shapes_raw.get(&s).map(|boxes| {
                    boxes.iter().map(|b| world::ShapeBox { min: b.min, max: b.max, faces: b.faces, content: b.content, uv: b.uv }).collect()
                });
                // Dasselbe fuer Koepfe, Truhen, Betten und Shulkerkisten: ihr Blockmodell traegt
                // nur die Partikeltextur, gezeichnet werden sie als Block-Entity. Der Vollwuerfel
                // stand sonst mit eben dieser Textur um das Modell herum -- ein Spielerkopf als
                // Seelensandblock. Bringt ein Pack echte Elemente mit, bleiben die.
                if boxes.is_none() && world::block_entity::is_model_block(s) {
                    return Some(Vec::new());
                }
                boxes
            })
            .collect();
        let mut block_shapes = world::BlockShapes::new(shapes_per_state);
        // Modelle mit gedrehten Elementen (Feuer, Laterne, Kerzen, ...) als echte Vierecke --
        // vorher fielen sie auf einen Vollwuerfel mit Partikeltextur zurueck, s. assets::build_model_quads.
        let model_quads_raw = assets::build_model_quads(installation, &state_entries);
        // Je State alle Fassungen: eine pro austauschbarer Kachel (Feuer, s. assets::FIRE_VARIANTS),
        // sonst genau eine.
        let model_quads_per_state: Vec<Option<Vec<Vec<world::ModelFace>>>> = (0..state_count as u32)
            .map(|s| {
                model_quads_raw.get(&s).map(|quads| {
                    let pool = quads.iter().find_map(|q| atlas.texture_variants.get(&q.texture)).map_or(1, std::vec::Vec::len);
                    (0..pool)
                        .map(|variant| {
                            quads
                                .iter()
                                .map(|q| {
                                    let rect = match atlas.texture_variants.get(&q.texture) {
                                        Some(p) => p[variant % p.len()],
                                        None => atlas.texture_uvs.get(&q.texture).copied().unwrap_or(atlas.missing[0]),
                                    };
                                    model_face(q, rect)
                                })
                                .collect()
                        })
                        .collect()
                })
            })
            .collect();
        block_shapes.set_model_quads(model_quads_per_state);
        // Weiche Beleuchtung: welche Modelle sie abschalten ("ambientocclusion": false), dazu
        // je State Vollwuerfel/Sicht/Leuchten -- einmal vorberechnet, weil der Mesher das fuer
        // jede sichtbare Flaeche bis zu zwoelfmal nachschlaegt (s. world::ambient_occlusion).
        // Aus dem Resourcepack gelesen, also auch dessen Modell-Einstellungen.
        let ao_disabled = assets::build_ao_disabled(installation, &state_entries);
        block_shapes.build_ao_flags(&ao_disabled);
        let block_shapes = std::sync::Arc::new(block_shapes);
        let cross_per_state: Vec<bool> = (0..state_count as u32).map(|s| cross_raw.contains(&s)).collect();
        let cross_blocks = std::sync::Arc::new(world::CrossBlocks::new(cross_per_state));

        // Zusaetzliche, unrotierte Elemente eines Kreuz-Modells (s. cross_extra_shapes-
        // Feldkommentar) -- eine von shapes_raw/block_shapes komplett getrennte Tabelle,
        // damit sie nie in die Nachbar-Verdeckungspruefung des Meshers gelangen kann.
        let cross_extra_raw = assets::build_cross_extras(installation, &state_entries);
        let cross_extra_per_state: Vec<Option<Vec<world::ShapeBox>>> = (0..state_count as u32)
            .map(|s| {
                cross_extra_raw.get(&s).map(|boxes| {
                    boxes.iter().map(|b| world::ShapeBox { min: b.min, max: b.max, faces: b.faces, content: b.content, uv: b.uv }).collect()
                })
            })
            .collect();
        let cross_extra_shapes = std::sync::Arc::new(world::BlockShapes::new(cross_extra_per_state));

        // Dazu die Ladezustaende der Armbrust als eigene Eintraege (s. item_icon_name): ohne
        // items/-Definition faellt der Name auf das gleichnamige Modell item/crossbow_arrow.
        let mut icon_namen = world::item_registry::all_item_names().to_vec();
        icon_namen.extend(["crossbow_arrow", "crossbow_firework"].map(String::from));
        fortschritt(60);
        let (ui_atlas, icons, definitions) = assets::build_ui_atlas(installation, &icon_namen, &atlas);
        let item_icons = (icons, definitions);
        fortschritt(70);

        // Entity-Modelle und ihr Texturatlas. Die Modelle selbst sind reine Daten (s.
        // world::entity_model) und werden EINMAL gebaut, nicht pro Frame -- jedes Modell
        // enthaelt Vecs, ein Neubau je Entity und Frame waere reine Allokations-Arbeit.
        let entity_models = world::entity_model::model_table();
        let player_wide = world::player_model(false);
        let player_slim = world::player_model(true);
        let cape = world::cape_model();
        let alt_font = installation.load_alt_font_glyphs().unwrap_or_default();
        let bed_head = world::entity_model::bed_head_model();
        let bed_foot = world::entity_model::bed_foot_model();
        // Ein Modell je Ruestungsteil (nicht je Material) -- die Textur kommt erst beim
        // Zeichnen dazu, s. world::entity_model::armor_model.
        let armor_models = std::array::from_fn(|i| {
            world::entity_model::armor_model(world::armor::ArmorSlot::ALL.iter().copied().find(|s| s.index() == i).expect("Slot"))
        });
        // Die Halte-Transformationen kommen aus shield.json -- also aus dem Resourcepack,
        // nicht fest verdrahtet: ein Pack darf das Schild anders in die Hand legen.
        let shield = ShieldRender {
            model: world::entity_model::shield_model(),
            plate: shield_teil("plate"),
            handle: shield_teil("handle"),
            third_person: [assets::item_hand_display(installation, "shield"), assets::item_hand_display_left(installation, "shield")],
            first_person: [
                assets::item_first_person_display(installation, "shield"),
                assets::item_first_person_display_left(installation, "shield"),
            ],
            third_person_blocking: [
                assets::model_hand_display(installation, "item/shield_blocking", "thirdperson_righthand"),
                assets::model_hand_display(installation, "item/shield_blocking", "thirdperson_lefthand"),
            ],
            first_person_blocking: [
                assets::model_hand_display(installation, "item/shield_blocking", "firstperson_righthand"),
                assets::model_hand_display(installation, "item/shield_blocking", "firstperson_lefthand"),
            ],
        };
        let mut entity_textures: Vec<&str> = entity_models.iter().map(|(_, model, _)| model.texture).collect();
        entity_textures.push(player_wide.texture);
        entity_textures.push(player_slim.texture);
        // Betten sind BlockEntities: ihr Blockmodell ist leer, gezeichnet werden sie mit einer
        // ENTITY-Textur (s. world::block_entity). Alle 16 Farben rein -- welche vorkommt, steht
        // erst zur Laufzeit fest, und 16 kleine 64x64-Kacheln fallen im Atlas nicht ins Gewicht.
        entity_textures.extend(world::BED_TEXTURES.iter().copied());
        entity_textures.extend(world::SHULKER_TEXTURES.iter().copied());
        entity_textures.extend(world::CHEST_TEXTURES.iter().copied());
        // Kupfergolem-Statuen als Inventar-Icons (s. set_special_icons).
        entity_textures.extend(world::COPPER_GOLEM_STATUE_TEXTURES);
        entity_textures.extend(world::entity_model::SIGN_TEXTURES);
        entity_textures.extend(world::entity_model::HANGING_SIGN_TEXTURES);
        // Ruestung: 18 kleine 64x32-Bilder, alle fest im Atlas statt ueber die
        // Nachladeplaetze der Handgegenstaende (s. world::armor::all_textures).
        entity_textures.extend(world::armor::all_textures());
        entity_textures.push(shield.model.texture);
        // Schild mit Banner: Grundtextur ohne das Holzmuster plus alle Musterlagen (Masken, die
        // beim Zeichnen eingefaerbt werden, s. emit_shield).
        entity_textures.push(SHIELD_BANNER_BASE);
        let schild_muster: Vec<String> = SHIELD_PATTERNS.iter().map(|p| format!("entity/shield/{p}")).collect();
        entity_textures.extend(schild_muster.iter().map(String::as_str));
        entity_textures.push(world::entity_model::END_CRYSTAL_TEXTURE);
        // Hintergrund einer Karte in der Hand (ItemInHandRenderer.renderMap).
        entity_textures.push("map/map_background");
        entity_textures.push("map/map_background_checkerboard");
        // Alle Katzenfarben -- welche vorkommt, schickt erst der Server.
        entity_textures.extend(world::entity_model::CAT_TEXTURES);
        entity_textures.push(world::entity_model::WIND_CHARGE_TEXTURE);
        entity_textures.push(world::entity_model::ARROW_TEXTURE);
        entity_textures.push(world::entity_model::SPECTRAL_ARROW_TEXTURE);
        // Das Feuer auf brennenden Entities (s. emit_entity_fire) -- animiert, der Atlas legt
        // die Streifen als Raster ab.
        entity_textures.extend(ENTITY_FIRE_TEXTURES);
        entity_textures.push(world::ELYTRA_TEXTURE);
        entity_textures.push(world::entity_model::ENCHANTING_BOOK_TEXTURE);
        // Alle Mobs, Boote und Loren aus der Datentabelle (s. world::mob_catalog).
        let katalog_texturen = world::mob_catalog::all_textures();
        entity_textures.extend(katalog_texturen.iter().map(String::as_str));
        let entity_atlas = assets::build_entity_atlas(installation, &entity_textures);
        fortschritt(85);
        let particle_atlas = assets::build_particle_atlas(installation, &protocol::particle::PARTICLE_NAMES, world::item_registry::all_item_names());
        fortschritt(90);

        Ok(Self {
            atlas,
            ui_atlas,
            item_icons,
            entity_atlas,
            particle_atlas,
            entity_models,
            player_wide,
            player_slim,
            cape_model: cape,
            alt_font,
            bed_head_model: bed_head,
            bed_foot_model: bed_foot,
            armor_models,
            shield,
            grass_colormap,
            foliage_colormap,
            dry_foliage_colormap,
            block_uvs,
            block_shapes,
            cross_blocks,
            cross_extra_shapes,
        })
    }
}

// Ein im Hintergrund fertig gebautes Resourcepack (s. App::reload_resource_pack): alles,
// was ohne GPU und ohne App-Zustand entsteht. Die eigene Installation gehoert dazu -- der
// Bau braucht sie veraenderlich, und die der App bleibt waehrenddessen benutzbar.
// Eine Bossbar (ClientBossBar). Der Fortschritt gleitet je Aenderung 100 ms lang vom
// angezeigten zum neuen Wert.
pub(crate) struct BossBarState {
    pub(crate) uuid: [u8; 16],
    pub(crate) name: Vec<protocol::nbt::TextSpan>,
    pub(crate) shown: f32,
    pub(crate) target: f32,
    pub(crate) set_at: Instant,
    pub(crate) color: i32,
    pub(crate) style: i32,
}

impl BossBarState {
    pub(crate) fn percent(&self, jetzt: Instant) -> f32 {
        let t = (jetzt.saturating_duration_since(self.set_at).as_secs_f32() / 0.1).clamp(0.0, 1.0);
        self.shown + (self.target - self.shown) * t
    }
}

// BossBar.Color und die Kerbenstile in Ordinal-Reihenfolge.
pub(crate) const BOSS_BAR_COLORS: [&str; 7] = ["pink", "blue", "red", "green", "yellow", "purple", "white"];
pub(crate) const BOSS_BAR_NOTCHES: [&str; 4] = ["6", "10", "12", "20"];

pub(crate) struct LoadedPack {
    pub(crate) installation: assets::MinecraftInstallation,
    pub(crate) sky: Vec<(i32, Vec<custom_sky::LayerDef>)>,
    pub(crate) assets: WorldAssets,
    pub(crate) sounds: vanilla_sounds::VanillaSounds,
    pub(crate) glint: [Option<assets::DecodedTexture>; 2],
    pub(crate) weather: [WeatherTexture; 2],
    pub(crate) fonts: assets::PackFonts,
}

pub(crate) enum PackLoadMsg {
    Progress(u32),
    Done(Result<Box<LoadedPack>, String>),
}

// Ein laufender Pack-Wechsel: woher die Meldungen kommen, welche Benachrichtigung den
// Fortschritt zeigt und welcher Pack es wird.
pub(crate) enum PackVorrat {
    Laeuft(std::sync::mpsc::Receiver<PackLoadMsg>),
    Fertig(Box<LoadedPack>),
}

// Ein Stapel, wie reload_resource_pack ihn baut (s. App::pack_plan).
pub(crate) struct PackPlan {
    pub(crate) skins: Vec<String>,
    pub(crate) exclusions: std::collections::BTreeMap<String, Vec<String>>,
    pub(crate) server_ids: Vec<[u8; 16]>,
    pub(crate) oben: Vec<assets::PackDir>,
    pub(crate) unten: Vec<assets::PackDir>,
    pub(crate) key: String,
}

// Woran zwei Stapel als gleich erkannt werden: die Ordner beider Haelften samt dem, was darin
// ausgeschaltet ist.
pub(crate) fn pack_stack_key(oben: &[assets::PackDir], unten: &[assets::PackDir]) -> String {
    let teil = |dirs: &[assets::PackDir]| dirs.iter().map(|d| format!("{}[{}]", d.path.display(), d.excluded.join(","))).collect::<Vec<_>>().join(";");
    format!("{}|{}", teil(oben), teil(unten))
}

// Baut ein Pack in einem eigenen Thread; Fortschritt und Ergebnis kommen ueber den Kanal.
// `leise`: Vorrat fuer einen anderen Tab -- der Thread laeuft mit niedrigster Prioritaet, das
// laufende Spiel soll davon nichts merken.
pub(crate) fn spawn_pack_build(oben: Vec<assets::PackDir>, unten: Vec<assets::PackDir>, leise: bool) -> std::io::Result<std::sync::mpsc::Receiver<PackLoadMsg>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new().name("resource-pack".into()).spawn(move || {
        #[cfg(target_os = "linux")]
        if leise {
            // SAFETY: setpriority auf den eigenen Thread (unter Linux gilt PRIO_PROCESS mit 0
            // nur fuer den aufrufenden Thread).
            unsafe {
                libc::setpriority(libc::PRIO_PROCESS, 0, 19);
            }
        }
        #[cfg(not(target_os = "linux"))]
        let _ = leise;
        let ergebnis = (|| -> Result<Box<LoadedPack>, String> {
            let mut installation = assets::MinecraftInstallation::locate_stack(MC_VERSION, oben, unten).map_err(|e| e.to_string())?;
            let assets = WorldAssets::build_with_progress(&mut installation, &mut |p| {
                let _ = tx.send(PackLoadMsg::Progress(p));
            })
            .map_err(|e| e.to_string())?;
            let sounds = vanilla_sounds::VanillaSounds::load(&mut installation);
            let _ = tx.send(PackLoadMsg::Progress(95));
            let glint = load_glint_textures(&mut installation);
            let weather = load_weather_textures(&mut installation);
            let fonts = installation.load_pack_fonts();
            let sky = custom_sky::load_defs(&mut installation);
            Ok(Box::new(LoadedPack { installation, sky, assets, sounds, glint, weather, fonts }))
        })();
        let _ = tx.send(PackLoadMsg::Done(ergebnis));
    })?;
    Ok(rx)
}

pub(crate) struct PackLoadJob {
    pub(crate) rx: std::sync::mpsc::Receiver<PackLoadMsg>,
    pub(crate) note: notifications::NotificationId,
    pub(crate) skins: Vec<String>,
    // Was in diesen Packs ausgeschaltet ist (s. App::loaded_skin_exclusions).
    pub(crate) exclusions: std::collections::BTreeMap<String, Vec<String>>,
    // Server-Resourcepacks in diesem Stapel (s. server_packs) -- bekommen am Ende ihre Meldung.
    pub(crate) server_ids: Vec<[u8; 16]>,
}

// Titel und Text der Fortschrittsmeldung beim Pack-Wechsel.
pub(crate) fn pack_load_note_text(skins: &[String], server: bool, prozent: u32) -> (String, String) {
    let titel = if server {
        "Server-Resourcepack wird geladen".to_string()
    } else if !skin_stack_has_packs(skins) {
        "Vanilla-Texturen werden geladen".to_string()
    } else {
        format!("Skin wird geladen: {}", skin_stack_label(skins))
    };
    (titel, format!("{prozent} %"))
}
