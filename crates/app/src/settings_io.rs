// Settings lesen und schreiben.

use crate::*;

mod parse;
mod serialize;

impl Settings {
    // Pegel fuer einen Ton der Kategorie `c`: Hauptlautstaerke mal Kategorie, wie in Vanilla.
    // Die Oberflaeche haengt nur an ihrem eigenen Regler, ihre Klaenge stehen bewusst
    // ausserhalb der Hauptlautstaerke (s. master_volume).
    pub(crate) fn sound_gain(&self, c: protocol::packets::SoundCategory) -> f32 {
        let kategorie = VOLUME_CATEGORIES.iter().position(|v| v.category == c).map_or(1.0, |i| self.category_volume[i] / 100.0);
        if c == protocol::packets::SoundCategory::Ui {
            kategorie
        } else {
            kategorie * self.master_volume / 100.0
        }
    }

    pub(crate) fn light_factor_of(&self, name: &str) -> u32 {
        self.light_factors.iter().find(|(n, _)| n == name).map_or(100, |&(_, p)| p)
    }

    pub(crate) fn set_light_factor(&mut self, name: &str, pct: u32) {
        self.light_factors.retain(|(n, _)| n != name);
        if pct != 100 {
            self.light_factors.push((name.to_string(), pct));
            self.light_factors.sort();
        }
    }

    // Fuer world::light::set_light_factors.
    pub(crate) fn light_factor_table(&self) -> Vec<(String, f32)> {
        self.light_factors.iter().map(|(n, p)| (n.clone(), *p as f32 / 100.0)).collect()
    }

    // Saettigung fuer den Farb-Durchgang (s. GpuState::render_world): None, solange das Modul
    // aus ist oder nichts veraendert -- dann bleibt es bei der blossen Kopie.
    pub(crate) fn color_saturation(&self) -> Option<f32> {
        (self.module_color && self.color_saturation != 100).then(|| self.color_saturation as f32 / 100.0)
    }

    // Der allgemeine Textrand aus Schalter und Art.
    pub(crate) const fn global_text_shadow(&self) -> TextShadow {
        match (self.text_shadow, self.text_shadow_style) {
            (false, _) => TextShadow::Off,
            (true, 1) => TextShadow::Vanilla,
            (true, 2) => TextShadow::Below,
            (true, 3) => TextShadow::Around,
            _ => TextShadow::Outline,
        }
    }

    // Rand eines Bereichs; None = allgemeine Einstellung.
    pub(crate) fn font_of(&self, area: ShadowArea) -> Option<FontKind> {
        area_font_kind(self.area_font[area.index()])
    }

    // Die Schrift, in der ein Bereich tatsaechlich steht: seine eigene oder die allgemeine.
    pub(crate) fn effective_font(&self, area: ShadowArea) -> FontKind {
        self.font_of(area).unwrap_or(FontKind::from_index(self.font))
    }

    pub(crate) const fn shadow_of(&self, area: ShadowArea) -> Option<TextShadow> {
        area_shadow_style(self.area_shadow[area.index()])
    }

    // Art im Schatten-Kaestchen `i` (0 = Standard, 1 + Bereich); None = leer.
    pub(crate) fn shadow_slot_style(&self, i: usize) -> Option<TextShadow> {
        match i {
            0 => Some(self.global_text_shadow()),
            i => ShadowArea::ALL.get(i - 1).and_then(|&a| self.shadow_of(a)),
        }
    }

    // Was der Regler neben Kaestchen `i` fuer diese Art bedeutet: bei der Kontur die Dicke in
    // Bildschirmpixeln, sonst die Deckkraft (0..1).
    // "Unten" und "Aussen" sind weich und blass: dort reicht der Regler bis zum 2,5-Fachen,
    // 40 % sind die fruehere volle Staerke. Vanilla ist bei 100 % voll deckend.
    pub(crate) fn shadow_strength(&self, i: usize, stil: TextShadow) -> f32 {
        let deckkraft = f32::from(self.shadow_opacity.get(i).copied().unwrap_or(100));
        match stil {
            TextShadow::Outline => f32::from(self.shadow_thickness.get(i).copied().unwrap_or(SHADOW_THICKNESS_DEFAULT)) / 10.0,
            TextShadow::Below | TextShadow::Around => deckkraft / SOFT_SHADOW_FULL_PERCENT,
            _ => deckkraft / 100.0,
        }
    }

