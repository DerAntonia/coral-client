// Gehaltene Items, Ruestung, Besatz, Elytren, Schild, Feuer an Entities und Item-Entities.

use super::*;

impl App {
    // Die Blockarme eines FREMDEN Entities -- SOFORT, sobald er das Schild benutzt, wie in
    // Vanilla. Es gab einmal ein Modul, das das Schild erst nach der 5-Tick-Blockfrist
    // zeigte; entfernt (2026-09-19): man muss sofort sehen, dass der Gegner hochzieht, um
    // reagieren zu koennen.
    pub(crate) fn entity_blocking_arms(&self, e: &world::Entity) -> (bool, bool) {
        blocking_arms(e.using_item_right_arm(), e.arm_items())
    }

    // Vanillas Benutzungshaltung ohne Schild und Speer (AvatarRenderer.getArmPose): welcher Arm
    // und wie. `arms`: was der rechte und der linke Arm haelt; `main_right`: Haupthand rechts.
    // Eine geladene Armbrust geht in den Anschlag, sobald man nicht gerade zuschlaegt.
    pub(crate) fn use_pose(
        &self,
        using_right: Option<bool>,
        arms: (Option<world::HeldItem>, Option<world::HeldItem>),
        main_right: bool,
        use_ticks: f32,
        swinging: bool,
    ) -> Option<(bool, world::entity_model::UsePose)> {
        use world::entity_model::UsePose;
        use world::item_behavior as ib;
        let item = |rechts: bool| if rechts { arms.0 } else { arms.1 };
        if let Some(rechts) = using_right {
            let h = item(rechts)?;
            let haltung = match world::entity::held_use_animation(&h) {
                ib::USE_ANIM_BOW => UsePose::Bow,
                ib::USE_ANIM_TRIDENT => UsePose::ThrowTrident,
                ib::USE_ANIM_CROSSBOW => {
                    let max = world::item_components::resolve(h.stack).map_or(25.0, |s| self.enchantments.crossbow_pull_ticks(&s) as f32);
                    UsePose::CrossbowCharge { ticks: use_ticks, max }
                }
                ib::USE_ANIM_SPYGLASS => UsePose::Spyglass,
                ib::USE_ANIM_TOOT_HORN => UsePose::TootHorn,
                ib::USE_ANIM_BRUSH => UsePose::Brush,
                _ => return None,
            };
            return Some((rechts, haltung));
        }
        if swinging {
            return None;
        }
        [main_right, !main_right]
            .into_iter()
            .find(|&rechts| item(rechts).is_some_and(|h| world::entity::held_charged_crossbow(&h)))
            .map(|rechts| (rechts, UsePose::CrossbowHold))
    }

    // Dasselbe wie Entity::using_item_right_arm, nur fuer uns selbst: der eigene
    // Benutzungszustand kommt nicht vom Server zurueck, sondern aus der eigenen Buchfuehrung
    // (s. using_item / using_offhand).
    pub(crate) fn own_using_item_right_arm(&self) -> Option<bool> {
        if let Some(stufe) = self.hand_debug_bow_active() {
            return (stufe > 0).then_some(true);
        }
        self.using_item.then_some(!self.using_offhand)
    }

    // Hand-Debug-Bogen (s. App::hand_debug_bow): die Stufe, solange Hand-Debug laeuft.
    pub(crate) fn hand_debug_bow_active(&self) -> Option<u8> {
        self.hand_debug_bow.as_ref().filter(|_| self.hand_debug_scale().is_some()).map(|(k, _)| *k)
    }

    // Wie lange die eigene Hand ihr Item schon benutzt (Ticks samt Zwischenbild). Fuer den
    // Hand-Debug-Bogen so lange, dass Vanillas Bogen-Definition die Stufe waehlt (pulling_1 ab
    // 13 Ticks, pulling_2 ab 18); Stufe 0 erst nach dem Heranziehen (hold::BOW_DRAW_TICKS).
    pub(crate) fn hand_use_ticks(&self, tick_progress: f32) -> f32 {
        match self.hand_debug_bow_active() {
            Some(k) => [0.0, 8.0, 15.0, 22.0][k as usize],
            None => self.own_use_ticks as f32 + tick_progress,
        }
    }

    // Unter welchem Namen die Hand-Debug-Werte dieses Items stehen: der Itemname, beim gespannten
    // Bogen je Stufe "bow_pulling_0" .. "_2" (fehlt sie, gilt "bow").
    pub(crate) fn tuning_name(&self, item: world::HeldItem, hand: i32) -> Option<String> {
        let name = world::item_registry::item_name(item.item_id)?;
        if name == "bow" && self.own_using_item_right_arm() == Some(hand == protocol::packets::HAND_MAIN) {
            let t = self.hand_use_ticks(0.0);
            let stufe = if t >= 18.0 { 2 } else { i32::from(t >= 13.0) };
            return Some(format!("bow_pulling_{stufe}"));
        }
        Some(name.to_string())
    }

    // Die Hand-Debug-Werte dieses Items (s. tuning_name), sonst die des Itemnamens, sonst null.
    // Die Hand an der Sehne: spannt eine Hand mit Modified View Model den Bogen, laesst die andere
    // ihr Item los und liegt an der Sehne, mit eigenen Werten je Spannstufe
    // ("bow_pulling_N_sehne", Hand-Debug-Knopf "Sehnenhand"). Ihre Hand und der Name der Werte.
    pub(crate) fn string_hand(&self) -> Option<(i32, String)> {
        if !self.view_model_on() {
            return None;
        }
        let rechts = self.own_using_item_right_arm()?;
        let (main, off) = self.own_hand_items();
        let bogen_hand = if rechts { protocol::packets::HAND_MAIN } else { protocol::packets::HAND_OFF };
        let name = self.tuning_name(if rechts { main } else { off }?, bogen_hand)?;
        let andere = if rechts { protocol::packets::HAND_OFF } else { protocol::packets::HAND_MAIN };
        name.starts_with("bow_pulling_").then(|| (andere, format!("{name}_sehne")))
    }

