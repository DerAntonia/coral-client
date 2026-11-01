// Ruestungs-HUD und Pfeil-Anzeige.

use super::*;

impl App {
    // Ruestungs-HUD im Spiel: nur mit Verbindung, nicht als Zuschauer und nur, wenn man
    // ueberhaupt etwas traegt.
    pub(crate) fn build_armor_hud(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, sprites: &mut Vec<UiSprite>, texts: &mut Vec<UiText>) {
        if !self.settings.module_armor_hud || self.conn_status != ConnStatus::Connected || self.game_mode == GAME_MODE_SPECTATOR {
            return;
        }
        let getragen = self.worn_armor();
        let teile: [Option<&protocol::item::ItemStack>; 4] = std::array::from_fn(|i| getragen[i].or_else(|| self.taken_off_armor(i)));
        if teile.iter().all(Option::is_none) {
            return;
        }
        self.push_armor_hud(gpu, teile, getragen.map(|t| t.is_none()), rects, sprites, texts);
    }

    // Haltbarkeit aller getragenen Teile mit Mending zusammen, 0..1: Summe der Reste durch
    // Summe der Hoechstwerte (750 von 1000 Punkten = 0.75). None ohne solches Teil.
    pub(crate) fn mending_armor_fraction(&self) -> Option<f32> {
        let (mut rest, mut max) = (0i64, 0i64);
        for stack in self.worn_armor().into_iter().flatten() {
            if !self.enchantments.has(stack, "minecraft:mending") {
                continue;
            }
            let Some(name) = world::item_registry::item_name(stack.item_id) else { continue };
            let (Some(r), Some(m)) = (durability_points(stack, name), max_damage_of(stack, name)) else { continue };
            rest += i64::from(r);
            max += i64::from(m);
        }
        (max > 0).then(|| (rest as f32 / max as f32).clamp(0.0, 1.0))
    }

    // Einmal je Tick: das getragene Teil je Slot merken (s. armor_last_worn).
    pub(crate) fn remember_worn_armor(&mut self) {
        for i in 0..4 {
            let Some(stack) = self.worn_armor()[i] else { continue };
            if self.armor_last_worn[i].as_ref() != Some(stack) {
                self.armor_last_worn[i] = Some(stack.clone());
            }
        }
    }

    // Das zuletzt in Slot `i` getragene Teil, wenn es jetzt im Inventar liegt. Genau dieses
    // Teil (gleicher Schaden, gleiche Verzauberung ...), nicht irgendeins derselben Art.
    pub(crate) fn taken_off_armor(&self, i: usize) -> Option<&protocol::item::ItemStack> {
        let zuletzt = self.armor_last_worn[i].as_ref()?;
        self.inventory.player[inventory::PLAYER_MAIN_START..=inventory::PLAYER_OFFHAND].iter().flatten().find(|s| *s == zuletzt)
    }

    // Helm, Brust, Hose und Schuhe aus dem eigenen Inventar, None fuer leere Slots.
    pub(crate) fn worn_armor(&self) -> [Option<&protocol::item::ItemStack>; 4] {
        std::array::from_fn(|i| {
            self.inventory.player.get(inventory::PLAYER_ARMOR_START + i).and_then(|s| s.as_ref()).filter(|s| s.item_id != 0 && s.count > 0)
        })
    }

    // Masse des Ruestungs-HUD: Kantenlaenge eines Item-Bilds, ein Bildpixel, Textfaktor der Zahl,
    // Abstand zwischen zwei Bildern und der Rand, um den die Zahl ("000") ueber ein Bild
    // hinausragt (links, oben, rechts, unten). Gemessen in der Schrift des Bereichs
    // Ruestungs-HUD.
    pub(crate) fn armor_hud_metrics(&self, gpu: &GpuState) -> ArmorHudMetrics {
        let set = &self.settings;
        let _schrift = self.area_font_scope(gpu, ShadowArea::ArmorHud);
        let size = (16.0 * self.hud_scale(gpu.size()) * set.hud_armor_scale as f32 / 100.0).round().max(8.0);
        let px = (size / 16.0).round().max(1.0);
        let text_scale = (px * set.armor_hud_number_scale as f32 / 100.0).max(0.5);
        let text = (gpu.measure_text("000", text_scale), gpu.text_height(text_scale));
        let (nx, ny) = armor_hud::number_origin((0.0, 0.0), size, px, (set.armor_hud_number_x, set.armor_hud_number_y), text);
        let rand = [(-nx).max(0.0), (-ny).max(0.0), (nx + text.0 - size).max(0.0), (ny + text.1 - size).max(0.0)].map(f32::ceil);
        ArmorHudMetrics { size, px, text_scale, gap: set.armor_hud_gap as f32 * px, rand }
    }

