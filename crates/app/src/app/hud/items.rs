// Items mit Anzahl und Haltbarkeit zeichnen.

use super::*;

impl App {
    pub(crate) fn push_item(&self, sprites: &mut Vec<UiSprite>, texts: &mut Vec<UiText>, stack: &protocol::item::ItemStack, x: f32, y: f32, size: f32) {
        self.push_item_limited(sprites, texts, stack, x, y, size, DurabilityBarLimit::Always, 0.0);
        // Verstaerkter (S) oder verlaengerter (L) Trank -- die Farbe allein unterscheidet die
        // drei Fassungen nicht. Nur hier, nicht in der Hotbar (die zeichnet ueber
        // push_item_limited). Halb so gross wie die Anzahl, auf ganze Pixel gerundet, damit die
        // Schrift scharf bleibt. Direkt links neben dem Flaschenhals: rechte Kante bei Pixel 6,
        // Unterkante bei 6,5 des 16er-Bilds -- Trink- und Wurftrank haben dort beide frei.
        let zeigen = |c: char| match c {
            'S' => self.settings.potion_mark_strong,
            'L' => self.settings.potion_mark_long,
            'W' => self.settings.potion_mark_weak,
            _ => true,
        };
        if let Some(c) = world::potion::variant_mark(stack).filter(|&c| zeigen(c)) {
            let scale = (size / 32.0).round().max(1.0);
            let text = c.to_string();
            let px = size / 16.0;
            let breite = self.gpu.as_ref().map_or(0.0, |g| g.measure_text(&text, scale));
            let hoehe = self.gpu.as_ref().map_or(0.0, |g| g.text_height(scale));
            let (tx, ty) = ((x + 6.0 * px - breite).round(), (y + 6.5 * px - hoehe).round());
            texts.push(UiText { text, x: tx, y: ty, color: [1.0, 1.0, 1.0, 1.0], scale });
        }
    }

    // Wie push_item, aber der Haltbarkeitsbalken erscheint erst, wenn hoechstens der Anteil
    // Grenze `limit` erreicht ist (Hotbar-Einstellung "Haltbarkeit ab").
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn push_item_limited(
        &self,
        sprites: &mut Vec<UiSprite>,
        texts: &mut Vec<UiText>,
        stack: &protocol::item::ItemStack,
        x: f32,
        y: f32,
        size: f32,
        limit: DurabilityBarLimit,
        pop: f32,
    ) {
        let Some(name) = world::item_registry::item_name(stack.item_id) else {
            return;
        };
        let erstes = sprites.len();
        self.push_item_icon_only(sprites, stack, x, y, size);
        // Aufnahme-Animation: nur das Bild, um (8, 12) des Slots schmaler und hoeher, wie
        // Vanillas Gui.renderSlot. Anzahl und Balken bleiben stehen.
        if pop > 0.0 {
            let g = 1.0 + pop / 5.0;
            let (sx, sy) = (1.0 / g, f32::midpoint(g, 1.0));
            let (mx, my) = (x + size * 0.5, y + size * 0.75);
            for sprite in &mut sprites[erstes..] {
                for c in &mut sprite.corners {
                    *c = [mx + (c[0] - mx) * sx, my + (c[1] - my) * sy];
                }
            }
        }
        // DrawContext.drawItemBar: Vanilla legt den Balken (13x1 bei (2, 13)) auf einen
        // schwarzen 13x2 -- unten ein Schatten. Hier stattdessen eine Kontur: ein
        // Bildschirmpixel Schwarz rundherum (gewuenscht, wie die Text-Kontur).
        let balken = item_bar(stack, name).filter(|_| match limit {
            DurabilityBarLimit::Always => true,
            DurabilityBarLimit::Rest(r) => r >= 1.0 || durability_rest(stack, name).is_some_and(|rest| rest <= r + 1e-4),
            DurabilityBarLimit::Points(p) => durability_points(stack, name).is_some_and(|rest| rest <= p),
        });
        if let (Some((schritt, farbe)), Some(weiss)) = (balken, self.ui_atlas.rect(assets::UI_WHITE)) {
            let px = size / 16.0;
            let uv = weiss.sub(1, 1, 2, 2);
            sprites.push(UiSprite::quad(x + 2.0 * px - 1.0, y + 13.0 * px - 1.0, 13.0 * px + 2.0, px + 2.0, uv, [0.0, 0.0, 0.0, 1.0], SpriteAtlas::Ui));
            let [r, g, b] = farbe.map(world::sky::srgb_to_linear);
            sprites.push(UiSprite::quad(x + 2.0 * px, y + 13.0 * px, schritt * px, px, uv, [r, g, b, 1.0], SpriteAtlas::Ui));
        }
        // DrawContext.drawCooldownProgress: weisser Schleier (0x7FFFFFFF) von unten, so hoch
        // wie der Rest der Abklingzeit. Vanilla rechnet in 16 GUI-Pixeln und rundet dort --
        // hier dieselbe Rundung auf dem 16er-Raster des Slots.
        if !self.item_cooldowns.is_empty() {
            let f = self.item_cooldowns.progress(stack, self.tick_progress());
            if f > 0.0 {
                if let Some(weiss) = self.ui_atlas.rect(assets::UI_WHITE) {
                    let px = size / 16.0;
                    let oben = y + (16.0 * (1.0 - f)).floor() * px;
                    let hoehe = (16.0 * f).ceil() * px;
                    sprites.push(UiSprite::quad(x, oben, size, hoehe, weiss.sub(1, 1, 2, 2), [1.0, 1.0, 1.0, 127.0 / 255.0], SpriteAtlas::Ui));
                }
            }
        }
        if stack.count > 1 {
            // Nur die Hotbar setzt eine Grenze fuer den Balken (s. build_hotbar).
            let hotbar = !matches!(limit, DurabilityBarLimit::Always);
            self.push_count_label_in(texts, &stack.count.to_string(), x, y, size, self.settings.stack_count_rgba(hotbar));
        }
    }