    // Rand und Staerke eines Bereichs fuer push_shadow_marker; None = wie der Standard.
    pub(crate) fn shadow_marker_of(&self, area: ShadowArea) -> Option<ShadowRun> {
        self.shadow_of(area).map(|s| (s, self.shadow_strength(1 + area.index(), s), self.shadow_half[1 + area.index()]))
    }

    pub(crate) const fn set_global_text_shadow(&mut self, stil: TextShadow) {
        (self.text_shadow, self.text_shadow_style) = match stil {
            TextShadow::Off => (false, self.text_shadow_style),
            TextShadow::Outline => (true, 0),
            TextShadow::Vanilla => (true, 1),
            TextShadow::Below => (true, 2),
            TextShadow::Around => (true, 3),
        };
    }

    // Was in einem Kaestchen des Schrift-Tabs steht; None = leer (wie der Standard).
    pub(crate) fn font_slot(&self, slot: FontSlot) -> Option<FontTile> {
        match slot {
            FontSlot::DefaultFont => Some(FontTile::Font(FontKind::from_index(self.font))),
            FontSlot::DefaultShadow => Some(FontTile::Shadow(self.global_text_shadow())),
            FontSlot::AreaFont(a) => self.font_of(a).map(FontTile::Font),
            FontSlot::AreaShadow(a) => self.shadow_of(a).map(FontTile::Shadow),
        }
    }

    // Ein Regler der Schadens-Faerbung -- eine Stelle fuer Stepper, Zeichnen und Verstellen.
    pub(crate) const fn damage_tint_of(&self, axis: DamageTintAxis) -> u32 {
        match axis {
            DamageTintAxis::Hue => self.damage_tint_hue,
            DamageTintAxis::Saturation => self.damage_tint_saturation,
            DamageTintAxis::Value => self.damage_tint_value,
            DamageTintAxis::Alpha => self.damage_tint_alpha,
        }
    }

    pub(crate) const fn damage_tint_mut(&mut self, axis: DamageTintAxis) -> &mut u32 {
        match axis {
            DamageTintAxis::Hue => &mut self.damage_tint_hue,
            DamageTintAxis::Saturation => &mut self.damage_tint_saturation,
            DamageTintAxis::Value => &mut self.damage_tint_value,
            DamageTintAxis::Alpha => &mut self.damage_tint_alpha,
        }
    }

    // Die fertige Schadens-Faerbung fuer den Shader: rgb + ANTEIL (0 = unveraendert,
    // 1 = voll durchgefaerbt). Der Anteil ist Vanillas overlayColor.a bereits umgedreht,
    // s. block.wgsl::fs_entity.
    pub(crate) fn damage_tint_rgba(&self) -> [f32; 4] {
        let rgb =
            hsv_to_rgb(self.damage_tint_hue as f32, self.damage_tint_saturation as f32 / 100.0, self.damage_tint_value as f32 / 100.0);
        [rgb[0], rgb[1], rgb[2], self.damage_tint_alpha as f32 / 100.0]
    }

    pub(crate) const fn crit_color_of(&self, axis: DamageTintAxis) -> u32 {
        match axis {
            DamageTintAxis::Hue => self.module_crit_color_hue,
            DamageTintAxis::Saturation => self.module_crit_color_saturation,
            DamageTintAxis::Value => self.module_crit_color_value,
            DamageTintAxis::Alpha => self.module_crit_color_alpha,
        }
    }

    pub(crate) const fn crit_color_mut(&mut self, axis: DamageTintAxis) -> &mut u32 {
        match axis {
            DamageTintAxis::Hue => &mut self.module_crit_color_hue,
            DamageTintAxis::Saturation => &mut self.module_crit_color_saturation,
            DamageTintAxis::Value => &mut self.module_crit_color_value,
            DamageTintAxis::Alpha => &mut self.module_crit_color_alpha,
        }
    }