    // Die Werte der Sehnenhand (s. string_hand); ohne eigene liegt sie wie die leere Hand.
    pub(crate) fn string_hand_tuning(&self, name: &str) -> [i32; 16] {
        self.settings.hand_fist_items.get(name).copied().unwrap_or_else(world::item_inspect::rest_tuning)
    }

    pub(crate) fn hand_tuning(&self, item: world::HeldItem, hand: i32) -> [i32; 16] {
        let werte = |n: &str| self.settings.hand_fist_items.get(n).copied();
        self.tuning_name(item, hand)
            .and_then(|n| werte(&n))
            .or_else(|| world::item_registry::item_name(item.item_id).and_then(werte))
            .unwrap_or_default()
    }

    // Was der EIGENE Spieler in den Haenden haelt -- aus dem Inventar statt aus dem
    // Entity-Zustand: fuer sich selbst schickt der Server kein Set Equipment, die Haupthand
    // ist der aktive Hotbar-Slot (dieselbe Zuordnung wie in totem_in_haenden).
    pub(crate) fn own_hand_stack(&self, hand: i32) -> Option<&protocol::item::ItemStack> {
        if hand != protocol::packets::HAND_OFF && self.hand_debug_bow_active().is_some() {
            return self.hand_debug_bow.as_ref().map(|(_, s)| s);
        }
        let slot = if hand == protocol::packets::HAND_OFF {
            inventory::PLAYER_OFFHAND
        } else {
            inventory::PLAYER_HOTBAR_START + self.inventory.held_slot.min(8)
        };
        self.inventory.player.get(slot).and_then(|s| s.as_ref()).filter(|st| st.item_id != 0)
    }

    pub(crate) fn own_hand_items(&self) -> (Option<world::HeldItem>, Option<world::HeldItem>) {
        let von_slot = |slot: usize| -> Option<world::HeldItem> {
            let stack = self.inventory.player.get(slot).and_then(|s| s.as_ref())?;
            (stack.item_id != 0).then(|| world::HeldItem::from_stack(stack))
        };
        let haupt = match (self.hand_debug_bow_active(), &self.hand_debug_bow) {
            (Some(_), Some((_, s))) => Some(world::HeldItem::from_stack(s)),
            _ => von_slot(inventory::PLAYER_HOTBAR_START + self.inventory.held_slot.min(8)),
        };
        (haupt, von_slot(inventory::PLAYER_OFFHAND))
    }

    // Was die Figur im Inventar in den Haenden haelt: wie own_hand_items, nur haengt gerade ein
    // Stapel an der Maus, haelt sie DEN in der Haupthand (ersetzt, was dort liegt).
    pub(crate) fn doll_hand_items(&self) -> (Option<world::HeldItem>, Option<world::HeldItem>) {
        let (main, off) = self.own_hand_items();
        let an_der_maus = self.inventory.cursor.as_ref().filter(|s| s.item_id != 0).map(world::HeldItem::from_stack);
        (an_der_maus.or(main), off)
    }

    // Die eigene Ruestung, in derselben Anordnung wie Entity::armor (nach
    // ArmorSlot::index): Fuesse, Beine, Brust, Kopf.
    //
    // Quelle ist das EIGENE Inventar, nicht das Set-Equipment-Paket -- das schickt der Server
    // an alle ANDEREN, nicht an einen selbst (derselbe Grund wie beim Armschwung, s.
    // ClientEvent::EntityAnimation). Die vier Ruestungsslots des Spielerinventars liegen ab
    // PLAYER_ARMOR_START und sind von oben nach unten sortiert (Helm zuerst), Entity::armor
    // dagegen von unten nach oben -- deshalb der umgekehrte Zugriff.
    pub(crate) fn own_armor(&self) -> [Option<world::HeldItem>; 4] {
        let von_slot = |slot: usize| -> Option<world::HeldItem> {
            let stack = self.inventory.player.get(slot).and_then(|s| s.as_ref())?;
            (stack.item_id != 0).then(|| world::HeldItem::from_stack(stack))
        };
        std::array::from_fn(|i| von_slot(inventory::PLAYER_ARMOR_START + (3 - i)))
    }