    // Nur das Bild eines Items (mit Glint), ohne Balken, Abklingzeit und Anzahl -- so zeichnet
    // Vanilla auch das im Buendel gewaehlte Item ins offene Buendel.
    pub(crate) fn push_item_icon_only(&self, sprites: &mut Vec<UiSprite>, stack: &protocol::item::ItemStack, x: f32, y: f32, size: f32) {
        let Some(name) = world::item_registry::item_name(stack.item_id) else {
            return;
        };
        let erstes = sprites.len();
        // item_model-Komponente: der Server will das Item mit einem anderen Modell zeigen --
        // soweit wir fuer dieses Modell ein Icon haben (Modelle aus Server-Resourcepacks nicht).
        let modell = stack.item_model.as_deref().map(|m| m.strip_prefix("minecraft:").unwrap_or(m)).filter(|m| self.item_icons.has(m));
        let banner = (name == "shield").then(|| world::entity::ShieldBanner::from_stack(stack)).flatten();
        // Zeigt item_model auf einen Kopf (Golden Head: Honigflasche als player_head), wird er
        // wie ein echter Kopf mit dem Skin aus der profile-Komponente gezeichnet.
        // Die gui-Haltung kommt vom KOPF-Modell, nicht vom Item: sonst stand der Golden Head
        // mit der Haltung der Honigflasche (flach, ungedreht) im Slot.
        let kopf = world::item_registry::item_name(world::entity::model_item_id(stack)).and_then(|m| self.skull_texture(m, stack).map(|t| (m, t)));
        if let Some((kopf_modell, (tex, hut))) = kopf {
            self.item_icons.push_skull(sprites, kopf_modell, tex, hut, x, y, size, 1.0);
        } else if let (None, Some(banner)) = (modell, banner) {
            self.push_shield_banner_icon(sprites, banner, x, y, size);
        } else {
            let tint = world::potion::stack_tint(stack);
            // Die Definition des Modells je Stapel auswerten (Verzauberung, custom_model_data,
            // Ladung ...). Das Schild bleibt beim Entity-Modell unter seinem Namen.
            let id = model_definition_id(stack, name);
            let input = self.item_model_input(stack, "gui");
            match self.item_icons.stack_keys(&id, &input).filter(|_| !self.item_icons.has_entity_model(&id)) {
                Some(keys) => {
                    for key in keys {
                        match key {
                            Some(key) => self.item_icons.push(sprites, key, tint, x, y, size, 1.0),
                            // Das im Buendel gewaehlte Item zwischen Rueck- und Vorderseite.
                            None => {
                                if let Some(gewaehlt) = usize::try_from(stack.bundle_selected).ok().and_then(|i| world::bundle::contents(stack).get(i)) {
                                    self.push_item_icon_only(sprites, gewaehlt, x, y, size);
                                }
                            }
                        }
                    }
                }
                None => self.item_icons.push(sprites, modell.unwrap_or_else(|| item_icon_name(name, stack)), tint, x, y, size, 1.0),
            }
        }
        // Glint: jedes Quad des Icons (auch jede Wuerfelseite eines Blockitems) noch einmal,
        // direkt dahinter, als Glint-Zwilling (s. shape_sprites und sprite.wgsl::fs_glint).
        if world::enchantment::has_glint(stack) {
            for i in erstes..sprites.len() {
                let icon = sprites[i];
                if let Some(atlas) = icon.atlas.glint() {
                    sprites.push(UiSprite { atlas, ..icon });
                }
            }
        }
    }

    // Zahl rechts unten in den Slot, wie in Vanilla. Skalierung an die Slotgroesse
    // gekoppelt, damit sie bei jeder GUI-Skalierung sitzt.
    // Ganze Stufe: die Zahl im Slot soll scharf bleiben (s. UiText::scale). Einen halben
    // Slot-Pixel nach rechts unten, wie in Vanilla: die Ziffern stehen so leicht ueber dem Rand
    // des Items und heben sich besser ab.
    pub(crate) fn push_count_label(&self, texts: &mut Vec<UiText>, text: &str, x: f32, y: f32, size: f32) {
        self.push_count_label_in(texts, text, x, y, size, self.settings.stack_count_rgba(false));
    }

    // Dasselbe in der Farbe `color` (s. Settings::stack_count_colors).
    pub(crate) fn push_count_label_in(&self, texts: &mut Vec<UiText>, text: &str, x: f32, y: f32, size: f32, color: [f32; 4]) {
        let scale = (size / 16.0).round().max(1.0);
        let width = self.gpu.as_ref().map_or(0.0, |g| g.measure_text(text, scale));
        let height = self.gpu.as_ref().map_or(0.0, |g| g.text_height(scale));
        let halb = size / 32.0;
        texts.push(UiText { text: text.to_string(), x: (x + size - width + halb).round(), y: (y + size - height + halb).round(), color, scale });
    }
}