    // Die vier Bilder samt Rand fuer die Zahl, ab der gespeicherten oberen linken Ecke -- gleich
    // gross, egal wie viele Teile man traegt, damit das Element beim An- und Ausziehen nicht
    // springt.
    pub(crate) fn hud_armor_rect(&self, gpu: &GpuState) -> Rect {
        let (sw, sh) = gpu.size();
        let m = self.armor_hud_metrics(gpu);
        let reihe = (4.0 * m.size + 3.0 * m.gap).max(m.size);
        let (w, h) = if armor_hud::is_vertical(self.settings.armor_hud_layout) { (m.size, reihe) } else { (reihe, m.size) };
        let (w, h) = (w + m.rand[0] + m.rand[2], h + m.rand[1] + m.rand[3]);
        let x = (self.settings.hud_armor_x * sw).clamp(0.0, (sw - w).max(0.0)).round();
        let y = (self.settings.hud_armor_y * sh).clamp(0.0, (sh - h).max(0.0)).round();
        Rect { x, y, w, h }
    }

    // Die Teile an ihren Plaetzen (s. armor_hud::cells), je mit der Haltbarkeit an der
    // eingestellten Stelle -- im Spiel und als Vorschau im HUD-Editor.
    // `ausgezogen`: das Teil liegt nur im Inventar -- sein Bild blass, die Zahl nicht. Solange
    // eins abgelegt ist, haben die noch getragenen einen gelben Rahmen.
    pub(crate) fn push_armor_hud(
        &self,
        gpu: &GpuState,
        teile: [Option<&protocol::item::ItemStack>; 4],
        ausgezogen: [bool; 4],
        rects: &mut Vec<UiRect>,
        sprites: &mut Vec<UiSprite>,
        texts: &mut Vec<UiText>,
    ) {
        let set = &self.settings;
        let rect = self.hud_armor_rect(gpu);
        let m = self.armor_hud_metrics(gpu);
        let schritt = m.size + m.gap;
        let hochkant = armor_hud::is_vertical(set.armor_hud_layout);
        let ans_ende = if hochkant { set.armor_hud_sort_vertical == 1 } else { set.armor_hud_sort_flat == 1 };
        let zellen = armor_hud::cells(teile.map(|t| t.is_some()), set.armor_hud_layout, ans_ende);
        let (x0, y0) = (rect.x + m.rand[0], rect.y + m.rand[1]);
        self.begin_text_area(gpu, texts, ShadowArea::ArmorHud);
        let text_h = gpu.text_height(m.text_scale);
        let etwas_abgelegt = teile.iter().zip(ausgezogen).any(|(t, aus)| t.is_some() && aus);
        for ((stack, zelle), ausgezogen) in teile.into_iter().zip(zellen).zip(ausgezogen) {
            let (Some(stack), Some(zelle)) = (stack, zelle) else { continue };
            let bild = if hochkant { (x0, y0 + zelle as f32 * schritt) } else { (x0 + zelle as f32 * schritt, y0) };
            if etwas_abgelegt && !ausgezogen {
                // Knapp ausserhalb des Bilds, ein Bildpixel breit.
                let r = Rect { x: bild.0.round() - m.px, y: bild.1.round() - m.px, w: m.size + 2.0 * m.px, h: m.size + 2.0 * m.px };
                rects.push(UiRect::framed(r, [0.0; 4], m.px, m.px, COLOR_ARMOR_KEPT));
            }
            let erstes = sprites.len();
            self.push_item_icon_only(sprites, stack, bild.0.round(), bild.1.round(), m.size);
            if ausgezogen {
                for s in &mut sprites[erstes..] {
                    s.color[3] *= 0.3;
                }
            }
            let Some(rest) = world::item_registry::item_name(stack.item_id).and_then(|name| durability_points(stack, name)) else { continue };
            let text = rest.to_string();
            let tw = gpu.measure_text(&text, m.text_scale);
            let (x, y) = armor_hud::number_origin(bild, m.size, m.px, (set.armor_hud_number_x, set.armor_hud_number_y), (tw, text_h));
            let max = world::item_registry::item_name(stack.item_id).and_then(|name| max_damage_of(stack, name)).unwrap_or(0);
            let color = if rest < armor_hud::LOW_DURABILITY {
                COLOR_ARMOR_LOW
            } else if set.armor_hud_green && max > 0 && rest as f32 > max as f32 * 0.87 {
                COLOR_ARMOR_HIGH
            } else {
                COLOR_WHITE
            };
            texts.push(UiText { text, x: x.round(), y: y.round(), color, scale: m.text_scale });
        }
        Self::end_text_area(gpu, texts);
    }

    // Wo die Pfeil-Anzeige sitzt: ein Quadrat um die gespeicherte Mitte, so gross wie ein
    // Hotbar-Slot mal der eigenen Groesse aus dem HUD-Editor.
    pub(crate) fn hud_arrows_rect(&self, gpu: &GpuState) -> Rect {
        let (sw, sh) = gpu.size();
        let size = (16.0 * self.hud_scale(gpu.size()) * self.settings.hud_arrows_scale as f32 / 100.0).round();
        let cx = self.settings.hud_arrows_x * sw;
        let cy = self.settings.hud_arrows_y * sh;
        Rect { x: (cx - size / 2.0).round(), y: (cy - size / 2.0).round(), w: size, h: size }
    }