    // Getragene Ruestung ueber das Grundmodell legen. `pose` und `base` sind DIESELBEN wie
    // beim Koerper -- nur so folgt der Helm dem Kopf und die Beinschienen den Beinen.
    //
    // Vanillas Reihenfolge (ArmorFeatureRenderer.render) ist Brust, Hose, Schuhe, Kopf. Sie
    // ist hier nicht bloss Geschmack: Brustpanzer und Hose ueberlappen sich am Rumpf, und
    // wer die Hose zuletzt zeichnete, saehe ihren Guertel ueber dem Panzer liegen.
    pub(crate) fn emit_armor(
        &self,
        armor: &[Option<world::HeldItem>; 4],
        models: &[world::Model; 4],
        pose: &world::Pose,
        base: world::entity_model::Affine,
        out: &mut Vec<world::BlockVertex>,
    ) {
        for slot in world::armor::ArmorSlot::ALL {
            let Some(item) = armor[slot.index()] else { continue };
            let Some(name) = world::item_registry::item_name(item.item_id) else { continue };
            // Ein Kopf auf dem Kopf (HeadFeatureRenderer): am Kopfteil, scale(1.1875, -1.1875,
            // -1.1875), translate(-0.5, 0, -0.5), dann das Kopfmodell wie in der Hand.
            if slot == world::armor::ArmorSlot::Head {
                if let Some((tex, hut)) = self.held_skull_texture(&item) {
                    let kopf = world::entity_model::part_transform(&models[slot.index()], pose, base, "head");
                    if let Some(kopf) = kopf {
                        let b = kopf
                            .then(world::entity_model::Affine::scale([1.1875, -1.1875, -1.1875]))
                            .then(world::entity_model::Affine::translation([-0.5, 0.0, -0.5]));
                        held_item::emit_skull_model(tex, hut, b, ENTITY_BRIGHTNESS, out);
                    }
                    continue;
                }
            }
            // Welches Equipment-Asset das Item an DIESEM Slot zeigt -- aus der equippable-
            // Component, sonst Vanillas Standard. Ein Helm im Stiefel-Slot wird nicht als
            // Stiefel gezeichnet (Vanilla prueft dasselbe: `component.slot() == slot`).
            let Some(asset) = world::armor::worn_asset(name, item.equip, slot) else { continue };
            let Some(equipment) = self.equipment_models.get(&asset).and_then(Option::as_ref) else { continue };
            let model = &models[slot.index()];
            // Die Ebenen in Dateireihenfolge (EquipmentLayerRenderer.renderLayers). Eine
            // faerbbare Ebene nimmt die Farbe des Items, sonst ihre Ersatzfarbe; ist beides 0,
            // faellt sie weg -- Vanillas getColorForLayer.
            for ebene in equipment.layers(slot.inner()) {
                let tint = match ebene.dyeable {
                    None => [1.0, 1.0, 1.0],
                    Some(ersatz) => {
                        let c = item.dyed.filter(|&c| c != 0).unwrap_or(ersatz);
                        if c == 0 {
                            continue;
                        }
                        [((c >> 16) & 0xff) as f32 / 255.0, ((c >> 8) & 0xff) as f32 / 255.0, (c & 0xff) as f32 / 255.0]
                    }
                };
                let Some(tex) = self.equipment_texture(&ebene.texture) else { continue };
                let start = out.len();
                world::entity_model::emit_tinted_two_sided(model, pose, base, tex, tint, ENTITY_BRIGHTNESS, out);
                self.push_armor_glint(item, model, pose, base, out, start);
            }
            let asset_pfad = asset.split_once(':').map_or(asset.as_str(), |(_, p)| p);
            // Ruestungsbesatz darueber (EquipmentRenderer: die Trim-Ebene nach den Grundebenen,
            // mit demselben Modell).
            if let Some(tex) = self
                .trim_key(&item, slot, asset_pfad)
                .and_then(|k| self.trim_textures.get(&k).copied().flatten())
                .and_then(|platz| self.entity_atlas.trim_slot_rect(platz))
            {
                let start = out.len();
                world::entity_model::emit_two_sided(model, pose, base, tex, ENTITY_BRIGHTNESS, out);
                self.push_armor_glint(item, model, pose, base, out, start);
            }
        }
    }

    // Schluessel der Trim-Textur eines Ruestungsteils: Ebene (humanoid oder
    // humanoid_leggings), Muster und Palette -- die Palette des Materials, ausser das
    // Material nennt fuer dieses Equipment-Asset (Pfad ohne Namespace, "iron") eine Ausnahme
    // (Eisen-Trim auf Eisenruestung -> iron_darker, sonst verschwaende er darin).
    pub(crate) fn trim_key(&self, item: &world::HeldItem, slot: world::armor::ArmorSlot, asset: &str) -> Option<String> {
        if !self.settings.armor_trims {
            return None;
        }
        let (material, muster) = item.trim?;
        let (palette, ausnahmen) = self.trim_registries.materials.get(usize::try_from(material).ok()?)?;
        let muster = self.trim_registries.patterns.get(usize::try_from(muster).ok()?)?;
        let palette = ausnahmen.get(asset).unwrap_or(palette);
        let ebene = if slot.inner() { "humanoid_leggings" } else { "humanoid" };
        Some(format!("{ebene}/{muster}_{palette}"))
    }