    pub(crate) const fn block_outline_of(&self, axis: DamageTintAxis) -> u32 {
        match axis {
            DamageTintAxis::Hue => self.block_outline_hue,
            DamageTintAxis::Saturation => self.block_outline_saturation,
            DamageTintAxis::Value => self.block_outline_value,
            DamageTintAxis::Alpha => self.block_outline_alpha,
        }
    }

    pub(crate) const fn block_outline_mut(&mut self, axis: DamageTintAxis) -> &mut u32 {
        match axis {
            DamageTintAxis::Hue => &mut self.block_outline_hue,
            DamageTintAxis::Saturation => &mut self.block_outline_saturation,
            DamageTintAxis::Value => &mut self.block_outline_value,
            DamageTintAxis::Alpha => &mut self.block_outline_alpha,
        }
    }

    // Farbe i des Moduls Eigenname (0, 1 = Verlauf, 2 = Schatten), sRGB 0..1.
    pub(crate) fn ego_rgb(&self, i: usize) -> [f32; 3] {
        let c = self.ego_colors[i];
        hsv_to_rgb(c[0] as f32, c[1] as f32 / 100.0, c[2] as f32 / 100.0)
    }

    // Farbe der Stueckzahl fuers Farbfeld: dieselbe wie die Zahl (die UI ist linear).
    pub(crate) fn stack_count_swatch(&self, hotbar: bool) -> [f32; 4] {
        self.stack_count_rgba(hotbar)
    }

    // Farbe der Stueckzahl (s. stack_count_colors), linear wie UiText::color.
    pub(crate) fn stack_count_rgba(&self, hotbar: bool) -> [f32; 4] {
        let c = self.stack_count_colors[usize::from(!hotbar)];
        let [r, g, b] = hsv_to_rgb(c[0] as f32, c[1] as f32 / 100.0, c[2] as f32 / 100.0).map(world::sky::srgb_to_linear);
        [r, g, b, 1.0]
    }

    pub(crate) fn saturation_hud_rgb(&self) -> [f32; 3] {
        hsv_to_rgb(self.saturation_hud_hue as f32, self.saturation_hud_saturation as f32 / 100.0, self.saturation_hud_value as f32 / 100.0)
    }

    // Farbe und Deckkraft des eigenen Blockumrisses, in derselben Form wie crit_color_rgba.
    pub(crate) fn block_outline_rgba(&self) -> [f32; 4] {
        let rgb = hsv_to_rgb(
            self.block_outline_hue as f32,
            self.block_outline_saturation as f32 / 100.0,
            self.block_outline_value as f32 / 100.0,
        );
        [rgb[0], rgb[1], rgb[2], self.block_outline_alpha as f32 / 100.0]
    }

    pub(crate) const fn sky_color_of(&self, axis: SkyColorAxis) -> u32 {
        match axis {
            SkyColorAxis::Hue => self.module_sky_color_hue,
            SkyColorAxis::Saturation => self.module_sky_color_saturation,
            SkyColorAxis::Value => self.module_sky_color_value,
        }
    }

    pub(crate) const fn sky_color_mut(&mut self, axis: SkyColorAxis) -> &mut u32 {
        match axis {
            SkyColorAxis::Hue => &mut self.module_sky_color_hue,
            SkyColorAxis::Saturation => &mut self.module_sky_color_saturation,
            SkyColorAxis::Value => &mut self.module_sky_color_value,
        }
    }

    // Die eingestellte Himmelsfarbe im ANZEIGE-Raum (sRGB) -- umgerechnet wird erst kurz vor
    // dem Zeichnen (s. App::sky_clear_color).
    pub(crate) fn sky_color_rgb(&self) -> [f32; 3] {
        hsv_to_rgb(
            self.module_sky_color_hue as f32,
            self.module_sky_color_saturation as f32 / 100.0,
            self.module_sky_color_value as f32 / 100.0,
        )
    }