    // Die Anzeige selbst -- im Spiel und als Vorschau im HUD-Editor. Mit Icon steht die Zahl
    // wie im Slot unten rechts (und fehlt bei genau einem Pfeil); ohne Icon steht sie immer da,
    // mittig im selben Quadrat.
    // `mit_zahl`: die Zahl als UI-Text mitzeichnen (Vorschau im HUD-Editor). Im Spiel kommt sie
    // aus dem Fadenkreuz-Pass (s. arrow_number).
    pub(crate) fn push_arrow_indicator(
        &self,
        gpu: &GpuState,
        stack: &protocol::item::ItemStack,
        sprites: &mut Vec<UiSprite>,
        texts: &mut Vec<UiText>,
        mit_zahl: bool,
    ) {
        let rect = self.hud_arrows_rect(gpu);
        let alpha = self.settings.arrow_opacity as f32 / 100.0;
        let scale = (rect.w / 16.0).round().max(1.0);
        let text = stack.count.to_string();
        let (w, h) = (gpu.measure_text(&text, scale), gpu.text_height(scale));
        if self.settings.arrow_number_only {
            if mit_zahl {
                let (x, y) = (rect.x + (rect.w - w) / 2.0, rect.y + (rect.h - h) / 2.0);
                texts.push(UiText { text, x: x.round(), y: y.round(), color: [1.0, 1.0, 1.0, alpha], scale });
            }
            return;
        }
        if let Some(name) = world::item_registry::item_name(stack.item_id) {
            self.item_icons.push(sprites, item_icon_name(name, stack), world::potion::stack_tint(stack), rect.x, rect.y, rect.w, alpha);
        }
        if stack.count > 1 && mit_zahl {
            texts.push(UiText { text, x: (rect.x + rect.w - w).round(), y: (rect.y + rect.h - h).round(), color: [1.0, 1.0, 1.0, alpha], scale });
        }
    }

    // Die Munition fuer die Waffe in der Hand (PlayerEntity.getProjectileType), mit der
    // Gesamtzahl gleicher Stapel als `count`. None ohne Bogen/Armbrust oder ohne Munition.
    pub(crate) fn arrow_hud_stack(&self) -> Option<protocol::item::ItemStack> {
        let ist_waffe = |hand: i32| {
            self.own_hand_stack(hand)
                .and_then(|s| world::item_registry::item_name(s.item_id))
                .filter(|n| matches!(*n, "bow" | "crossbow"))
        };
        // Vanilla fragt die Haupthand zuerst.
        let waffe = ist_waffe(protocol::packets::HAND_MAIN).or_else(|| ist_waffe(protocol::packets::HAND_OFF))?;
        self.projectile_for(waffe)
    }

    // PlayerEntity.getProjectileType fuer diese Waffe ("bow"/"crossbow"), ohne den
    // Kreativ-Ersatzpfeil: die gewaehlte Munition mit der Gesamtzahl gleicher Stapel als
    // `count`, None ohne Munition.
    pub(crate) fn projectile_for(&self, waffe: &str) -> Option<protocol::item::ItemStack> {
        let ist_pfeil = |s: &protocol::item::ItemStack| {
            matches!(world::item_registry::item_name(s.item_id), Some("arrow" | "spectral_arrow" | "tipped_arrow"))
        };
        // Die Armbrust nimmt eine Rakete nur aus einer HAND, im Inventar sucht sie nur Pfeile.
        let in_hand = |s: &protocol::item::ItemStack| {
            ist_pfeil(s) || (waffe == "crossbow" && world::item_registry::item_name(s.item_id) == Some("firework_rocket"))
        };
        let hand = |h: i32| self.own_hand_stack(h).filter(|s| in_hand(s)).cloned();
        let slot = |i: usize| self.inventory.player.get(i).and_then(|s| s.as_ref()).filter(|st| st.item_id != 0 && st.count > 0);
        let gewaehlt = hand(protocol::packets::HAND_OFF).or_else(|| hand(protocol::packets::HAND_MAIN)).or_else(|| {
            // Vanillas Inventar-Reihenfolge: Hotbar, dann die drei Reihen, dann die Zweithand.
            (0..9)
                .map(|i| inventory::PLAYER_HOTBAR_START + i)
                .chain((0..27).map(|i| inventory::PLAYER_MAIN_START + i))
                .chain(std::iter::once(inventory::PLAYER_OFFHAND))
                .filter_map(slot)
                .find(|s| ist_pfeil(s))
                .cloned()
        })?;
        let gesamt: i32 = (inventory::PLAYER_MAIN_START..=inventory::PLAYER_OFFHAND)
            .filter_map(slot)
            .filter(|s| s.item_id == gewaehlt.item_id && s.potion == gewaehlt.potion)
            .map(|s| s.count)
            .sum();
        Some(protocol::item::ItemStack { count: gesamt, ..gewaehlt })
    }

    // Vanillas popTime minus Tick-Anteil: 5 direkt nach der Aufnahme, nach fuenf Ticks 0.
    pub(crate) fn hotbar_pop_amount(&self, index: usize) -> f32 {
        self.hotbar_pop.get(index).copied().flatten().map_or(0.0, |t| (5.0 - t.elapsed().as_secs_f32() * 20.0).max(0.0))
    }
}