    // Die Trim-Texturen aller sichtbaren Ruestungen erzeugen und hochladen, soweit noch nicht
    // geschehen. Sind alle Plaetze belegt, wird der aelteste verdraengt.
    // Die Windkugel-Textur laeuft in Vanilla durch: RenderTypes.breezeWind verschiebt sie um
    // ageInTicks * 0.03 in u (WindChargeRenderer.xOffset), mit wiederholter Textur. Im Atlas
    // gibt es kein Wiederholen -- deshalb wird die Textur an ihrem Platz selbst verschoben,
    // um ganze Texel wie bei Vanillas Nearest-Abtastung. Ein gemeinsamer Takt fuer alle
    // Windkugeln statt ihres eigenen Alters; nebeneinander faellt das nicht auf.
    pub(crate) fn scroll_wind_charge_texture(&mut self) {
        let fliegt = self.entities.iter().any(|(_, e)| matches!(world::entity_type_name(e.entity_type), Some("wind_charge" | "breeze_wind_charge")));
        if !fliegt {
            return;
        }
        let Some(rect) = self.entity_atlas.rect(world::entity_model::WIND_CHARGE_TEXTURE) else { return };
        let groesse = self.entity_atlas.size as f32;
        let (x0, y0) = ((rect[0] * groesse).round() as u32, (rect[1] * groesse).round() as u32);
        let (w, h) = (((rect[2] - rect[0]) * groesse).round() as u32, ((rect[3] - rect[1]) * groesse).round() as u32);
        if w == 0 || h == 0 {
            return;
        }
        let alter = self.own_age_ticks + self.tick_progress();
        let versatz = (((alter * 0.03) % 1.0) * w as f32) as u32 % w;
        if self.wind_charge_shift == Some(versatz) {
            return;
        }
        let Some(gpu) = self.gpu.as_ref() else { return };
        let stride = self.entity_atlas.size as usize * 4;
        let mut pixel = vec![0u8; (w * h * 4) as usize];
        for y in 0..h as usize {
            for x in 0..w as usize {
                // uv' = uv + versatz: an Stelle x steht, was vorher bei x + versatz stand.
                let quelle = (y0 as usize + y) * stride + (x0 as usize + (x + versatz as usize) % w as usize) * 4;
                let ziel = (y * w as usize + x) * 4;
                pixel[ziel..ziel + 4].copy_from_slice(&self.entity_atlas.rgba[quelle..quelle + 4]);
            }
        }
        gpu.block_renderer.upload_item(&gpu.queue, (x0, y0), w, h, &pixel);
        self.wind_charge_shift = Some(versatz);
    }

    // Textur einer Equipment-Ebene ("ns:entity/equipment/<ebene>/<name>"): aus dem Atlas, wenn
    // sie dort steht (Vanilla-Ruestung, auch vom Pack ersetzt), sonst aus den nachgeladenen
    // Plaetzen (s. ensure_trim_textures).
    pub(crate) fn equipment_texture(&self, texture: &str) -> Option<[f32; 4]> {
        if let Some(rect) = texture.strip_prefix("minecraft:").and_then(|p| self.entity_atlas.rect(p)) {
            return Some(rect);
        }
        let platz = self.trim_textures.get(&format!("eq:{texture}")).copied().flatten()?;
        self.entity_atlas.trim_slot_rect(platz)
    }

    // Vor dem Bauen der Vertices: Equipment-Modelle der getragenen Ruestung laden und
    // Texturen nachholen, die nicht im Atlas stehen -- Trims (zusammengesetzt aus Muster und
    // Palette) und Ebenen eigener Equipment-Assets. Beide teilen sich die Trim-Plaetze; ist
    // alles belegt, weicht der am laengsten belegte.
    pub(crate) fn ensure_trim_textures(&mut self) {
        let mut schluessel: Vec<String> = Vec::new();
        let ruestungen = self.entities.iter().map(|(_, e)| e.armor).chain(std::iter::once(self.own_armor())).collect::<Vec<_>>();
        for armor in &ruestungen {
            for slot in world::armor::ArmorSlot::ALL {
                let Some(item) = armor[slot.index()] else { continue };
                let Some(name) = world::item_registry::item_name(item.item_id) else { continue };
                let Some(asset) = world::armor::worn_asset(name, item.equip, slot) else { continue };
                if !self.equipment_models.contains_key(&asset) {
                    let modell = self.installation.load_equipment_model(&asset);
                    self.equipment_models.insert(asset.clone(), modell);
                }
                let Some(Some(equipment)) = self.equipment_models.get(&asset) else { continue };
                for ebene in equipment.layers(slot.inner()) {
                    let im_atlas = ebene.texture.strip_prefix("minecraft:").is_some_and(|p| self.entity_atlas.rect(p).is_some());
                    let k = format!("eq:{}", ebene.texture);
                    if !im_atlas && !self.trim_textures.contains_key(&k) && !schluessel.contains(&k) {
                        schluessel.push(k);
                    }
                }
                let asset_pfad = asset.split_once(':').map_or(asset.as_str(), |(_, p)| p);
                if let Some(k) = self.trim_key(&item, slot, asset_pfad) {
                    if !self.trim_textures.contains_key(&k) && !schluessel.contains(&k) {
                        schluessel.push(k);
                    }
                }
            }
            // Die Fluegel des Brust-Items (s. emit_elytra): eigenes Asset, Ebene "wings".
            let Some(item) = armor[world::armor::ArmorSlot::Chest.index()] else { continue };
            let Some(asset) = world::item_registry::item_name(item.item_id).and_then(|n| world::armor::wings_asset(n, item.equip)) else { continue };
            if !self.equipment_models.contains_key(&asset) {
                let modell = self.installation.load_equipment_model(&asset);
                self.equipment_models.insert(asset.clone(), modell);
            }
            let Some(Some(equipment)) = self.equipment_models.get(&asset) else { continue };
            for ebene in &equipment.wings {
                let im_atlas = ebene.texture.strip_prefix("minecraft:").is_some_and(|p| self.entity_atlas.rect(p).is_some());
                let k = format!("eq:{}", ebene.texture);
                if !im_atlas && !self.trim_textures.contains_key(&k) && !schluessel.contains(&k) {
                    schluessel.push(k);
                }
            }
        }
        for k in schluessel {
            let pixel = if let Some(textur) = k.strip_prefix("eq:") {
                // Equipment-Texturen haben dasselbe 64x32-Raster wie die Trim-Plaetze; ein
                // HD-Pack mit groesseren Bildern bleibt hier aussen vor.
                let (ns, pfad) = textur.split_once(':').unwrap_or(("minecraft", textur));
                self.installation
                    .load_texture(&format!("{ns}/textures/{pfad}.png"))
                    .ok()
                    .filter(|t| t.width == assets::TRIM_WIDTH && t.height == assets::TRIM_HEIGHT)
                    .map(|t| t.rgba)
            } else {
                k.split_once('/')
                    .and_then(|(ebene, rest)| rest.split_once('_').map(|(muster, palette)| (ebene, muster, palette)))
                    .and_then(|(ebene, muster, palette)| assets::build_trim_texture(&mut self.installation, ebene, muster, palette))
            };
            let Some(pixel) = pixel else {
                self.trim_textures.insert(k, None);
                continue;
            };
            let kapazitaet = self.entity_atlas.trim_slot_count();
            if kapazitaet == 0 {
                return;
            }
            let platz = if self.trim_slot_order.len() < kapazitaet {
                self.trim_slot_order.len()
            } else {
                let Some(alt) = self.trim_slot_order.pop_front() else { return };
                self.trim_textures.remove(&alt).flatten().unwrap_or(0)
            };
            self.trim_slot_order.push_back(k.clone());
            if let (Some(gpu), Some(origin)) = (self.gpu.as_ref(), self.entity_atlas.trim_slot_origin(platz)) {
                gpu.block_renderer.upload_cape(&gpu.queue, origin, assets::TRIM_WIDTH, assets::TRIM_HEIGHT, &pixel);
            }
            self.trim_textures.insert(k, Some(platz));
        }
    }