    // Ein Regler der Schild-Farben. Die Cooldown-Farbe hat keine Deckkraft (sie faerbt Linien).
    pub(crate) const fn shield_color_mut(&mut self, farbe: ShieldColor, achse: DamageTintAxis) -> Option<&mut u32> {
        Some(match (farbe, achse) {
            (ShieldColor::Cooldown, DamageTintAxis::Hue) => &mut self.shield_cd_hue,
            (ShieldColor::Cooldown, DamageTintAxis::Saturation) => &mut self.shield_cd_saturation,
            (ShieldColor::Cooldown, DamageTintAxis::Value) => &mut self.shield_cd_value,
            (ShieldColor::Cooldown, DamageTintAxis::Alpha) => &mut self.shield_cd_alpha,
            (ShieldColor::Hold, DamageTintAxis::Hue) => &mut self.shield_hold_hue,
            (ShieldColor::Hold, DamageTintAxis::Saturation) => &mut self.shield_hold_saturation,
            (ShieldColor::Hold, DamageTintAxis::Value) => &mut self.shield_hold_value,
            (ShieldColor::Hold, DamageTintAxis::Alpha) => &mut self.shield_hold_alpha,
        })
    }

    // Ohne "X/Y getrennt": den einen Wert auf den anderen uebertragen (`von_y` = Y gilt).
    pub(crate) const fn couple_sensitivity(&mut self, von_y: bool) {
        if self.mouse_sens_split {
            return;
        }
        if von_y {
            self.mouse_sensitivity = self.mouse_sensitivity_y;
        } else {
            self.mouse_sensitivity_y = self.mouse_sensitivity;
        }
    }

    // Feste Kreuzfarbe (Modus Einfarbig).
    pub(crate) fn fire_crosshair_rgb(&self) -> [f32; 3] {
        hsv_to_rgb(self.fire_crosshair_hue as f32, self.fire_crosshair_saturation as f32 / 100.0, self.fire_crosshair_value as f32 / 100.0)
    }

    pub(crate) fn fire_crosshair_rgba(&self) -> [f32; 4] {
        let [r, g, b] = self.fire_crosshair_rgb();
        [r, g, b, 1.0]
    }

    pub(crate) fn crosshair_rgb(&self) -> [f32; 3] {
        hsv_to_rgb(self.crosshair_hue as f32, self.crosshair_saturation as f32 / 100.0, self.crosshair_value as f32 / 100.0)
    }

    // Groesse der gewaehlten eingebauten Fadenkreuz-Form in Pixeln.
    pub(crate) fn crosshair_size(&self) -> u32 {
        self.crosshair_sizes[self.crosshair_shape.min(CROSSHAIR_SHAPES.len() - 1)]
    }

    // Eintrag der Formauswahl: die eingebaute Form oder "Eigene" (CROSSHAIR_SHAPE_CUSTOM).
    pub(crate) const fn crosshair_shape_choice(&self) -> usize {
        if self.crosshair_custom {
            CROSSHAIR_SHAPE_CUSTOM
        } else {
            self.crosshair_shape
        }
    }

    // Eintrag der Formauswahl setzen (geklemmt); "Eigene" faengt beim eingebauten Kreuz an,
    // nicht bei nichts, wenn noch nie gemalt wurde.
    pub(crate) fn set_crosshair_shape_choice(&mut self, wahl: usize) {
        let neu = wahl.min(CROSSHAIR_SHAPE_CUSTOM);
        if neu == CROSSHAIR_SHAPE_CUSTOM {
            self.use_custom_crosshair();
        } else {
            self.crosshair_custom = false;
            self.crosshair_shape = neu;
        }
    }

    pub(crate) fn use_custom_crosshair(&mut self) {
        if !self.crosshair_custom && self.crosshair_pattern == crosshair_editor::Pattern::standard(false) {
            self.crosshair_pattern = crosshair_editor::Pattern::standard(self.crosshair_outline);
        }
        self.crosshair_custom = true;
    }

    pub(crate) fn step_crosshair_size(&mut self, dir: i32) {
        let i = self.crosshair_shape.min(CROSSHAIR_SHAPES.len() - 1);
        let (_, min, max, _) = CROSSHAIR_SHAPES[i];
        self.crosshair_sizes[i] = (self.crosshair_sizes[i] as i32 + dir).clamp(min as i32, max as i32) as u32;
    }

    pub(crate) const fn shield_color_of(&self, farbe: ShieldColor, achse: DamageTintAxis) -> u32 {
        match (farbe, achse) {
            (ShieldColor::Cooldown, DamageTintAxis::Hue) => self.shield_cd_hue,
            (ShieldColor::Cooldown, DamageTintAxis::Saturation) => self.shield_cd_saturation,
            (ShieldColor::Cooldown, DamageTintAxis::Value) => self.shield_cd_value,
            (ShieldColor::Cooldown, DamageTintAxis::Alpha) => self.shield_cd_alpha,
            (ShieldColor::Hold, DamageTintAxis::Hue) => self.shield_hold_hue,
            (ShieldColor::Hold, DamageTintAxis::Saturation) => self.shield_hold_saturation,
            (ShieldColor::Hold, DamageTintAxis::Value) => self.shield_hold_value,
            (ShieldColor::Hold, DamageTintAxis::Alpha) => self.shield_hold_alpha,
        }
    }

    pub(crate) const fn shield_mark(&self, farbe: ShieldColor) -> ShieldMark {
        ShieldMark::from_index(match farbe {
            ShieldColor::Cooldown => self.shield_cd_mark,
            ShieldColor::Hold => self.shield_hold_mark,
        })
    }

    // Farbe als RGBA (Deckkraft 0..1). Als Hitbox-Farbe zaehlt die Deckkraft nicht.
    pub(crate) fn shield_color_rgba(&self, farbe: ShieldColor) -> [f32; 4] {
        let rgb = hsv_to_rgb(
            self.shield_color_of(farbe, DamageTintAxis::Hue) as f32,
            self.shield_color_of(farbe, DamageTintAxis::Saturation) as f32 / 100.0,
            self.shield_color_of(farbe, DamageTintAxis::Value) as f32 / 100.0,
        );
        [rgb[0], rgb[1], rgb[2], self.shield_color_of(farbe, DamageTintAxis::Alpha) as f32 / 100.0]
    }

    // Die Faerbung fuer einen kritischen Treffer, in derselben Form wie damage_tint_rgba.
    pub(crate) fn crit_color_rgba(&self) -> [f32; 4] {
        let rgb = hsv_to_rgb(
            self.module_crit_color_hue as f32,
            self.module_crit_color_saturation as f32 / 100.0,
            self.module_crit_color_value as f32 / 100.0,
        );
        [rgb[0], rgb[1], rgb[2], self.module_crit_color_alpha as f32 / 100.0]
    }

    // Sichtfeld der jeweiligen Perspektive -- eine Stelle statt eines match an jedem
    // Aufrufer (Kamera, Stepper, Wertfeld, Zuruecksetzen).
    pub(crate) const fn perspective_skipped(&self, p: Perspective) -> bool {
        match p {
            Perspective::FirstPerson => false,
            Perspective::ThirdPersonBack => self.skip_third_back,
            Perspective::ThirdPersonFront => self.skip_third_front,
        }
    }

    // Die 3.-Person-Ansicht, in die Quicklook schaut: die nicht uebersprungene, wenn genau
    // eine uebersprungen ist. Sonst kein Quicklook (und kein H/T an der Taste).
    pub(crate) const fn quicklook_target(&self) -> Option<Perspective> {
        match (self.skip_third_back, self.skip_third_front) {
            (true, false) => Some(Perspective::ThirdPersonFront),
            (false, true) => Some(Perspective::ThirdPersonBack),
            _ => None,
        }
    }

    pub(crate) const fn fov_of(&self, p: Perspective) -> f32 {
        match p {
            Perspective::FirstPerson => self.fov,
            Perspective::ThirdPersonBack => self.fov_third_back,
            Perspective::ThirdPersonFront => self.fov_third_front,
        }
    }

    // Kameraabstand der Perspektive. Die Ego-Ansicht liefert 0 -- sie hat keinen Abstand,
    // und 0 ist genau das, was update_perspective_offset dort ohnehin einsetzt.
    pub(crate) const fn camera_distance_of(&self, p: Perspective) -> f32 {
        match p {
            Perspective::FirstPerson => 0.0,
            Perspective::ThirdPersonBack => self.camera_distance_back,
            Perspective::ThirdPersonFront => self.camera_distance_front,
        }
    }