    // Die Fluegel auf dem Ruecken (WingsLayer): fuer jedes Brust-Item, dessen Equipment-Asset
    // eine Ebene "wings" hat -- von Haus aus die Elytra, Server-Packs geben auch anderen Items
    // eigene Fluegel. Vanilla setzt das Modell um 0.125 nach hinten an die Wurzel des Spielermodells -- nicht an
    // den Rumpf, deshalb macht es das Kippen beim Schleichen nicht mit, sondern hat dafuer eigene
    // Winkel (s. world::elytra_pose). Wer einen Umhang hat, traegt dessen Bild auch auf den
    // Fluegeln (SkinTextures.elytraTexture faellt auf den Umhang zurueck; beide Bilder haben
    // dasselbe 64x32-Raster), sofern die Ebene use_player_texture hat. Liefert, ob das Asset
    // Fluegel hat -- dann entfaellt der Umhang (CapeLayer).
    pub(crate) fn emit_elytra(
        &self,
        armor: &[Option<world::HeldItem>; 4],
        cape_tex: Option<[f32; 4]>,
        glide: Option<&world::Glide>,
        sneaking: bool,
        base: world::entity_model::Affine,
        out: &mut Vec<world::BlockVertex>,
    ) -> bool {
        let Some(chest) = armor[world::armor::ArmorSlot::Chest.index()] else { return false };
        let Some(asset) = world::item_registry::item_name(chest.item_id).and_then(|n| world::armor::wings_asset(n, chest.equip)) else {
            return false;
        };
        let Some(Some(equipment)) = self.equipment_models.get(&asset) else { return false };
        if equipment.wings.is_empty() {
            return false;
        }
        let base = base.then(world::entity_model::Affine::translation([0.0, 0.0, 0.125]));
        let pose = world::elytra_pose(glide, sneaking);
        let anfang = out.len();
        for ebene in &equipment.wings {
            let tex = match cape_tex.filter(|_| ebene.use_player_texture) {
                Some(t) => Some(t),
                None => self.equipment_texture(&ebene.texture),
            };
            if let Some(tex) = tex {
                world::emit_entity_model(&world::elytra_model(), &pose, base, tex, ENTITY_BRIGHTNESS, out);
            }
        }
        // Beidseitig wie der End-Kristall: Vanilla zeichnet Equipment mit armorCutoutNoCull.
        // Viele Umhang-Bilder fuellen nur die Rueckseite der Fluegel, die Vorderseite ist
        // durchsichtig -- von vorn sieht man dann die Innenseite der Rueckseite.
        let rueckseiten: Vec<world::BlockVertex> = out[anfang..].chunks_exact(3).flat_map(|t| [t[0], t[2], t[1]]).collect();
        out.extend(rueckseiten);
        true
    }

    // Glint auf einer Ruestungsebene (ARMOR_ENTITY_GLINT). Vanilla legt ihn auf JEDE Ebene
    // eines glitzernden Teils (EquipmentRenderer: ein Glint-Verbraucher je Ebene).
    pub(crate) fn push_armor_glint(
        &self,
        item: world::HeldItem,
        model: &world::entity_model::Model,
        pose: &world::Pose,
        base: world::entity_model::Affine,
        out: &[world::BlockVertex],
        start: usize,
    ) {
        if item.enchanted {
            self.push_glint(out, start, render::glint::GLINT_SCALE_ARMOR, render::glint::GLINT_TEXTURE_ARMOR, |l| {
                world::entity_model::emit_two_sided(model, pose, base, [0.0, 0.0, 1.0, 1.0], 1.0, l);
            });
        }
    }