    pub(crate) const fn set_camera_distance_of(&mut self, p: Perspective, value: f32) {
        let clamped = value.clamp(CAMERA_DISTANCE_MIN, CAMERA_DISTANCE_MAX);
        match p {
            Perspective::FirstPerson => {}
            Perspective::ThirdPersonBack => self.camera_distance_back = clamped,
            Perspective::ThirdPersonFront => self.camera_distance_front = clamped,
        }
    }

    // Hoehenversatz der Perspektive. Die Ego-Ansicht liefert 0 -- ihre Kamera sitzt im Auge.
    pub(crate) const fn camera_y_offset_of(&self, p: Perspective) -> f32 {
        match p {
            Perspective::FirstPerson => 0.0,
            Perspective::ThirdPersonBack => self.camera_y_offset_back,
            Perspective::ThirdPersonFront => self.camera_y_offset_front,
        }
    }

    pub(crate) const fn set_camera_y_offset_of(&mut self, p: Perspective, value: f32) {
        let clamped = value.clamp(CAMERA_Y_OFFSET_MIN, CAMERA_Y_OFFSET_MAX);
        match p {
            Perspective::FirstPerson => {}
            Perspective::ThirdPersonBack => self.camera_y_offset_back = clamped,
            Perspective::ThirdPersonFront => self.camera_y_offset_front = clamped,
        }
    }

    pub(crate) const fn set_fov_of(&mut self, p: Perspective, value: f32) {
        let clamped = value.clamp(FOV_MIN, FOV_MAX);
        match p {
            Perspective::FirstPerson => self.fov = clamped,
            Perspective::ThirdPersonBack => self.fov_third_back = clamped,
            Perspective::ThirdPersonFront => self.fov_third_front = clamped,
        }
    }
}

// Nur bei einem bekannten Tastennamen ueberschreiben -- eine von Hand kaputt editierte
// Zeile ("keybind_forward=Foo") soll auf dem Default bleiben statt den Client zu einer
// unbelegten Aktion zu bringen (s. Kommentar bei Settings::parse zum generellen Prinzip).
// Alle Eingaben einer Aktion als eine Zeile fuer settings.conf -- kommagetrennt. Weder
// Tasten- noch Maustastennamen enthalten ein Komma, der Trenner ist also eindeutig.
pub(crate) fn binds_to_str(keybinds: &Keybinds, action: KeyBindAction) -> String {
    keybinds.binds(action).map(bind_name).collect::<Vec<_>>().join(",")
}

// Gegenrichtung. Eine Datei aus der Zeit vor den Mehrfachbelegungen enthaelt genau einen
// Namen ohne Komma -- die laeuft hier unveraendert durch und landet in Slot 0. Ein leerer
// Wert heisst "diese Aktion ist unbelegt" (das gibt es jetzt, s. remove_at) und ueberschreibt
// den Standard deshalb bewusst.
pub(crate) fn set_keybind_from_str(keybinds: &mut Keybinds, action: KeyBindAction, value: &str) {
    let parsed: Vec<Bind> = value.split(',').map(str::trim).filter(|n| !n.is_empty()).filter_map(bind_from_name).collect();
    if parsed.is_empty() && !value.trim().is_empty() {
        return; // unlesbarer Wert -> Standard stehen lassen (wie bisher)
    }
    for slot in 0..MAX_BINDS_PER_ACTION {
        keybinds.slots[action.index()][slot] = parsed.get(slot).copied();
    }
}

pub(crate) fn set_keybind_mode_from_str(modes: &mut KeybindModes, action: KeyBindAction, value: &str) {
    if let Some(mode) = keybind_mode_from_str(value) {
        modes.set(action, mode);
    }
}

// Standardwerte fuer neue Installationen, Schluessel wie in settings.conf (s. Default).
pub(crate) const DEFAULT_SETTINGS_CONF: &str = include_str!("default-settings.conf");