    // Item in der Hand eines Entities, an den jeweiligen Arm gehaengt. Vanilla zeichnet beide
    // Haende in HeldItemFeatureRenderer.render -- rechte zuerst, dann die linke.
    //
    // Die zugehoerige ARMHALTUNG setzt apply_item_arm_poses, und zwar vor diesem Aufruf: das
    // Item haengt an der Armtransformation, also muss der Arm schon angehoben sein.
    pub(crate) fn emit_held_items(
        &self,
        // (rechter Arm, linker Arm) -- bei Linkshaendern ist die Haupthand links.
        hands: (Option<world::HeldItem>, Option<world::HeldItem>),
        blocking: (bool, bool),
        // Schwungfortschritt eines Speerstichs der Haupthand (sonst 0) und ob sie der rechte
        // Arm ist (s. world::entity_model::third_person_stab_item_offset).
        stich: (f32, bool),
        // Zusatzdrehung des Items an der rechten bzw. linken Hand (Item ansehen, sonst Einheit).
        ansehen: (world::Affine, world::Affine),
        model: &world::entity_model::Model,
        pose: &world::Pose,
        base: world::entity_model::Affine,
        out: &mut Vec<world::BlockVertex>,
    ) {
        for (item, right, blockt, extra) in [(hands.0, true, blocking.0, ansehen.0), (hands.1, false, blocking.1, ansehen.1)] {
            let Some(item) = item else { continue };
            let arm_part = if right { "right_arm" } else { "left_arm" };
            let Some(arm) = world::entity_model::part_transform(model, pose, base, arm_part) else { continue };
            // Das Schild geht einen eigenen Weg -- sein Item-Modell hat gar keine Geometrie
            // (s. world::entity_model::shield_model).
            if world::item_registry::item_name(item.item_id) == Some("shield") {
                self.emit_shield(held_item::hand_pre_transform(arm, right).then(extra), right, false, blockt, item.enchanted, item.banner, out);
                continue;
            }
            let start = out.len();
            let versatz = if right == stich.1 && stich.0 > 0.0 { world::entity_model::third_person_stab_item_offset(stich.0) } else { world::Affine::IDENTITY };
            let pre = held_item::hand_pre_transform(arm, right).then(versatz).then(extra);
            // Pack-Modelle je Stapel (Verzauberung, custom_model_data ...), sobald geladen.
            if let Some(modell) = self.held_model_override(&item, right).filter(|m| self.held_items.has_model(m)) {
                self.held_items.emit_model_pre(&modell, pre, right, ENTITY_BRIGHTNESS, false, out);
                if item.enchanted {
                    self.push_glint(out, start, render::glint::GLINT_SCALE_ITEM, render::glint::GLINT_TEXTURE_ITEM, |l| {
                        self.held_items.emit_model_pre(&modell, pre, right, 1.0, true, l);
                    });
                }
                continue;
            }
            self.held_items.set_skull(self.held_skull_texture(&item));
            self.held_items.emit_pre(item.model_id, item.potion_color, pre, right, ENTITY_BRIGHTNESS, false, out);
            if item.enchanted {
                self.push_glint(out, start, render::glint::GLINT_SCALE_ITEM, render::glint::GLINT_TEXTURE_ITEM, |l| {
                    self.held_items.emit_pre(item.model_id, item.potion_color, pre, right, 1.0, true, l);
                });
            }
        }
    }

    // Das Schild als Entity-Modell. `pre` ist dieselbe Vorkette wie bei jedem anderen
    // Handgegenstand (held_item::hand_pre_transform bzw. Vanillas Ego-Kette), danach folgt
    // Transformation.apply und zuletzt ShieldModelRenderer.render:
    //
    //     1. verschieben um translation (x links negiert)
    //     2. drehen um rotation
    //     3. skalieren um scale
    //     4. verschieben um (-0.5, -0.5, -0.5)   <- gehoert HIERHIN
    //     5. dann der SpecialModelRenderer: spiegeln (1, -1, -1)
    //
    // ECHTER FEHLER GEWESEN (das Schild schwebte in der Luft): ich hatte den
    // -0.5-Versatz weggelassen, weil ich ihn im Quad-Zweig (submitItem) vermutet hatte -- dort
    // zentriert er ja das 0..1-Wuerfelmodell eines gewoehnlichen Items. Er steht aber in
    // Transformation.apply SELBST, also VOR der Verzweigung in Quads und Spezialmodell, und
    // gilt damit fuer das Schild genauso. Ohne ihn sass es einen halben Block in jeder Achse
    // neben der Hand -- gut einen Meter daneben.
    //
    // Die Spiegelung -1 auf Y und Z hat uebrigens die Determinante +1: die Umlaufrichtung
    // der Dreiecke bleibt erhalten, das Backface-Culling der Pipeline greift also weiterhin
    // richtig.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn emit_shield(
        &self,
        pre: world::entity_model::Affine,
        right_hand: bool,
        first_person: bool,
        blocking: bool,
        glint: bool,
        banner: Option<world::entity::ShieldBanner>,
        out: &mut Vec<world::BlockVertex>,
    ) {
        use world::entity_model::Affine;
        let Some(tex) = self.entity_atlas.rect(self.shield.model.texture) else { return };
        let platte_tex = match banner {
            Some(_) => self.entity_atlas.rect(SHIELD_BANNER_BASE).unwrap_or(tex),
            None => tex,
        };
        let tabelle = match (first_person, blocking) {
            (true, false) => &self.shield.first_person,
            (true, true) => &self.shield.first_person_blocking,
            (false, false) => &self.shield.third_person,
            (false, true) => &self.shield.third_person_blocking,
        };
        // Vanilla nimmt den Eintrag des jeweiligen ItemDisplayContext UND spiegelt danach die
        // x-Verschiebung (Transformation.apply mit leftHanded) -- beides, nicht eines davon.
        let display = if right_hand { tabelle[0] } else { tabelle[1].mirrored() };
        let base = pre
            .then(Affine::translation(display.translation))
            .then(Affine::rotation_xyz_deg(display.rotation[0], display.rotation[1], display.rotation[2]))
            .then(Affine::scale(display.scale))
            .then(Affine::translation([-0.5, -0.5, -0.5]))
            .then(Affine::scale([1.0, -1.0, -1.0]));
        let start = out.len();
        world::emit_entity_model(&self.shield.plate, &world::Pose::new(), base, platte_tex, ENTITY_BRIGHTNESS, out);
        // Die Bannerlagen (BannerBlockEntityRenderer.renderCanvas): erst die Grundfarbe als
        // Lage "base", dann jedes Muster in seiner Farbe -- dieselbe Platte, gleiche Tiefe,
        // die spaetere Lage gewinnt (Tiefentest LessEqual).
        if let Some(banner) = banner {
            let lagen = std::iter::once(("base", banner.base)).chain(
                banner.layers().iter().filter_map(|(id, farbe)| Some((self.trim_registries.banners.get(*id as usize)?.as_str(), *farbe))),
            );
            for (muster, farbe) in lagen {
                if let Some(t) = self.entity_atlas.rect(&format!("entity/shield/{muster}")) {
                    world::entity_model::emit_tinted(&self.shield.plate, &world::Pose::new(), base, t, dye_rgb(farbe), ENTITY_BRIGHTNESS, out);
                }
            }
        }
        // Glint nur auf der Platte, mit der ITEM-Glinttextur (RenderLayers.entityGlint).
        if glint {
            self.push_glint(out, start, render::glint::GLINT_SCALE_SHIELD, render::glint::GLINT_TEXTURE_ITEM, |l| {
                world::emit_entity_model(&self.shield.plate, &world::Pose::new(), base, [0.0, 0.0, 1.0, 1.0], 1.0, l);
            });
        }
        world::emit_entity_model(&self.shield.handle, &world::Pose::new(), base, tex, ENTITY_BRIGHTNESS, out);
    }

    // Schild mit Banner im Inventar: wie set_shield_icon, nur die Platte mit shield_base und
    // den eingefaerbten Bannerlagen darueber (s. emit_shield).
    pub(crate) fn push_shield_banner_icon(&self, sprites: &mut Vec<UiSprite>, banner: world::entity::ShieldBanner, x: f32, y: f32, size: f32) {
        use world::entity_model::Affine;
        let base = Affine::translation([-0.5, -0.5, -0.5]).then(Affine::scale([1.0, -1.0, -1.0]));
        let boxen = |part: &world::Model, tex: [f32; 4]| {
            let mut v = Vec::new();
            world::emit_entity_model(part, &world::Pose::new(), base, tex, 1.0, &mut v);
            v
        };
        let (Some(holz), Some(grund)) = (self.entity_atlas.rect(self.shield.model.texture), self.entity_atlas.rect(SHIELD_BANNER_BASE)) else { return };
        let mut lagen = vec![(vec![boxen(&self.shield.plate, grund), boxen(&self.shield.handle, holz)], [1.0; 3])];
        let muster = std::iter::once(("base", banner.base))
            .chain(banner.layers().iter().filter_map(|(id, farbe)| Some((self.trim_registries.banners.get(*id as usize)?.as_str(), *farbe))));
        for (m, farbe) in muster {
            if let Some(t) = self.entity_atlas.rect(&format!("entity/shield/{m}")) {
                lagen.push((vec![boxen(&self.shield.plate, t)], dye_rgb(farbe)));
            }
        }
        self.item_icons.push_layers(sprites, "shield", &lagen, x, y, size, 1.0);
    }

    // Das Feuer um ein brennendes Entity (Vanillas FireCommandRenderer): uebereinander
    // gestaffelte, zur Kamera gedrehte Flaechen im Wechsel aus fire_0 und fire_1, jede 0.45
    // hoeher, etwas schmaler und etwas weiter hinten als die vorige, bis die Hoehe des Entities
    // erreicht ist. Voll hell (Licht 240) und beidseitig, weil der Entity-Durchgang Rueckseiten
    // verwirft.
    pub(crate) fn emit_entity_fire(&self, feet: [f32; 3], width: f32, height: f32, out: &mut Vec<world::BlockVertex>) {
        use world::entity_model::Affine;
        let ticks = self.own_age_ticks.max(0.0) as u64;
        let (Some(r0), Some(r1)) = (
            self.entity_atlas.animated_rect(ENTITY_FIRE_TEXTURES[0], ticks),
            self.entity_atlas.animated_rect(ENTITY_FIRE_TEXTURES[1], ticks),
        ) else {
            return;
        };
        let f = width * 1.4;
        if f <= 0.0 {
            return;
        }
        let (cam_yaw, cam_pitch) = self.vanilla_camera_angles();
        // Nur um die Hochachse zur Kamera (MathHelper.rotateAround(Y_AXIS, Kamera)).
        let drehung = affine_from_mat4(display_billboard_matrix(1, 0.0, 0.0, cam_yaw, cam_pitch));
        let mut rest = height / f;
        let basis = Affine::translation(feet)
            .then(Affine::scale([f; 3]))
            .then(drehung)
            .then(Affine::translation([0.0, 0.0, 0.3 - rest.trunc() * 0.02]));
        let (mut g, mut j, mut k) = (0.5f32, 0.0f32, 0.0f32);
        let mut l = 0u32;
        while rest > 0.0 {
            let [mut u0, v0, mut u1, v1] = if l.is_multiple_of(2) { r0 } else { r1 };
            if (l / 2).is_multiple_of(2) {
                std::mem::swap(&mut u0, &mut u1);
            }
            // Vanillas Ecken: (-g,-j) (g,-j) (g,1.4-j) (-g,1.4-j) mit den UVs (u1,v1) (u0,v1)
            // (u0,v0) (u1,v0) -- u0/u1 hier schon nach dem Tausch benannt.
            let ecke = |x: f32, y: f32, u: f32, v: f32| world::BlockVertex { position: basis.apply([x, y, k]), color: [1.0; 3], uv: [u, v] };
            let a = ecke(-g, -j, u1, v1);
            let b = ecke(g, -j, u0, v1);
            let c = ecke(g, 1.4 - j, u0, v0);
            let d = ecke(-g, 1.4 - j, u1, v0);
            out.extend_from_slice(&[a, b, c, a, c, d, a, c, b, a, d, c]);
            rest -= 0.45;
            j -= 0.45;
            g *= 0.9;
            k -= 0.03;
            l += 1;
        }
    }

    // Liegende Items und Wurfgeschosse: das Item-Modell mit seiner GROUND-Haltung.
    //   Dropped (ItemEntityRenderer): Hoehe aus der Modellbox (Unterkante + 1/16 ueber dem
    //     Boden), Wippen sin(Alter/10 + Versatz)*0.1 + 0.1, Drehung Alter/20 + Versatz um Y,
    //     und je nach Stapel 1 bis 5 Kopien -- duenne (flache) Modelle hintereinander, dicke
    //     zufaellig verstreut.
    //   Flying (FlyingItemEntityRenderer): skaliert und zur Kamera gedreht.
    // Der Versatz (uniqueOffset) ist in Vanilla zufaellig je Entity; hier aus der Entity-Id
    // abgeleitet, damit er ueber die Frames gleich bleibt.
    pub(crate) fn emit_item_entity(
        &self,
        id: i32,
        e: &world::Entity,
        art: world::ItemRenderKind,
        feet: [f32; 3],
        progress: f32,
        out: &mut Vec<world::BlockVertex>,
    ) {
        use world::entity_model::Affine;
        let item = match (e.item, art) {
            (Some(item), _) => item,
            (None, world::ItemRenderKind::Flying { default_item, .. }) => {
                let Some(item_id) = world::item_registry::item_id_by_name(default_item) else { return };
                world::HeldItem::plain(item_id)
            }
            // Ein liegendes Item ohne empfangenen Stapel zeigt Vanilla gar nicht (leerer Stack).
            (None, world::ItemRenderKind::Dropped) => return,
        };
        let mut lokal = Vec::new();
        self.held_items.set_skull(self.held_skull_texture(&item));
        // Laeuft das Item ueber den Core-Shader eines Packs, braucht es die Geometrie hier nur
        // fuer seine Masse (Hoehe ueber dem Boden, Abstand der Kopien) -- gezeichnet wird es
        // unten je Platzierung (s. HeldItems::emit_vanilla).
        let umgeleitet = self.held_items.routes_to_vanilla(item.model_id);
        self.held_items.without_vanilla(|| self.held_items.emit_ground(item.model_id, item.potion_color, Affine::IDENTITY, ENTITY_BRIGHTNESS, false, &mut lokal));
        if lokal.is_empty() {
            return;
        }
        let alter = e.age_ticks(progress);
        let platzierungen: Vec<Affine> = match art {
            world::ItemRenderKind::Flying { scale, .. } => {
                // Das eigene, eben geworfene Geschoss saesse sonst die ersten Ticks mitten im
                // Bild (aeltere Vanilla-Fassungen blenden es genauso aus).
                let [x, y, z] = feet;
                if alter < 2.0 && x * x + y * y + z * z < 12.25 {
                    return;
                }
                let (cam_yaw, cam_pitch) = self.vanilla_camera_angles();
                let drehung = affine_from_mat4(display_billboard_matrix(3, 0.0, 0.0, cam_yaw, cam_pitch));
                vec![Affine::translation(feet).then(Affine::scale([scale; 3])).then(drehung)]
            }
            world::ItemRenderKind::Dropped => {
                let (mut min_y, mut min_z, mut max_z) = (f32::MAX, f32::MAX, f32::MIN);
                for v in &lokal {
                    min_y = min_y.min(v.position[1]);
                    min_z = min_z.min(v.position[2]);
                    max_z = max_z.max(v.position[2]);
                }
                let versatz = item_unique_offset(id);
                let hoehe = -min_y + 0.0625 + (alter / 10.0 + versatz).sin() * 0.1 + 0.1;
                let basis = Affine::translation(feet)
                    .then(Affine::translation([0.0, hoehe, 0.0]))
                    .then(Affine::rotation_y_deg((alter / 20.0 + versatz).to_degrees()));
                dropped_item_copies(e.item_count, e.item_seed, max_z - min_z).into_iter().map(|p| basis.then(p)).collect()
            }
        };
        if umgeleitet {
            for p in &platzierungen {
                self.held_items.emit_ground(item.model_id, item.potion_color, *p, ENTITY_BRIGHTNESS, false, &mut Vec::new());
            }
            return;
        }
        let start = out.len();
        for p in &platzierungen {
            out.extend(lokal.iter().map(|v| world::BlockVertex { position: p.apply(v.position), ..*v }));
        }
        if item.enchanted {
            self.push_glint(out, start, render::glint::GLINT_SCALE_ITEM, render::glint::GLINT_TEXTURE_ITEM, |l| {
                let mut uv = Vec::new();
                self.held_items.emit_ground(item.model_id, item.potion_color, Affine::IDENTITY, 1.0, true, &mut uv);
                for p in &platzierungen {
                    l.extend(uv.iter().map(|v| world::BlockVertex { position: p.apply(v.position), ..*v }));
                }
            });
        }
    }
}
