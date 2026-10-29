// Client-seitiger Inventarzustand plus die Vanilla-Bildschirmaufteilung dafuer.
//
// Der Server behaelt das letzte Wort: was er schickt, ueberschreibt hier immer alles. Fuer
// die einfachen Klicks (Aufnehmen/Ablegen auf gewoehnlichen Lagerslots) rechnet der Client
// das Ergebnis aber selbst voraus -- einmal, damit die Anzeige sofort reagiert, und vor
// allem, weil das Klick-Paket seit 1.21.5 genau diese Vorhersage als Prüfsumme mitschickt
// (s. Abschnitt "Klick-Vorhersage" weiter unten und packets::click_container_payload).
//
// Slot-Nummerierung ist die des Vanilla-ScreenHandlers (nicht die der PlayerInventory!):
// beim Spielerinventar (sync_id 0) sind das 46 Slots, s. PLAYER_*-Konstanten.

use protocol::item::ItemStack;

// Spielerinventar als ScreenHandler 0.
pub const PLAYER_SLOT_COUNT: usize = 46;
pub const PLAYER_CRAFT_RESULT: usize = 0;
pub const PLAYER_ARMOR_START: usize = 5; // Helm, Brust, Hose, Schuhe
pub const PLAYER_MAIN_START: usize = 9; // 27 Slots
pub const PLAYER_HOTBAR_START: usize = 36; // 9 Slots
pub const PLAYER_OFFHAND: usize = 45;
// HandledScreen.handleHotbarKeyPressed: die Offhand-Taste ueber einem Slot schickt SWAP mit
// Knopf 40 -- der Index der Zweithand im PlayerInventory.
pub const OFFHAND_SWAP_BUTTON: i8 = 40;

// Vanilla-Groessen in GUI-Pixeln (vor der Skalierung).
pub const SLOT_PITCH: f32 = 18.0;
pub const ICON_SIZE: f32 = 16.0;
pub const INVENTORY_SHEET_W: f32 = 176.0;
pub const INVENTORY_SHEET_H: f32 = 166.0;
pub const HOTBAR_W: f32 = 182.0;
pub const HOTBAR_H: f32 = 22.0;
pub const HOTBAR_SELECTION: f32 = 24.0;

#[derive(Default)]
pub struct OpenContainer {
    pub sync_id: i32,
    // ScreenHandlerType-Registry-ID aus dem Open-Screen-Paket -- entscheidet, ob die
    // Slots gewoehnliche Lagerplaetze sind (s. GENERIC_CONTAINER_TYPES). -1 = noch
    // unbekannt (Inhalt kam vor dem Open-Screen-Paket).
    pub screen_type: i32,
    pub title: String,
    pub slots: Vec<Option<ItemStack>>,
    // Zuletzt vom Server geschickte Revision -- muss jedes Klick-Paket mitfuehren, sonst
    // schickt der Server den kompletten Container erneut (s. onClickSlot).
    pub revision: i32,
    // Zahlenwerte des Containers (ContainerSetData), Index = Datenplatz. Zaubertisch: 0..2
    // Kosten, 3 Samen, 4..6 Verzauberungs-Hinweis, 7..9 Stufen-Hinweis; Amboss: 0 Kosten.
    pub data: Vec<i16>,
    // Nur am Handelsbildschirm: Angebote, Auswahl, Scrollstand (s. MerchantState).
    pub merchant: MerchantState,
}

// Zustand des Handelsbildschirms (MerchantScreen + MerchantMenu auf Client-Seite).
#[derive(Default)]
pub struct MerchantState {
    pub offers: Vec<protocol::packets::TradeOffer>,
    // Stufe 1..5 (Novize..Meister), 0 = unbekannt/kein Dorfbewohner (fahrender Haendler).
    pub level: i32,
    pub xp: i32,
    pub show_progress: bool,
    pub can_restock: bool,
    // Ausgewaehltes Angebot (MerchantScreen.selectedIndex).
    pub selected: usize,
    // Erstes sichtbares Angebot der Liste (MerchantScreen.indexStartOffset).
    pub scroll: usize,
    // Zieht der Nutzer gerade am Schieber?
    pub dragging: bool,
    // Zuletzt angeklickter Knopf (Position 0..7, nicht Angebot): er bleibt hervorgehoben,
    // wie ein fokussierter Knopf in Vanilla.
    pub focused: Option<usize>,
}

impl MerchantState {
    // Wie weit die Liste hoechstens scrollt: alles ueber sieben Angebote hinaus.
    pub fn max_scroll(&self) -> usize {
        self.offers.len().saturating_sub(MERCHANT_VISIBLE_OFFERS)
    }

    // Mausrad: eine Zeile je Rastung, begrenzt auf die Liste (MerchantScreen.mouseScrolled).
    pub fn scroll_by(&mut self, delta: f64) {
        if self.offers.len() > MERCHANT_VISIBLE_OFFERS {
            self.scroll = (self.scroll as f64 - delta).clamp(0.0, self.max_scroll() as f64) as usize;
        }
    }

    // Schieber ziehen (MerchantScreen.mouseDragged): `rel_y` ist die Zeigerhoehe ueber der
    // Bahnoberkante in GUI-Pixeln. Die halbe Schieberhoehe (13.5) zentriert ihn am Zeiger.
    pub fn drag_to(&mut self, rel_y: f32) {
        let n = self.max_scroll();
        let f = (rel_y - 13.5) / (MERCHANT_SCROLL_TRACK - 27.0) * n as f32 + 0.5;
        self.scroll = (f as i32).clamp(0, n as i32) as usize;
    }

    // Oberkante des Schiebers ueber der Bahn in GUI-Pixeln (MerchantScreen.renderScrollbar);
    // None = nichts zu scrollen (der graue Schieber steht oben).
    pub fn scroller_offset(&self) -> Option<f32> {
        let k = (self.offers.len() + 1).checked_sub(MERCHANT_VISIBLE_OFFERS)? as i32;
        if k <= 1 {
            return None;
        }
        let bahn = MERCHANT_SCROLL_TRACK as i32;
        let l = bahn - (27 + (k - 1) * bahn / k);
        let m = 1 + l / k + bahn / k;
        let mut n = (self.scroll as i32 * m).min(113);
        if self.scroll as i32 == k - 1 {
            n = 113;
        }
        Some(n as f32)
    }
}

pub struct Inventory {
    // ScreenHandler-0-Slots des Spielers (immer PLAYER_SLOT_COUNT Eintraege).
    pub player: Vec<Option<ItemStack>>,
    pub player_revision: i32,
    // Aktiver Hotbar-Slot 0..8.
    pub held_slot: usize,
    // Stack an der Maus (waehrend des Umsortierens).
    pub cursor: Option<ItemStack>,
    // Offener Container (Kiste, Ofen, ...). None = nur das Spielerinventar.
    pub open: Option<OpenContainer>,
    // Ist der Inventar-/Container-Bildschirm sichtbar?
    pub screen_open: bool,
}

impl Default for Inventory {
    fn default() -> Self {
        Self { player: vec![None; PLAYER_SLOT_COUNT], player_revision: 0, held_slot: 0, cursor: None, open: None, screen_open: false }
    }
}

impl Inventory {
    // Beim Verbindungsabbruch/Neuverbinden: alles zurueck auf leer.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn hotbar(&self, index: usize) -> Option<&ItemStack> {
        self.player.get(PLAYER_HOTBAR_START + index).and_then(|s| s.as_ref())
    }

    // Der gerade sichtbare Container: dessen sync_id, Revision und Slots.
    pub fn active(&self) -> (i32, i32, &[Option<ItemStack>]) {
        match &self.open {
            Some(container) => (container.sync_id, container.revision, container.slots.as_slice()),
            None => (0, self.player_revision, self.player.as_slice()),
        }
    }

    // Der Stapel in einem Slot des GERADE aktiven Bereichs (Container, sonst Spielerinventar).
    pub fn slot_at_index(&self, slot: usize) -> Option<&ItemStack> {
        let (_, _, slots) = self.active();
        slots.get(slot).and_then(|s| s.as_ref())
    }

    pub fn slot_at_index_mut(&mut self, slot: usize) -> Option<&mut ItemStack> {
        let slots = match &mut self.open {
            Some(container) => &mut container.slots,
            None => &mut self.player,
        };
        slots.get_mut(slot).and_then(|s| s.as_mut())
    }

    pub fn set_content(&mut self, sync_id: i32, revision: i32, mut slots: Vec<Option<ItemStack>>) {
        // Abgeschnittener Inhalt (ein Item war unlesbar): auf die Groesse auffuellen, die der
        // Typ vorgibt, und den Spielerteil aus dem bekannten Inventar nehmen -- so liegt
        // wenigstens alles, was lesbar war, am richtigen Platz, und die Hotbar bleibt sichtbar.
        let typ = self.open.as_ref().filter(|c| c.sync_id == sync_id).map(|c| c.screen_type);
        if let Some(reihen) = typ.and_then(container_rows).filter(|_| sync_id != 0) {
            let soll = reihen * 9 + 36;
            if slots.len() < soll {
                let lesbar = slots.len();
                slots.resize(soll, None);
                let start = reihen * 9;
                for i in lesbar.max(start)..soll {
                    let offset = i - start;
                    let quelle = if offset < 27 { PLAYER_MAIN_START + offset } else { PLAYER_HOTBAR_START + (offset - 27) };
                    slots[i] = self.player[quelle].clone();
                }
            }
        }
        if sync_id == 0 {
            self.player_revision = revision;
            for (index, stack) in slots.into_iter().enumerate() {
                if index < PLAYER_SLOT_COUNT {
                    self.player[index] = stack;
                }
            }
            return;
        }
        // Bei jedem Standard-Container sind die letzten 36 Slots das Spielerinventar
        // (27 Haupt + 9 Hotbar) -- von dort in die Hotbar-Anzeige uebernehmen, sonst waere
        // die HUD-Hotbar waehrend einer offenen Kiste veraltet.
        if slots.len() >= 36 {
            let start = slots.len() - 36;
            self.player[PLAYER_MAIN_START..PLAYER_MAIN_START + 27].clone_from_slice(&slots[start..start + 27]);
            for i in 0..9 {
                self.player[PLAYER_HOTBAR_START + i] = slots[start + 27 + i].clone();
            }
        }
        match &mut self.open {
            Some(container) if container.sync_id == sync_id => {
                container.revision = revision;
                container.slots = slots;
            }
            _ => {
                // Inhalt fuer einen Container, den wir (noch) nicht kennen -- Screen erst bei
                // OpenScreen zeigen, den Inhalt aber schon aufheben.
                self.open = Some(OpenContainer { sync_id, screen_type: -1, title: String::new(), slots, revision, data: Vec::new(), merchant: MerchantState::default() });
            }
        }
    }

    pub fn set_slot(&mut self, sync_id: i32, revision: i32, slot: i16, stack: Option<ItemStack>) {
        if slot < 0 {
            return;
        }
        let slot = slot as usize;
        if sync_id == 0 {
            self.player_revision = revision;
            if slot < PLAYER_SLOT_COUNT {
                self.player[slot] = stack;
            }
            return;
        }
        if let Some(container) = &mut self.open {
            if container.sync_id == sync_id && slot < container.slots.len() {
                container.revision = revision;
                container.slots[slot] = stack.clone();
                // Spiegeln, falls der Slot zum Spielerteil des Containers gehoert.
                let start = container.slots.len().saturating_sub(36);
                if slot >= start {
                    let offset = slot - start;
                    let target = if offset < 27 { PLAYER_MAIN_START + offset } else { PLAYER_HOTBAR_START + (offset - 27) };
                    self.player[target] = stack;
                }
            }
        }
    }

    // Set Player Inventory (0x6A) nummeriert nach PlayerInventory, nicht nach ScreenHandler:
    // 0..8 Hotbar, 9..35 Hauptinventar, 36..39 Ruestung (Fuesse zuerst), 40 Zweithand.
    pub fn set_player_slot(&mut self, slot: i32, stack: Option<ItemStack>) {
        let target = match slot {
            0..=8 => PLAYER_HOTBAR_START + slot as usize,
            9..=35 => slot as usize,
            36..=39 => PLAYER_ARMOR_START + (39 - slot as usize),
            40 => PLAYER_OFFHAND,
            _ => return,
        };
        self.player[target] = stack;
    }

    pub fn open_screen(&mut self, sync_id: i32, screen_type: i32, title: String) {
        match &mut self.open {
            Some(container) if container.sync_id == sync_id => {
                container.screen_type = screen_type;
                container.title = title;
            }
            _ => self.open = Some(OpenContainer { sync_id, screen_type, title, slots: Vec::new(), revision: 0, data: Vec::new(), merchant: MerchantState::default() }),
        }
        self.screen_open = true;
    }

    // ContainerSetData: nur fuer den gerade offenen Container.
    pub fn set_data(&mut self, sync_id: i32, index: i16, value: i16) {
        let Some(container) = self.open.as_mut().filter(|c| c.sync_id == sync_id) else { return };
        let Ok(i) = usize::try_from(index) else { return };
        if i >= 64 {
            return;
        }
        if container.data.len() <= i {
            container.data.resize(i + 1, 0);
        }
        container.data[i] = value;
    }

    // Angebotsliste fuer den offenen Handelsbildschirm. Auswahl und Scrollstand bleiben,
    // werden aber auf die neue Liste begrenzt (der Server schickt sie nach jedem Handel neu).
    pub fn set_merchant_offers(&mut self, m: protocol::packets::MerchantOffers) {
        let Some(container) = self.open.as_mut().filter(|c| c.sync_id == m.sync_id) else { return };
        let st = &mut container.merchant;
        st.offers = m.offers;
        st.level = m.level;
        st.xp = m.xp;
        st.show_progress = m.show_progress;
        st.can_restock = m.can_restock;
        st.selected = st.selected.min(st.offers.len().saturating_sub(1));
        st.scroll = st.scroll.min(st.max_scroll());
    }

    pub fn close(&mut self) {
        self.open = None;
        self.screen_open = false;
        self.cursor = None;
    }
}

// --- Klick-Vorhersage --------------------------------------------------------------------
//
// Seit 1.21.5 beschreibt jedes Klick-Paket, wie das Ergebnis aussehen soll (s.
// packets::click_container_payload). Ein Client, der nichts vorhersagt, kann dort nur
// "weiss ich nicht" schicken und bekommt jeden Slot einzeln zurueckgeschickt.
//
// Deshalb rechnet der Client den einfachen Fall selbst: das reine Aufnehmen/Ablegen mit
// Links- und Rechtsklick auf gewoehnliche Lagerslots, und nur fuer Stapel ohne gesetzte
// Components (nur die lassen sich exakt hashen, s. ItemStack::hashable). Alles andere
// (Shift-Klick durch den halben Container, Ruestungs-/Ergebnis-/Werkbank-Slots, Stapel mit
// Verzauberungen o.ae.) wird bewusst NICHT vorhergesagt -- dort geht weiter der leere Hash
// raus und der Server hat wie bisher das letzte Wort.
//
// Verrechnet sich die Vorhersage trotzdem, ist das kein Schaden: der Server merkt es am
// abweichenden Hash und schickt den richtigen Inhalt nach.

// Ergebnis einer Vorhersage: was sich lokal aendert und was ins Paket geschrieben wird.
// Ein Klick mit oder auf ein Buendel (BundleItem.overrideStackedOnOther, dann
// overrideOtherStackedOnMe): Linksklick legt ein, Rechtsklick nimmt heraus -- statt zu tauschen
// bzw. zu teilen. None, wenn kein Buendel betroffen ist und der gewoehnliche Klick gilt. Dazu der
// Klang. Ein veraenderter Inhalt ist nicht mehr hashbar (gesetzte Component).
fn bundle_click(slot: i16, button: i8, cursor: Option<ItemStack>, in_slot: Option<ItemStack>) -> Option<(Prediction, Option<&'static str>)> {
    use world::bundle;
    let geaendert = |mut b: ItemStack| {
        b.has_added_components = true;
        b
    };
    let fertig = |s: Option<ItemStack>, c: Option<ItemStack>, klang| Some((Prediction { modified: vec![(slot, s)], cursor: c, hashable: false }, klang));
    match (button, cursor, in_slot) {
        // Buendel an der Maus, Linksklick auf ein Item: das Item hinein.
        (0, Some(b), Some(mut s)) if bundle::is_bundle(&b) => {
            let mut b = b;
            if bundle::insert(&mut b, &mut s) > 0 {
                fertig((s.count > 0).then_some(s), Some(geaendert(b)), Some("item.bundle.insert"))
            } else {
                fertig(Some(s), Some(b), Some("item.bundle.insert_fail"))
            }
        }
        // Buendel an der Maus, Rechtsklick auf einen leeren Platz: ein Eintrag heraus.
        (1, Some(b), None) if bundle::is_bundle(&b) => {
            let mut b = b;
            match bundle::remove_one(&mut b) {
                Some(raus) => fertig(Some(raus), Some(geaendert(b)), Some("item.bundle.remove_one")),
                None => fertig(None, Some(b), None),
            }
        }
        // Buendel im Platz, Linksklick mit einem Item: das Item hinein.
        (0, Some(mut c), Some(b)) if bundle::is_bundle(&b) => {
            let mut b = b;
            if bundle::insert(&mut b, &mut c) > 0 {
                fertig(Some(geaendert(b)), (c.count > 0).then_some(c), Some("item.bundle.insert"))
            } else {
                fertig(Some(b), Some(c), Some("item.bundle.insert_fail"))
            }
        }
        // Buendel im Platz, Rechtsklick mit leerer Hand: ein Eintrag an die Maus.
        (1, None, Some(b)) if bundle::is_bundle(&b) => {
            let mut b = b;
            match bundle::remove_one(&mut b) {
                Some(raus) => fertig(Some(geaendert(b)), Some(raus), Some("item.bundle.remove_one")),
                None => fertig(Some(b), None, None),
            }
        }
        _ => None,
    }
}

pub struct Prediction {
    pub modified: Vec<(i16, Option<ItemStack>)>,
    pub cursor: Option<ItemStack>,
    // Laesst sich das Ergebnis dem Server byte-genau BESCHREIBEN (s. ItemStack::hashable)?
    // Gesendet wird die Vorhersage so oder so: ohne exakten Hash weicht er absichtlich ab und
    // der Server schickt seinen Stand (s. packets::write_item_stack_hash). Leere Felder
    // statt dessen liessen ihn glauben, beim Client habe sich nichts geaendert -- dann kam
    // keine Korrektur. Bleibt als Angabe fuer die Tests.
    #[cfg_attr(not(test), allow(dead_code))]
    pub hashable: bool,
}

// Ein gesperrter Klick (Ruestung mit Fluch der Bindung, s. App::predict_inventory_click):
// Vanillas ArmorSlot.canTakeItems ist false, ScreenHandler.internalOnSlotClick laesst dann
// ALLES stehen -- Slot und Cursor. Das Paket geht trotzdem raus, mit genau diesem
// unveraenderten Zustand.
pub fn locked_prediction(cursor: Option<&ItemStack>) -> Prediction {
    Prediction { modified: Vec::new(), cursor: cursor.cloned(), hashable: cursor.is_none_or(|c| c.hashable()) }
}

// Das Zittern eines gesperrten Slots (Fluch der Bindung) -- KEIN Vanilla-Verhalten, dort
// passiert optisch gar nichts. Gewuenscht war eine Rueckmeldung wie in osu! beim zu fruehen
// Klick auf einen Kreis: ein kurzes, schnell abklingendes Hin-und-her zur Seite.
//
// Waagerechter Versatz in GUI-Pixeln (16 = Slotbreite) nach `sekunden` seit dem Klick; 0,
// sobald es vorbei ist.
pub const SHAKE_SECONDS: f32 = 0.3;
const SHAKE_AMPLITUDE_GUI_PX: f32 = 2.0;
const SHAKE_HZ: f32 = 10.0;

pub fn shake_offset(sekunden: f32) -> f32 {
    if !(0.0..SHAKE_SECONDS).contains(&sekunden) {
        return 0.0;
    }
    let abklingen = 1.0 - sekunden / SHAKE_SECONDS;
    SHAKE_AMPLITUDE_GUI_PX * abklingen * (std::f32::consts::TAU * SHAKE_HZ * sekunden).sin()
}

// Menue-Typen (ScreenHandlerType-Registry-IDs aus registries.json), bei denen JEDER Slot
// ein gewoehnlicher Lagerplatz ohne Annahmeregeln ist: generic_9x1 .. generic_9x6.
const GENERIC_CONTAINER_TYPES: std::ops::RangeInclusive<i32> = 0..=5;

impl Inventory {
    // Darf fuer diesen Slot ueberhaupt vorhergesagt werden? Nur wo der Slot alles annimmt.
    fn slot_is_plain_storage(&self, slot: usize) -> bool {
        match &self.open {
            // Container: nur bei den generischen Kisten-Menues, dort sind auch die
            // Spielerinventar-Slots am Ende gewoehnliche Plaetze.
            Some(container) => GENERIC_CONTAINER_TYPES.contains(&container.screen_type) && slot < container.slots.len(),
            // Spielerinventar: Hauptteil, Hotbar, Zweithand und die vier Craft-Felder (die
            // nehmen alles an). NICHT die Ruestung (nimmt nur passende Teile) und nicht das
            // Ergebnisfeld. Beim Craft-Feld aendert sich zusaetzlich das Ergebnis -- dazu
            // sagen wir nichts, also schickt der Server es von sich aus.
            None => (PLAYER_MAIN_START..=PLAYER_OFFHAND).contains(&slot) || (1..=4).contains(&slot),
        }
    }

    fn slot_stack(&self, slot: usize) -> Option<&ItemStack> {
        let (_, _, slots) = self.active();
        slots.get(slot).and_then(|s| s.as_ref())
    }

    // Der Bildschirm-Slot, hinter dem Hotbar-Platz 0..8 steckt. Bei offenem Container haengen
    // die 36 Spielerslots hinten dran (27 Hauptteil, dann 9 Hotbar), sonst ist es das
    // Spielerinventar selbst.
    pub fn hotbar_screen_slot(&self, index: usize) -> Option<usize> {
        // 40 = die Zweithand (Vanillas Tausch per Offhand-Taste, s. OFFHAND_SWAP_BUTTON). Nur im
        // eigenen Inventar ein Bildschirm-Platz -- in einem Container gibt es sie dort nicht,
        // dann bleibt es ohne Vorhersage beim Serverwort.
        if index == OFFHAND_SWAP_BUTTON as usize {
            return self.open.is_none().then_some(PLAYER_OFFHAND);
        }
        if index > 8 {
            return None;
        }
        match &self.open {
            Some(container) => Some(container.slots.len().checked_sub(9)? + index),
            None => Some(PLAYER_HOTBAR_START + index),
        }
    }

    // Vorhersage fuer einen Klick. None = nicht vorhersagbar (Aufrufer schickt leere Hashes
    // und wartet auf den Server).
    pub fn predict_click(&self, slot: i16, button: i8, mode: i32) -> Option<Prediction> {
        if mode == protocol::packets::CLICK_MODE_QUICK_MOVE {
            return self.predict_quick_move(slot);
        }
        if mode == protocol::packets::CLICK_MODE_SWAP {
            return self.predict_swap(slot, usize::try_from(button).ok()?);
        }
        if mode == protocol::packets::CLICK_MODE_PICKUP_ALL {
            return self.predict_pickup_all(slot, button);
        }
        if mode != protocol::packets::CLICK_MODE_PICKUP {
            return None;
        }
        // Wegwerfen (Klick ins Leere): nur der Cursor aendert sich.
        if slot == DROP_SLOT {
            let cursor = self.cursor.as_ref()?;
            let hashable = cursor.hashable();
            let rest = match button {
                0 => None,
                1 if cursor.count > 1 => Some(ItemStack { count: cursor.count - 1, ..cursor.clone() }),
                1 => None,
                _ => return None,
            };
            return Some(Prediction { modified: Vec::new(), cursor: rest, hashable });
        }

        let slot_index = usize::try_from(slot).ok()?;
        if !self.slot_is_plain_storage(slot_index) {
            return None;
        }
        let in_slot = self.slot_stack(slot_index).cloned();
        let cursor = self.cursor.clone();
        // Nicht hashbare Stapel (Components: verzaubert, benannt, beschaedigt) verhinderten
        // hier frueher die GANZE Vorhersage -- also wartete gerade bei echter Ausruestung die
        // Anzeige jedes Mal auf den Server. Sie verhindern nur die Hash-FELDER des Pakets;
        // lokal vorgreifen geht weiterhin (s. Prediction::hashable).
        let hashable = in_slot.as_ref().is_none_or(|s| s.hashable()) && cursor.as_ref().is_none_or(|s| s.hashable());
        if let Some((p, _klang)) = bundle_click(slot, button, cursor.clone(), in_slot.clone()) {
            return Some(p);
        }

        let (new_slot, new_cursor) = match (button, cursor, in_slot) {
            // Linksklick, leere Hand: alles aufnehmen.
            (0, None, Some(stack)) => (None, Some(stack)),
            // Linksklick mit vollem Cursor auf leeren Slot: alles ablegen.
            (0, Some(cursor), None) => (Some(cursor), None),
            // Linksklick, gleiches Item: so viel zusammenlegen wie die Stapelgrenze zulaesst.
            (0, Some(cursor), Some(mut stack)) if stack.stackable_with(&cursor) => {
                let space = world::item_components::max_stack(&stack) - stack.count;
                let moved = space.clamp(0, cursor.count);
                stack.count += moved;
                let rest = cursor.count - moved;
                (Some(stack), (rest > 0).then_some(ItemStack { count: rest, ..cursor }))
            }
            // Linksklick, anderes Item: tauschen.
            (0, Some(cursor), Some(stack)) => (Some(cursor), Some(stack)),
            // Rechtsklick, leere Hand: die groessere Haelfte aufnehmen.
            (1, None, Some(stack)) => {
                let taken = (stack.count + 1) / 2;
                let rest = stack.count - taken;
                let mut picked = stack.clone();
                picked.count = taken;
                ((rest > 0).then_some(ItemStack { count: rest, ..stack }), Some(picked))
            }
            // Rechtsklick mit vollem Cursor: ein einzelnes Stueck ablegen.
            (1, Some(cursor), None) => {
                let mut single = cursor.clone();
                single.count = 1;
                let rest = cursor.count - 1;
                (Some(single), (rest > 0).then_some(ItemStack { count: rest, ..cursor }))
            }
            (1, Some(cursor), Some(mut stack)) if stack.stackable_with(&cursor) => {
                if stack.count >= world::item_components::max_stack(&stack) {
                    return None; // voll -- Vanilla tut hier nichts, das muessen wir nicht raten
                }
                stack.count += 1;
                let rest = cursor.count - 1;
                (Some(stack), (rest > 0).then_some(ItemStack { count: rest, ..cursor }))
            }
            // Alles Uebrige (Rechtsklick auf fremdes Item, leerer Slot ohne Cursor, andere
            // Tasten) bleibt dem Server ueberlassen.
            _ => return None,
        };

        Some(Prediction { modified: vec![(slot, new_slot)], cursor: new_cursor, hashable })
    }

    // Der Klang eines Buendel-Klicks (Einlegen, Einlegen misslungen, Herausnehmen) -- Vanilla
    // spielt ihn schon im Client.
    pub fn bundle_click_sound(&self, slot: i16, button: i8, mode: i32) -> Option<&'static str> {
        if mode != protocol::packets::CLICK_MODE_PICKUP {
            return None;
        }
        let slot_index = usize::try_from(slot).ok().filter(|i| self.slot_is_plain_storage(*i))?;
        bundle_click(slot, button, self.cursor.clone(), self.slot_stack(slot_index).cloned())?.1
    }

    // Vorhersage fuers Tauschen mit einem Hotbar-Platz (Zifferntaste ueber einem Slot,
    // SlotActionType.SWAP).
    //
    // Ohne sie wartete JEDER Zifferntausch auf die Serverantwort, und Hotbar-Swaps
    // mit der Taste wirkten langsam. Vanilla rechnet das wie jeden anderen Klick sofort selbst
    // (ClientPlayerInteractionManager.clickSlot ruft screenHandler.onSlotClick fuer ALLE
    // Aktionsarten und diffed danach die Slots fuer die Hash-Felder).
    //
    // Nachgebaut ist nur Vanillas einfacher Fall aus ScreenHandler.internalOnSlotClick: beide
    // Slots sind gewoehnliche Lagerplaetze, es wird schlicht getauscht. Vanillas Aufteilen
    // (`itemStack4.getCount() > slot.getMaxItemCount(...)`) kann dort nicht auftreten, weil die
    // Grenze eines Lagerplatzes die des Items selbst ist -- ein groesserer Stapel waere schon
    // vorher unmoeglich. Alles Uebrige (Ruestung, Ofen, Amboss) bleibt dem Server ueberlassen.
    fn predict_swap(&self, slot: i16, hotbar_index: usize) -> Option<Prediction> {
        let slot_index = usize::try_from(slot).ok()?;
        let hotbar_slot = self.hotbar_screen_slot(hotbar_index)?;
        if slot_index == hotbar_slot {
            return None; // Tausch mit sich selbst -- Vanilla tut nichts
        }
        if !self.slot_is_plain_storage(slot_index) || !self.slot_is_plain_storage(hotbar_slot) {
            return None;
        }
        let in_slot = self.slot_stack(slot_index).cloned();
        let in_hotbar = self.slot_stack(hotbar_slot).cloned();
        if in_slot.is_none() && in_hotbar.is_none() {
            return None; // beide leer -- Vanilla tut nichts
        }
        let hashable = in_slot.as_ref().is_none_or(|s| s.hashable()) && in_hotbar.as_ref().is_none_or(|s| s.hashable());
        Some(Prediction {
            modified: vec![(slot, in_hotbar), (hotbar_slot as i16, in_slot)],
            // Tauschen laesst den Stapel an der Maus unangetastet.
            cursor: self.cursor.clone(),
            hashable,
        })
    }

    // Vorhersage fuers Einsammeln per Doppelklick (SlotActionType.PICKUP_ALL).
    //
    // Vanillas ScreenHandler.internalOnSlotClick, Zweig PICKUP_ALL: sammelt gleichartige
    // Stapel auf den Cursor, aber NUR wenn der Cursor voll und der geklickte Slot leer ist.
    // Zwei Durchgaenge -- im ersten werden VOLLE Stapel uebersprungen, damit angebrochene
    // zuerst aufgebraucht werden, im zweiten zaehlt alles.
    fn predict_pickup_all(&self, slot: i16, button: i8) -> Option<Prediction> {
        if button != 0 {
            return None; // Vanilla laeuft dann rueckwaerts -- brauchen wir nicht, also raten wir nicht
        }
        let slot_index = usize::try_from(slot).ok()?;
        let mut cursor = self.cursor.clone()?;
        if self.slot_stack(slot_index).is_some() {
            return None; // Vanillas `!slot.hasStack()`
        }
        let max = world::item_components::max_stack(&cursor);
        if cursor.count >= max {
            return None;
        }
        let (_, _, slots) = self.active();
        let mut rest: Vec<Option<ItemStack>> = slots.to_vec();
        let mut geaendert: Vec<(i16, Option<ItemStack>)> = Vec::new();
        for durchgang in 0..2 {
            for index in 0..rest.len() {
                if cursor.count >= max {
                    break;
                }
                if !self.slot_is_plain_storage(index) {
                    continue;
                }
                let Some(vorhanden) = rest[index].clone() else { continue };
                if !vorhanden.stackable_with(&cursor) {
                    continue;
                }
                if durchgang == 0 && vorhanden.count >= world::item_components::max_stack(&vorhanden) {
                    continue;
                }
                let genommen = (max - cursor.count).min(vorhanden.count);
                cursor.count += genommen;
                let ubrig = vorhanden.count - genommen;
                let neu = (ubrig > 0).then_some(ItemStack { count: ubrig, ..vorhanden });
                rest[index] = neu.clone();
                geaendert.push((index as i16, neu));
            }
        }
        if geaendert.is_empty() {
            return None; // nichts zu holen -- dann sagt der Server, ob doch
        }
        let hashable = geaendert.iter().all(|(_, s)| s.as_ref().is_none_or(|s| s.hashable())) && cursor.hashable();
        Some(Prediction { modified: geaendert, cursor: Some(cursor), hashable })
    }

    // Vorhersage fuers Stapel-Verschieben (Shift-Klick, SlotActionType.QUICK_MOVE).
    //
    // Ohne sie wartete JEDER Shift-Klick auf die Serverantwort, bevor sich etwas bewegte --
    // die haeufigste Inventar-Aktion ueberhaupt, und jeder Stapelwechsel
    // hatte spuerbare Latenz.
    //
    // Nachgebaut sind Vanillas zwei Alltagsfaelle (PlayerScreenHandler.quickMove und
    // GenericContainerScreenHandler.quickMove):
    //   * Spielerinventar ohne Container: Hauptteil (9..36) <-> Hotbar (36..45)
    //   * generische Kiste: Kistenteil -> Spielerteil (von HINTEN gefuellt) und zurueck
    // Alles andere (Ruestung, Werkbank, Ofen, Amboss) hat eigene Regeln und bleibt
    // unvorhergesagt -- dort ist eine falsche Vorhersage schlimmer als eine kurze Wartezeit.
    fn predict_quick_move(&self, slot: i16) -> Option<Prediction> {
        let index = usize::try_from(slot).ok()?;
        let quelle = self.slot_stack(index)?.clone();
        let (_, _, slots) = self.active();

        let (start, ende, von_hinten) = match &self.open {
            Some(container) => {
                if !GENERIC_CONTAINER_TYPES.contains(&container.screen_type) {
                    return None;
                }
                let kiste = container.slots.len().checked_sub(36)?;
                if index < kiste {
                    (kiste, slots.len(), true) // Kiste -> Spieler, von hinten
                } else {
                    (0, kiste, false) // Spieler -> Kiste
                }
            }
            None => match index {
                // Hauptteil -> Hotbar, Hotbar -> Hauptteil. Beide von vorn, wie Vanilla.
                PLAYER_MAIN_START..=35 => (PLAYER_HOTBAR_START, PLAYER_OFFHAND, false),
                PLAYER_HOTBAR_START..=44 => (PLAYER_MAIN_START, PLAYER_HOTBAR_START, false),
                _ => return None,
            },
        };

        let max = world::item_components::max_stack(&quelle);
        let mut rest = quelle.count;
        let mut geaendert: Vec<(i16, Option<ItemStack>)> = Vec::new();
        // Zwei Durchgaenge, genau wie ScreenHandler.insertItem: erst auf vorhandene Stapel
        // derselben Sorte verteilen, danach der erste freie Platz.
        let reihenfolge: Vec<usize> = if von_hinten { (start..ende).rev().collect() } else { (start..ende).collect() };
        for &ziel in &reihenfolge {
            if rest == 0 {
                break;
            }
            let Some(vorhanden) = slots.get(ziel).and_then(|s| s.as_ref()) else { continue };
            if !vorhanden.stackable_with(&quelle) {
                continue;
            }
            let platz = (max - vorhanden.count).clamp(0, rest);
            if platz == 0 {
                continue;
            }
            rest -= platz;
            geaendert.push((ziel as i16, Some(ItemStack { count: vorhanden.count + platz, ..vorhanden.clone() })));
        }
        if rest > 0 {
            if let Some(&frei) = reihenfolge.iter().find(|&&z| slots.get(z).is_none_or(|s| s.is_none())) {
                geaendert.push((frei as i16, Some(ItemStack { count: rest, ..quelle.clone() })));
                rest = 0;
            }
        }
        if geaendert.is_empty() {
            return None; // nichts passt -- Vanilla laesst den Stapel dann liegen
        }
        // Der Quellslot behaelt, was nicht untergekommen ist.
        geaendert.push((slot, (rest > 0).then(|| ItemStack { count: rest, ..quelle.clone() })));

        // Hashbar nur, wenn JEDER beteiligte Stapel es ist (s. Prediction::hashable).
        let hashable = quelle.hashable() && geaendert.iter().all(|(_, s)| s.as_ref().is_none_or(|s| s.hashable()));
        Some(Prediction { modified: geaendert, cursor: self.cursor.clone(), hashable })
    }

    // Die Vorhersage lokal anwenden, damit die Anzeige sofort reagiert. Der Server
    // ueberschreibt das ohnehin, sobald er widerspricht.
    pub fn apply_prediction(&mut self, prediction: &Prediction) {
        for (slot, stack) in &prediction.modified {
            let Ok(index) = usize::try_from(*slot) else { continue };
            match &mut self.open {
                Some(container) if index < container.slots.len() => {
                    container.slots[index] = stack.clone();
                    // Spielerteil des Containers spiegeln (s. set_slot).
                    let start = container.slots.len().saturating_sub(36);
                    if index >= start {
                        let offset = index - start;
                        let target = if offset < 27 { PLAYER_MAIN_START + offset } else { PLAYER_HOTBAR_START + (offset - 27) };
                        self.player[target] = stack.clone();
                    }
                }
                Some(_) => {}
                None if index < PLAYER_SLOT_COUNT => self.player[index] = stack.clone(),
                None => {}
            }
        }
        self.cursor = prediction.cursor.clone();
    }
}

// Vanillas Sonder-Slotnummer fuer "neben das Fenster geklickt" = wegwerfen.
pub const DROP_SLOT: i16 = -999;

// --- Bildschirmaufteilung ---------------------------------------------------------------

// Ein Slot auf dem Bildschirm: Index im aktiven Container plus die linke obere Ecke des
// 16x16-Icons in Bildschirmpixeln.
#[derive(Clone, Copy, Debug)]
pub struct SlotRect {
    pub index: usize,
    pub x: f32,
    pub y: f32,
    pub size: f32,
    // Wie weit die TREFFERFLAECHE ueber das Icon hinausragt (je Seite, in Bildschirm-
    // pixeln). Die Slots stehen im 18er-Raster, das Icon ist aber nur 16 breit -- ohne
    // diesen Rand laegen zwischen den Slots 2 Pixel tote Zone, und ein Klick dorthin
    // zaehlte als "daneben" (und warf damit den Stapel an der Maus weg).
    pub pad: f32,
}

impl SlotRect {
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x - self.pad && px < self.x + self.size + self.pad && py >= self.y - self.pad && py < self.y + self.size + self.pad
    }
}

// Aufteilung des offenen Bildschirms: Wo liegt das Hintergrund-Sheet, welche Slots gibt es.
pub struct ScreenLayout {
    // Hintergrund in Bildschirmpixeln (linke obere Ecke + Groesse).
    pub bg_x: f32,
    pub bg_y: f32,
    // Groesse des Hintergrund-Sheets in Bildschirmpixeln. Entscheidet mit, ob ein Klick
    // "neben das Fenster" ging (= wegwerfen) oder nur in eine Ritze innerhalb des Fensters
    // (= nichts tun), s. ScreenLayout::is_outside.
    pub bg_w: f32,
    pub bg_h: f32,
    // Sheet-Teile, die gezeichnet werden: (Quellrechteck im Sheet in GUI-Pixeln, Ziel-Y-
    // Versatz in GUI-Pixeln). Beim Spielerinventar ein Teil, bei Kisten zwei (oberer
    // Container-Teil + der Spielerinventar-Teil aus generic_54.png).
    pub sheet: &'static str,
    pub parts: Vec<((u32, u32, u32, u32), f32)>,
    pub slots: Vec<SlotRect>,
    pub scale: f32,
    // Senkrechte Trennlinie in Bildschirmpixeln: Slots links davon bekommen ihren Tooltip
    // nach LINKS, Slots rechts davon nach RECHTS (s. ScreenLayout::tooltip_anchor). Im
    // Spielerinventar liegt sie zwischen Zweithand-Slot und Werkbank-Feldern, bei
    // Containern schlicht in der Fenstermitte.
    pub tooltip_divider_x: f32,
    // Rechteck der Spielerfigur (Vanillas "Puppe") in Bildschirmpixeln, falls dieser
    // Bildschirm eine hat. Nur das SPIELERINVENTAR zeichnet sie -- Kisten, Oefen und
    // Werkbaenke nicht.
    pub doll: Option<DollRect>,
}

// Wo und wie gross die Spielerfigur im Inventar steht. Vanillas InventoryScreen zeichnet sie
// per drawEntity im Rechteck (26, 8)..(75, 78) relativ zum Fenster, mit Groesse 30 und
// Versatz 0.0625.
//
// Die 30 sind der Massstab in GUI-Pixeln je Meter (drawEntity skaliert damit), nicht etwa eine Pixelgroesse der Figur.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DollRect {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    // Bildschirmpixel je Meter Modellhoehe (30 * gui_scale).
    pub pixels_per_meter: f32,
}

impl DollRect {
    pub fn center(&self) -> (f32, f32) {
        ((self.x0 + self.x1) / 2.0, (self.y0 + self.y1) / 2.0)
    }
}

// Vanillas `size`-Parameter in InventoryScreen.render.
pub const DOLL_SIZE: f32 = 30.0;

impl ScreenLayout {
    pub fn slot_at(&self, px: f32, py: f32) -> Option<usize> {
        self.slots.iter().find(|slot| slot.contains(px, py)).map(|slot| slot.index)
    }

    // Bildschirmmitte des Icons dieses Slots (ohne den Trefferrand).
    pub fn slot_center(&self, index: usize) -> Option<(f32, f32)> {
        self.slots.iter().find(|slot| slot.index == index).map(|slot| (slot.x + slot.size / 2.0, slot.y + slot.size / 2.0))
    }

    // Wo der Tooltip hingehoert: (x, MITTE-y, rechtsbuendig?). Zwei feste Plaetze NEBEN dem
    // Fenster -- rechts daneben fuer Slots rechts der Trennlinie, links daneben fuer die
    // links davon. Damit steht die Beschriftung immer an derselben Stelle (springt nicht mit
    // der Maus mit) und liegt nie ueber einem anderen Item, weil sie ausserhalb des
    // Fensters sitzt.
    //
    // Die Hoehe folgt dem Item: der Text steht auf derselben Zeile wie die Kachel, ueber
    // die man gerade faehrt. Nur die Seite ist fest -- so bleibt der Bezug zum Item
    // erkennbar, ohne dass der Text jemals ueber einer anderen Kachel liegt. Der Aufrufer
    // zieht die halbe Texthoehe ab (die Schriftmasse kennt nur die GPU-Seite).
    pub fn tooltip_anchor(&self, slot: &SlotRect) -> (f32, f32, bool) {
        let gap = 6.0 * self.scale;
        let y = slot.y + slot.size / 2.0;
        if slot.x + slot.size / 2.0 >= self.tooltip_divider_x {
            (self.bg_x + self.bg_w + gap, y, false)
        } else {
            (self.bg_x - gap, y, true)
        }
    }

    // Liegt der Punkt ausserhalb des Fensters? Nur dann wirft ein Klick den Stapel an der
    // Maus weg -- innerhalb des Fensters (Rand, Beschriftungszeile, Ritze zwischen zwei
    // Slots) passiert bewusst nichts, genau wie in Vanilla.
    pub fn is_outside(&self, px: f32, py: f32) -> bool {
        px < self.bg_x || px >= self.bg_x + self.bg_w || py < self.bg_y || py >= self.bg_y + self.bg_h
    }
}

// Ganzzahlige GUI-Skalierung wie Vanillas "Auto": so gross wie moeglich, solange das
// 320x240-Grundraster noch passt. Wie in Vanilla ohne Deckel -- auf 4K ergibt das x9.
pub fn gui_scale(screen: (f32, f32)) -> f32 {
    let by_width = (screen.0 / 320.0).floor();
    let by_height = (screen.1 / 240.0).floor();
    by_width.min(by_height).max(1.0)
}

// Hoechste feste Stufe, die die Einstellungen anbieten: was Auto ergibt, aber mindestens
// x4, damit kleine Fenster ihre gewohnten Stufen behalten.
pub fn max_gui_scale(screen: (f32, f32)) -> u32 {
    (gui_scale(screen) as u32).max(4)
}

// Obergrenze fuer gespeicherte Werte, bevor die Bildschirmgroesse bekannt ist (8K traegt x18).
pub const GUI_SCALE_SETTING_MAX: u32 = 32;

// Die Hotbar unten: Sheet-Position und die neun Icon-Positionen. `origin` ist die linke
// obere Ecke des Sheets in Bildschirmpixeln (aus dem HUD-Editor).
pub fn hotbar_slots(origin: (f32, f32), scale: f32) -> Vec<SlotRect> {
    (0..9)
        .map(|i| SlotRect {
            index: PLAYER_HOTBAR_START + i,
            x: origin.0 + (3.0 + i as f32 * 20.0) * scale,
            y: origin.1 + 3.0 * scale,
            size: ICON_SIZE * scale,
            pad: 2.0 * scale, // Hotbar steht im 20er-Raster
        })
        .collect()
}

// Aufteilung des Spielerinventars (Vanillas InventoryScreen, 176x166).
pub fn player_screen_layout(screen: (f32, f32), scale: f32) -> ScreenLayout {
    let bg_w = INVENTORY_SHEET_W * scale;
    let bg_h = INVENTORY_SHEET_H * scale;
    let bg_x = ((screen.0 - bg_w) / 2.0).floor();
    let bg_y = ((screen.1 - bg_h) / 2.0).floor();
    let at = |index: usize, gx: f32, gy: f32| SlotRect {
        index,
        x: bg_x + gx * scale,
        y: bg_y + gy * scale,
        size: ICON_SIZE * scale,
        pad: (SLOT_PITCH - ICON_SIZE) / 2.0 * scale,
    };

    let mut slots = Vec::with_capacity(PLAYER_SLOT_COUNT);
    slots.push(at(PLAYER_CRAFT_RESULT, 154.0, 28.0));
    for i in 0..4 {
        slots.push(at(1 + i, 98.0 + (i % 2) as f32 * SLOT_PITCH, 18.0 + (i / 2) as f32 * SLOT_PITCH));
    }
    for i in 0..4 {
        slots.push(at(PLAYER_ARMOR_START + i, 8.0, 8.0 + i as f32 * SLOT_PITCH));
    }
    for i in 0..27 {
        slots.push(at(PLAYER_MAIN_START + i, 8.0 + (i % 9) as f32 * SLOT_PITCH, 84.0 + (i / 9) as f32 * SLOT_PITCH));
    }
    for i in 0..9 {
        slots.push(at(PLAYER_HOTBAR_START + i, 8.0 + i as f32 * SLOT_PITCH, 142.0));
    }
    slots.push(at(PLAYER_OFFHAND, 77.0, 62.0));

    ScreenLayout {
        bg_x,
        bg_y,
        bg_w,
        bg_h,
        sheet: "container/inventory",
        parts: vec![((0, 0, INVENTORY_SHEET_W as u32, INVENTORY_SHEET_H as u32), 0.0)],
        slots,
        scale,
        // Zwischen Zweithand-Slot (endet bei GUI-x 93) und Werkbank-Raster (beginnt bei 98).
        tooltip_divider_x: bg_x + 95.5 * scale,
        doll: Some(DollRect {
            x0: bg_x + 26.0 * scale,
            y0: bg_y + 8.0 * scale,
            x1: bg_x + 75.0 * scale,
            y1: bg_y + 78.0 * scale,
            pixels_per_meter: DOLL_SIZE * scale,
        }),
    }
}

// Wo der Zeiger beim Oeffnen des Spielerinventars erscheint: in der Mitte des mittleren Slots
// der obersten Inventarreihe. Aus dem Layout, damit die Skalierung (Settings::inventory_scale)
// von selbst stimmt -- eine feste Pixelposition laege bei jeder anderen Skalierung daneben.
pub fn inventory_cursor_spawn(layout: &ScreenLayout) -> Option<(f32, f32)> {
    let slot = layout.slots.iter().find(|s| s.index == PLAYER_MAIN_START + 4)?;
    Some((slot.x + slot.size / 2.0, slot.y + slot.size / 2.0))
}

// Aufteilung eines Kisten-artigen Containers (Vanillas GenericContainerScreen): `rows`
// Reihen zu je 9 Slots ueber dem Spielerinventar.
fn container_layout(screen: (f32, f32), scale: f32, slot_count: usize, screen_type: i32) -> ScreenLayout {
    // Die Reihen kommen aus dem TYP, wo er sie festlegt -- nicht aus der Slotzahl: ein
    // abgeschnittener Inhalt (unlesbares Item, s. packets::read_container_content) ergab
    // sonst eine Kiste mit einer Reihe, in der alles verrutscht lag.
    let rows = container_rows(screen_type).unwrap_or_else(|| (slot_count.saturating_sub(36)) / 9).clamp(1, 6);
    let bg_gui_h = 114.0 + rows as f32 * SLOT_PITCH;
    let bg_w = INVENTORY_SHEET_W * scale;
    let bg_h = bg_gui_h * scale;
    let bg_x = ((screen.0 - bg_w) / 2.0).floor();
    let bg_y = ((screen.1 - bg_h) / 2.0).floor();
    let at = |index: usize, gx: f32, gy: f32| SlotRect {
        index,
        x: bg_x + gx * scale,
        y: bg_y + gy * scale,
        size: ICON_SIZE * scale,
        pad: (SLOT_PITCH - ICON_SIZE) / 2.0 * scale,
    };

    let container_slots = rows * 9;
    let mut slots = Vec::with_capacity(slot_count);
    for i in 0..container_slots {
        slots.push(at(i, 8.0 + (i % 9) as f32 * SLOT_PITCH, 18.0 + (i / 9) as f32 * SLOT_PITCH));
    }
    // Vanillas GenericContainerScreenHandler: das Spielerinventar beginnt bei
    // `18 + rows * 18 + 13`, die Hotbar 58 darunter (ScreenHandler.addPlayerSlots).
    // Hier stand vorher
    // bg_gui_h - 82 bzw. - 24, und das ist genau einen Pixel zu tief.
    let inv_top = 18.0 + rows as f32 * SLOT_PITCH + 13.0;
    for i in 0..27 {
        slots.push(at(container_slots + i, 8.0 + (i % 9) as f32 * SLOT_PITCH, inv_top + (i / 9) as f32 * SLOT_PITCH));
    }
    for i in 0..9 {
        slots.push(at(container_slots + 27 + i, 8.0 + i as f32 * SLOT_PITCH, inv_top + 58.0));
    }

    // generic_54.png: oben der Containerteil (so viele Reihen wie noetig), darunter der
    // feste Spielerinventar-Teil ab Sheet-Zeile 126.
    let top_h = 17 + rows as u32 * 18;
    ScreenLayout {
        bg_x,
        bg_y,
        bg_w,
        bg_h,
        sheet: "container/generic_54",
        parts: vec![((0, 0, 176, top_h), 0.0), ((0, 126, 176, 96), top_h as f32)],
        slots,
        scale,
        tooltip_divider_x: bg_x + bg_w / 2.0,
        // Kisten und Co. zeigen keine Spielerfigur -- nur das Spielerinventar.
        doll: None,
    }
}

// Aufteilung fuer den gerade offenen Bildschirm.
// `scale` kommt vom Aufrufer (Settings::inventory_scale ueber App::gui_scale_of), nicht mehr
// aus gui_scale() -- sonst liesse sich die feste Skalierung aus den Einstellungen nicht
// durchreichen.
// Wie viele Container-Reihen ein Bildschirmtyp hat, wo das feststeht (Menu-Registry,
// Reihenfolge der register()-Aufrufe in ScreenHandlerType): generic_9x1..9x6 = 0..5,
// shulker_box = 20 (drei Reihen).
pub fn container_rows(screen_type: i32) -> Option<usize> {
    match screen_type {
        0..=5 => Some(screen_type as usize + 1),
        20 => Some(3),
        _ => None,
    }
}

// Menue-Typen mit eigenem Bildschirm (Registry-IDs aus MenuType, s. container_rows).
pub const MENU_ANVIL: i32 = 8;
pub const MENU_BLAST_FURNACE: i32 = 10;
pub const MENU_CRAFTING: i32 = 12;
pub const MENU_ENCHANTMENT: i32 = 13;
pub const MENU_FURNACE: i32 = 14;
pub const MENU_GRINDSTONE: i32 = 15;
pub const MENU_LECTERN: i32 = 17;
pub const MENU_MERCHANT: i32 = 19;
pub const MENU_SMOKER: i32 = 22;

// Handelsbildschirm (MerchantScreen): 276 breit statt 176, links die Angebotsliste, rechts
// zwei Bezahlslots und das Ergebnis; das Spielerinventar ist um 100 nach rechts gerueckt.
pub const MERCHANT_SHEET_W: f32 = 276.0;
const MERCHANT_SLOTS: [(f32, f32); 3] = [(136.0, 37.0), (162.0, 37.0), (220.0, 37.0)];
// Sieben Angebotsknoepfe (88x20) ab GUI (5, 18), je 20 tiefer; Schieber 6x27 bei x 94 auf
// einer 139 hohen Bahn.
pub const MERCHANT_VISIBLE_OFFERS: usize = 7;
pub const MERCHANT_SCROLL_TRACK: f32 = 139.0;

// Werkbank (CraftingScreenHandler): Ergebnis bei (124, 35), das 3x3-Raster ab (30, 17).
const CRAFTING_SLOTS: [(f32, f32); 10] = [
    (124.0, 35.0),
    (30.0, 17.0),
    (48.0, 17.0),
    (66.0, 17.0),
    (30.0, 35.0),
    (48.0, 35.0),
    (66.0, 35.0),
    (30.0, 53.0),
    (48.0, 53.0),
    (66.0, 53.0),
];
// Ofen, Schmelzofen, Raeucherofen (AbstractFurnaceScreenHandler): Zutat, Brennstoff, Ergebnis.
const FURNACE_SLOTS: [(f32, f32); 3] = [(56.0, 17.0), (56.0, 53.0), (116.0, 35.0)];

// Zaubertisch, Amboss und Schleifstein: je ein eigenes 176x166-Sheet mit den Containerslots
// an Vanillas Stellen (EnchantmentMenu, AnvilMenu, GrindstoneMenu) und dem Spielerinventar
// darunter bei (8, 84) wie im normalen Inventar. Vorher bekamen sie die Kisten-Aufteilung --
// die Slots lagen dann nicht auf den gemalten Kaesten.
fn special_layout(screen: (f32, f32), scale: f32, screen_type: i32) -> Option<ScreenLayout> {
    let (sheet, eigene): (&'static str, &[(f32, f32)]) = match screen_type {
        MENU_ENCHANTMENT => ("container/enchanting_table", &[(15.0, 47.0), (35.0, 47.0)]),
        MENU_ANVIL => ("container/anvil", &[(27.0, 47.0), (76.0, 47.0), (134.0, 47.0)]),
        MENU_GRINDSTONE => ("container/grindstone", &[(49.0, 19.0), (49.0, 40.0), (129.0, 34.0)]),
        MENU_CRAFTING => ("container/crafting_table", &CRAFTING_SLOTS),
        MENU_FURNACE => ("container/furnace", &FURNACE_SLOTS),
        MENU_BLAST_FURNACE => ("container/blast_furnace", &FURNACE_SLOTS),
        MENU_SMOKER => ("container/smoker", &FURNACE_SLOTS),
        MENU_MERCHANT => ("container/villager", &MERCHANT_SLOTS),
        _ => return None,
    };
    let (sheet_w, inv_x) = if screen_type == MENU_MERCHANT { (MERCHANT_SHEET_W, 108.0) } else { (INVENTORY_SHEET_W, 8.0) };
    let bg_w = sheet_w * scale;
    let bg_h = INVENTORY_SHEET_H * scale;
    let bg_x = ((screen.0 - bg_w) / 2.0).floor();
    let bg_y = ((screen.1 - bg_h) / 2.0).floor();
    let at = |index: usize, gx: f32, gy: f32| SlotRect {
        index,
        x: bg_x + gx * scale,
        y: bg_y + gy * scale,
        size: ICON_SIZE * scale,
        pad: (SLOT_PITCH - ICON_SIZE) / 2.0 * scale,
    };
    let mut slots: Vec<SlotRect> = eigene.iter().enumerate().map(|(i, &(gx, gy))| at(i, gx, gy)).collect();
    let n = eigene.len();
    for i in 0..27 {
        slots.push(at(n + i, inv_x + (i % 9) as f32 * SLOT_PITCH, 84.0 + (i / 9) as f32 * SLOT_PITCH));
    }
    for i in 0..9 {
        slots.push(at(n + 27 + i, inv_x + i as f32 * SLOT_PITCH, 142.0));
    }
    Some(ScreenLayout {
        bg_x,
        bg_y,
        bg_w,
        bg_h,
        sheet,
        parts: vec![((0, 0, sheet_w as u32, INVENTORY_SHEET_H as u32), 0.0)],
        slots,
        scale,
        tooltip_divider_x: bg_x + bg_w / 2.0,
        // Der Zaubertisch zeichnet links oben sein Buch (GUI (14, 14) bis (52, 45), Massstab
        // 40) -- dieselbe 3D-Bahn wie die Spielerfigur im Inventar.
        doll: (screen_type == MENU_ENCHANTMENT).then_some(DollRect {
            x0: bg_x + 14.0 * scale,
            y0: bg_y + 14.0 * scale,
            x1: bg_x + 52.0 * scale,
            y1: bg_y + 45.0 * scale,
            pixels_per_meter: 40.0 * scale,
        }),
    })
}

// Handelsbildschirm: Knopf `i` (0..7) der Angebotsliste in Bildschirmpixeln.
pub fn merchant_button_rect(layout: &ScreenLayout, i: usize) -> (f32, f32, f32, f32) {
    let s = layout.scale;
    (layout.bg_x + 5.0 * s, layout.bg_y + (18.0 + 20.0 * i as f32) * s, 88.0 * s, 20.0 * s)
}

// Handelsbildschirm: die Schieberbahn rechts der Liste (6 breit, 139 hoch ab GUI (94, 18)).
pub fn merchant_scroll_rect(layout: &ScreenLayout) -> (f32, f32, f32, f32) {
    let s = layout.scale;
    (layout.bg_x + 94.0 * s, layout.bg_y + 18.0 * s, 6.0 * s, MERCHANT_SCROLL_TRACK * s)
}

// Zaubertisch: die drei Optionen (EnchantmentScreen, 108x19 ab GUI (60, 14), je 19 tiefer),
// in Bildschirmpixeln.
pub fn enchant_option_rect(layout: &ScreenLayout, i: usize) -> (f32, f32, f32, f32) {
    let s = layout.scale;
    (layout.bg_x + 60.0 * s, layout.bg_y + (14.0 + 19.0 * i as f32) * s, 108.0 * s, 19.0 * s)
}

pub fn screen_layout(inventory: &Inventory, screen: (f32, f32), scale: f32) -> ScreenLayout {
    if let Some(layout) = inventory.open.as_ref().filter(|c| !c.slots.is_empty()).and_then(|c| special_layout(screen, scale, c.screen_type)) {
        return layout;
    }
    match &inventory.open {
        // Ein Container, dessen Slotzahl nicht ins 9er-Raster passt (Ofen, Werkbank, ...),
        // bekommt bewusst dieselbe Kisten-Aufteilung: die Slots sitzen dann nicht exakt auf
        // den gemalten Kaesten, sind aber vollstaendig sichtbar und bedienbar. Eigene
        // Aufteilungen je Containertyp waeren ein Dutzend weiterer Sheets.
        Some(container) if !container.slots.is_empty() => container_layout(screen, scale, container.slots.len(), container.screen_type),
        Some(_) => player_screen_layout(screen, scale),
        None => player_screen_layout(screen, scale),
    }
}

#[cfg(test)]
mod special_screen_tests {
    use super::*;

    #[test]
    // Amboss: Eingaben bei (27, 47) und (76, 47), Ergebnis (134, 47), das Spielerinventar
    // beginnt mit Slot 3 bei (8, 84) -- AnvilMenu/ItemCombinerMenu.
    fn amboss_slots_liegen_auf_vanillas_kaesten() {
        let mut inv = Inventory::default();
        inv.open_screen(4, MENU_ANVIL, "Amboss".into());
        inv.set_content(4, 1, vec![None; 39]);
        let layout = screen_layout(&inv, (1920.0, 1080.0), 2.0);
        assert_eq!(layout.sheet, "container/anvil");
        let gui = |slot: usize| ((layout.slots[slot].x - layout.bg_x) / 2.0, (layout.slots[slot].y - layout.bg_y) / 2.0);
        assert_eq!(gui(0), (27.0, 47.0));
        assert_eq!(gui(2), (134.0, 47.0));
        assert_eq!(layout.slots[3].index, 3);
        assert_eq!(gui(3), (8.0, 84.0));
        assert_eq!(layout.slots.len(), 39);
    }

    // Werkbank: Ergebnis (124, 35), Raster ab (30, 17), Spielerinventar ab Slot 10 bei (8, 84).
    #[test]
    fn werkbank_slots_liegen_auf_vanillas_kaesten() {
        let mut inv = Inventory::default();
        inv.open_screen(3, MENU_CRAFTING, "Werkbank".into());
        inv.set_content(3, 1, vec![None; 46]);
        let layout = screen_layout(&inv, (1920.0, 1080.0), 2.0);
        assert_eq!(layout.sheet, "container/crafting_table");
        let gui = |slot: usize| ((layout.slots[slot].x - layout.bg_x) / 2.0, (layout.slots[slot].y - layout.bg_y) / 2.0);
        assert_eq!(gui(0), (124.0, 35.0));
        assert_eq!(gui(1), (30.0, 17.0));
        assert_eq!(gui(9), (66.0, 53.0));
        assert_eq!(gui(10), (8.0, 84.0));
        assert_eq!(gui(45), (8.0 + 8.0 * 18.0, 142.0));
        assert_eq!(layout.slots.len(), 46);
    }

    // Handel: Bezahlslots (136, 37) und (162, 37), Ergebnis (220, 37), das Spielerinventar ab
    // Slot 3 bei (108, 84) -- MerchantScreenHandler, Fenster 276 breit.
    #[test]
    fn haendler_slots_und_liste() {
        let mut inv = Inventory::default();
        inv.open_screen(5, MENU_MERCHANT, "Bauer".into());
        inv.set_content(5, 1, vec![None; 39]);
        let layout = screen_layout(&inv, (1920.0, 1080.0), 2.0);
        assert_eq!(layout.sheet, "container/villager");
        assert_eq!(layout.bg_w, 552.0);
        let gui = |slot: usize| ((layout.slots[slot].x - layout.bg_x) / 2.0, (layout.slots[slot].y - layout.bg_y) / 2.0);
        assert_eq!(gui(0), (136.0, 37.0));
        assert_eq!(gui(2), (220.0, 37.0));
        assert_eq!(gui(3), (108.0, 84.0));
        assert_eq!(gui(38), (108.0 + 8.0 * 18.0, 142.0));
        assert_eq!(merchant_button_rect(&layout, 1).1 - layout.bg_y, 76.0);

        let angebot = protocol::packets::TradeOffer {
            cost_a: ItemStack::new(1, 1),
            cost_b: None,
            result: None,
            out_of_stock: false,
            uses: 0,
            max_uses: 1,
            xp: 0,
            special_price: 0,
            price_multiplier: 0.0,
            demand: 0,
        };
        let angebote = |n: usize| protocol::packets::MerchantOffers {
            sync_id: 5,
            offers: vec![angebot.clone(); n],
            level: 2,
            xp: 20,
            show_progress: true,
            can_restock: true,
        };
        inv.set_merchant_offers(angebote(10));
        let st = &mut inv.open.as_mut().unwrap().merchant;
        assert_eq!(st.max_scroll(), 3);
        assert_eq!(st.scroller_offset(), Some(0.0));
        st.scroll_by(-5.0);
        assert_eq!(st.scroll, 3);
        assert_eq!(st.scroller_offset(), Some(113.0));
        st.drag_to(0.0);
        assert_eq!(st.scroll, 0);
        st.drag_to(139.0);
        assert_eq!(st.scroll, 3);
        st.selected = 9;
        // Kuerzere Liste: Auswahl und Scrollstand rutschen mit.
        inv.set_merchant_offers(angebote(4));
        let st = &inv.open.as_ref().unwrap().merchant;
        assert_eq!((st.selected, st.scroll, st.scroller_offset()), (3, 0, None));
    }

    #[test]
    fn zaubertisch_merkt_sich_kosten_und_hinweise() {
        let mut inv = Inventory::default();
        inv.open_screen(2, MENU_ENCHANTMENT, String::new());
        inv.set_content(2, 1, vec![None; 38]);
        inv.set_data(2, 1, 12);
        inv.set_data(2, 5, -1);
        inv.set_data(9, 0, 99);
        let c = inv.open.as_ref().unwrap();
        assert_eq!(c.data[1], 12);
        assert_eq!(c.data[5], -1);
        assert_eq!(c.data[0], 0, "anderer Container zählt nicht");
        let layout = screen_layout(&inv, (1920.0, 1080.0), 2.0);
        assert_eq!(layout.sheet, "container/enchanting_table");
        assert_eq!(layout.slots.len(), 38);
    }
}

#[cfg(test)]
mod cursor_spawn_tests {
    use super::*;

    #[test]
    fn zeiger_steht_mittig_im_mittleren_slot_der_obersten_reihe() {
        for scale in [1.0, 2.0, 3.0, 4.5] {
            let layout = player_screen_layout((1920.0, 1080.0), scale);
            let (x, y) = inventory_cursor_spawn(&layout).expect("Slot vorhanden");
            // Vanillas Raster: Spalte 4 bei GUI-x 8 + 4*18, oberste Reihe bei GUI-y 84.
            assert!((x - (layout.bg_x + (8.0 + 4.0 * 18.0 + 8.0) * scale)).abs() < 1e-3, "x bei Skala {scale}");
            assert!((y - (layout.bg_y + (84.0 + 8.0) * scale)).abs() < 1e-3, "y bei Skala {scale}");
            assert_eq!(layout.slot_at(x, y), Some(PLAYER_MAIN_START + 4));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Vanillas GenericContainerScreenHandler setzt das Spielerinventar auf
    // `18 + rows * 18 + 13` und die Hotbar 58 darunter. Vorher lag beides einen Pixel
    // tiefer.
    #[test]
    fn spielerinventar_liegt_auf_vanillas_zeile() {
        for (rows, screen_type) in [(3usize, 2i32), (6, 5)] {
            let layout = container_layout((1920.0, 1080.0), 1.0, rows * 9 + 36, screen_type);
            let gui_y = |slot: usize| layout.slots[slot].y - layout.bg_y;
            let erste_reihe = 18.0 + rows as f32 * SLOT_PITCH + 13.0;
            assert_eq!(gui_y(rows * 9), erste_reihe, "{rows} Reihen: Inventar-Oberkante");
            assert_eq!(gui_y(rows * 9 + 27), erste_reihe + 58.0, "{rows} Reihen: Hotbar");
            // Der Containerteil selbst beginnt unveraendert bei 18.
            assert_eq!(gui_y(0), 18.0);
        }
    }

    // Abgeschnittener Inhalt einer 9x5-Kiste (nur 28 Slots lesbar): fuenf Reihen bleiben,
    // der Rest wird aufgefuellt, und der Spielerteil kommt aus dem bekannten Inventar.
    #[test]
    fn abgeschnittener_inhalt_behaelt_reihen_und_hotbar() {
        let mut inv = Inventory::default();
        let schwert = ItemStack::new(7, 1);
        inv.player[PLAYER_HOTBAR_START] = Some(schwert.clone());
        inv.open_screen(3, 4, "Queue Duels".into());
        inv.set_content(3, 1, vec![None; 28]);
        let c = inv.open.as_ref().unwrap();
        assert_eq!(c.slots.len(), 45 + 36);
        assert_eq!(c.slots[45 + 27], Some(schwert), "erster Hotbar-Platz aus dem Spielerinventar");
        let layout = screen_layout(&inv, (1920.0, 1080.0), 3.0);
        assert_eq!(layout.slots.len(), 81);
    }

    // Die Slotmitte liegt im Slot und trifft ihn wieder (dorthin springt der Zeiger nach
    // dem Ablegen).
    #[test]
    fn slotmitte_trifft_den_eigenen_slot() {
        let layout = player_screen_layout((1920.0, 1080.0), 3.0);
        for slot in &layout.slots {
            let (x, y) = layout.slot_center(slot.index).unwrap();
            assert_eq!(layout.slot_at(x, y), Some(slot.index));
        }
        assert_eq!(layout.slot_center(9999), None);
    }

    #[test]
    fn spielerinventar_hat_alle_slots_genau_einmal() {
        let layout = player_screen_layout((1920.0, 1080.0), 3.0);
        let mut indices: Vec<usize> = layout.slots.iter().map(|s| s.index).collect();
        indices.sort_unstable();
        indices.dedup();
        assert_eq!(indices.len(), PLAYER_SLOT_COUNT);
        assert_eq!(layout.slots.len(), PLAYER_SLOT_COUNT);
    }

    // Der eigentliche Bug: zwischen zwei Slots lagen 2 Pixel tote Zone, ein Klick dorthin
    // galt als "daneben" und warf den Stapel an der Maus weg. Jeder Punkt zwischen zwei
    // benachbarten Slots muss jetzt einem von beiden gehoeren.
    #[test]
    fn zwischen_zwei_slots_gibt_es_keine_luecke_mehr() {
        let layout = player_screen_layout((1920.0, 1080.0), 3.0);
        let first = layout.slots.iter().find(|s| s.index == PLAYER_MAIN_START).copied().unwrap();
        let second = layout.slots.iter().find(|s| s.index == PLAYER_MAIN_START + 1).copied().unwrap();
        let below = layout.slots.iter().find(|s| s.index == PLAYER_MAIN_START + 9).copied().unwrap();

        // Waagerecht zwischen Slot 0 und 1 jeden Pixel abklappern ...
        let mut x = first.x;
        while x < second.x {
            assert!(layout.slot_at(x, first.y + 1.0).is_some(), "Lücke bei x={x}");
            x += 1.0;
        }
        // ... und senkrecht zwischen Reihe 1 und 2.
        let mut y = first.y;
        while y < below.y {
            assert!(layout.slot_at(first.x + 1.0, y).is_some(), "Lücke bei y={y}");
            y += 1.0;
        }
    }

    // Der Tooltip steht seitlich NEBEN dem Fenster (verdeckt also nie ein Item), aber auf
    // der Hoehe des Items, ueber dem die Maus steht.
    // Das Zittern: startet in der Ruhelage, schlaegt nach beiden Seiten aus, klingt ab und
    // ist nach SHAKE_SECONDS vorbei.
    #[test]
    fn zittern_klingt_ab_und_endet() {
        assert_eq!(shake_offset(0.0), 0.0);
        assert!(shake_offset(0.025) > 1.0, "erster Ausschlag nach rechts");
        assert!(shake_offset(0.075) < -1.0, "dann nach links");
        assert!(shake_offset(0.225).abs() < shake_offset(0.025).abs(), "klingt ab");
        assert_eq!(shake_offset(SHAKE_SECONDS), 0.0);
        assert_eq!(shake_offset(-0.1), 0.0);
    }

    // Gesperrt heisst: nichts aendert sich, der Cursor bleibt, wie er ist.
    #[test]
    fn gesperrter_klick_aendert_nichts() {
        let p = locked_prediction(None);
        assert!(p.modified.is_empty() && p.cursor.is_none() && p.hashable);
        let cursor = ItemStack::new(3, 1);
        let p = locked_prediction(Some(&cursor));
        assert_eq!(p.cursor.as_ref().map(|c| c.item_id), Some(3));
    }

    #[test]
    fn tooltip_steht_neben_dem_fenster_auf_hoehe_des_items() {
        let layout = player_screen_layout((1920.0, 1080.0), 3.0);
        let offhand = layout.slots.iter().find(|s| s.index == PLAYER_OFFHAND).copied().unwrap();
        let craft = layout.slots.iter().find(|s| s.index == 1).copied().unwrap();
        let main = layout.slots.iter().find(|s| s.index == PLAYER_MAIN_START).copied().unwrap();
        let hotbar = layout.slots.iter().find(|s| s.index == PLAYER_HOTBAR_START + 8).copied().unwrap();

        let (left_x, left_y, right_aligned) = layout.tooltip_anchor(&offhand);
        assert!(right_aligned, "links der Linie -> rechtsbündig links neben dem Fenster");
        assert!(left_x <= layout.bg_x, "Tooltip liegt links AUSSERHALB des Fensters");
        assert_eq!(left_y, offhand.y + offhand.size / 2.0, "Höhe folgt dem Item");

        let (right_x, _, right_aligned) = layout.tooltip_anchor(&craft);
        assert!(!right_aligned, "rechts der Linie -> linksbündig rechts neben dem Fenster");
        assert!(right_x >= layout.bg_x + layout.bg_w, "Tooltip liegt rechts AUSSERHALB des Fensters");

        // Die SEITE ist fest: andere Slots derselben Seite haben dieselbe x-Position, nur
        // eine andere Hoehe.
        let (main_x, main_y, _) = layout.tooltip_anchor(&main);
        assert_eq!(main_x, left_x);
        assert_ne!(main_y, left_y);
        let (hotbar_x, hotbar_y, _) = layout.tooltip_anchor(&hotbar);
        assert_eq!(hotbar_x, right_x);
        assert_eq!(hotbar_y, hotbar.y + hotbar.size / 2.0);
    }

    // Weggeworfen wird nur ausserhalb des Fensters -- innen (z.B. am Rand ueber den Slots)
    // darf ein Klick nichts ausloesen.
    #[test]
    fn wegwerfen_nur_ausserhalb_des_fensters() {
        let layout = player_screen_layout((1920.0, 1080.0), 3.0);
        assert!(!layout.is_outside(layout.bg_x + 2.0, layout.bg_y + 2.0), "linke obere Ecke ist INNEN");
        assert!(!layout.is_outside(layout.bg_x + layout.bg_w - 1.0, layout.bg_y + layout.bg_h - 1.0));
        assert!(layout.is_outside(layout.bg_x - 1.0, layout.bg_y + 10.0));
        assert!(layout.is_outside(layout.bg_x + layout.bg_w, layout.bg_y + 10.0));
        assert!(layout.is_outside(10.0, 10.0));
    }

    #[test]
    fn kiste_mit_27_slots_hat_drei_reihen_plus_spielerinventar() {
        let layout = container_layout((1920.0, 1080.0), 3.0, 27 + 36, 2);
        assert_eq!(layout.slots.len(), 27 + 36);
        // Erster Container-Slot oben links, erster Spieler-Slot deutlich darunter.
        assert!(layout.slots[27].y > layout.slots[0].y);
    }

    #[test]
    fn container_inhalt_spiegelt_das_spielerinventar_in_die_hotbar() {
        let mut inv = Inventory {
            open: Some(OpenContainer { sync_id: 1, screen_type: 2, title: String::new(), slots: Vec::new(), revision: 0, data: Vec::new(), merchant: MerchantState::default() }),
            ..Default::default()
        };
        let mut slots = vec![None; 27 + 36];
        slots[27 + 27] = Some(ItemStack::new(936, 1)); // erster Hotbar-Slot
        inv.set_content(1, 5, slots);
        assert_eq!(inv.hotbar(0).map(|s| s.item_id), Some(936));
    }

    fn plain_inventory() -> Inventory {
        Inventory::default()
    }

    // Buendel: Linksklick legt ein (statt zu tauschen), Rechtsklick nimmt heraus (statt zu
    // teilen) -- in beide Richtungen, mit dem Buendel an der Maus oder im Platz.
    #[test]
    fn buendel_klicks_legen_ein_und_nehmen_heraus() {
        let id = |n: &str| world::item_registry::item_id_by_name(n).unwrap();
        let platz = (PLAYER_MAIN_START + 2) as i16;
        let pickup = protocol::packets::CLICK_MODE_PICKUP;
        let mut inv = plain_inventory();
        inv.player[PLAYER_MAIN_START + 2] = Some(ItemStack::new(id("bundle"), 1));
        inv.cursor = Some(ItemStack::new(id("stone"), 70));
        assert_eq!(inv.bundle_click_sound(platz, 0, pickup), Some("item.bundle.insert"));
        let p = inv.predict_click(platz, 0, pickup).unwrap();
        assert!(!p.hashable, "der Inhalt ist eine gesetzte Component");
        inv.apply_prediction(&p);
        assert_eq!(inv.cursor.as_ref().map(|c| c.count), Some(6), "64 passen hinein");
        let b = inv.player[PLAYER_MAIN_START + 2].clone().unwrap();
        assert_eq!(world::bundle::weight(&b), world::bundle::Fraction::ONE);
        // Voll: nichts passt mehr, der Klick tauscht trotzdem nicht.
        assert_eq!(inv.bundle_click_sound(platz, 0, pickup), Some("item.bundle.insert_fail"));
        // Rechtsklick mit leerer Hand: der Eintrag an die Maus.
        inv.cursor = None;
        inv.apply_prediction(&inv.predict_click(platz, 1, pickup).unwrap());
        assert_eq!(inv.cursor.as_ref().map(|c| (c.item_id, c.count)), Some((id("stone"), 64)));
        assert!(world::bundle::contents(inv.player[PLAYER_MAIN_START + 2].as_ref().unwrap()).is_empty());
        // Buendel an der Maus, Linksklick auf Steine im Platz: die Steine hinein.
        let steine = inv.cursor.take();
        inv.cursor = inv.player[PLAYER_MAIN_START + 2].take();
        inv.player[PLAYER_MAIN_START + 2] = steine;
        inv.apply_prediction(&inv.predict_click(platz, 0, pickup).unwrap());
        assert!(inv.player[PLAYER_MAIN_START + 2].is_none());
        assert_eq!(world::bundle::contents(inv.cursor.as_ref().unwrap()).len(), 1);
        // Und Rechtsklick auf den leeren Platz: wieder heraus.
        inv.apply_prediction(&inv.predict_click(platz, 1, pickup).unwrap());
        assert_eq!(inv.player[PLAYER_MAIN_START + 2].as_ref().map(|s| s.count), Some(64));
        // Ohne Buendel bleibt alles beim Alten.
        inv.cursor = Some(ItemStack::new(id("dirt"), 1));
        assert_eq!(inv.bundle_click_sound(platz, 0, pickup), None);
    }

    // Offhand-Taste ueber einem Slot: Tausch mit der Zweithand (Knopf 40) -- so legt man ein
    // Totem aus dem Inventar nach.
    #[test]
    fn offhand_taste_tauscht_mit_der_zweithand() {
        let mut inv = plain_inventory();
        inv.player[PLAYER_MAIN_START + 4] = Some(ItemStack::new(7, 1));
        let p = inv.predict_click((PLAYER_MAIN_START + 4) as i16, OFFHAND_SWAP_BUTTON, protocol::packets::CLICK_MODE_SWAP).unwrap();
        inv.apply_prediction(&p);
        assert!(inv.player[PLAYER_MAIN_START + 4].is_none());
        assert_eq!(inv.player[PLAYER_OFFHAND].as_ref().map(|s| s.item_id), Some(7));
    }

    // Zifferntaste ueber einem Slot: der Tausch muss SOFORT lokal stehen, nicht erst nach der
    // Serverantwort, sonst wirken Hotbar-Swaps mit der Taste langsam.
    #[test]
    fn zifferntaste_tauscht_mit_dem_hotbar_platz() {
        let mut inv = plain_inventory();
        inv.player[PLAYER_MAIN_START] = Some(ItemStack::new(1, 12));
        inv.player[PLAYER_HOTBAR_START + 3] = Some(ItemStack::new(2, 5));

        let p = inv.predict_click(PLAYER_MAIN_START as i16, 3, protocol::packets::CLICK_MODE_SWAP).unwrap();
        assert!(p.hashable, "zwei gewöhnliche Stapel sind hashbar");
        assert!(p.cursor.is_none(), "Tauschen lässt den Zeiger unberührt");
        inv.apply_prediction(&p);
        assert_eq!(inv.player[PLAYER_MAIN_START].as_ref().map(|s| (s.item_id, s.count)), Some((2, 5)));
        assert_eq!(inv.player[PLAYER_HOTBAR_START + 3].as_ref().map(|s| (s.item_id, s.count)), Some((1, 12)));

        // Leerer Hotbar-Platz: der Stapel wandert hinueber, der Quellslot bleibt leer.
        let p = inv.predict_click(PLAYER_MAIN_START as i16, 8, protocol::packets::CLICK_MODE_SWAP).unwrap();
        inv.apply_prediction(&p);
        assert!(inv.player[PLAYER_MAIN_START].is_none());
        assert_eq!(inv.player[PLAYER_HOTBAR_START + 8].as_ref().map(|s| s.item_id), Some(2));

        // Beide leer -- Vanilla tut nichts, also auch keine Vorhersage.
        assert!(inv.predict_click(PLAYER_MAIN_START as i16, 1, protocol::packets::CLICK_MODE_SWAP).is_none());
        // Ruestungsslot: eigene Annahmeregeln, bleibt dem Server ueberlassen.
        assert!(inv.predict_click(PLAYER_ARMOR_START as i16, 3, protocol::packets::CLICK_MODE_SWAP).is_none());
    }

    // Doppelklick mit vollem Zeiger sammelt gleichartige Stapel ein -- angebrochene zuerst,
    // volle erst im zweiten Durchgang (ScreenHandler.internalOnSlotClick, PICKUP_ALL).
    #[test]
    fn doppelklick_sammelt_angebrochene_stapel_zuerst() {
        let mut inv = plain_inventory();
        inv.cursor = Some(ItemStack::new(1, 1));
        inv.player[PLAYER_MAIN_START] = Some(ItemStack::new(1, 64)); // voll
        inv.player[PLAYER_MAIN_START + 1] = Some(ItemStack::new(1, 20)); // angebrochen
        inv.player[PLAYER_MAIN_START + 2] = Some(ItemStack::new(9, 30)); // andere Sorte
        let leer = (PLAYER_MAIN_START + 5) as i16;

        let p = inv.predict_click(leer, 0, protocol::packets::CLICK_MODE_PICKUP_ALL).unwrap();
        inv.apply_prediction(&p);
        assert_eq!(inv.cursor.as_ref().map(|s| s.count), Some(64), "Zeiger ist voll");
        assert!(inv.player[PLAYER_MAIN_START + 1].is_none(), "der angebrochene Stapel ging zuerst");
        assert_eq!(inv.player[PLAYER_MAIN_START].as_ref().map(|s| s.count), Some(21), "vom vollen nur der Rest");
        assert_eq!(inv.player[PLAYER_MAIN_START + 2].as_ref().map(|s| s.count), Some(30), "fremde Sorte bleibt");

        // Auf einem BELEGTEN Slot sammelt Vanilla nicht ein.
        inv.cursor = Some(ItemStack::new(1, 1));
        assert!(inv.predict_click(PLAYER_MAIN_START as i16, 0, protocol::packets::CLICK_MODE_PICKUP_ALL).is_none());
    }

    // Pfeile mit Trank tragen immer gesetzte Components. Gleiche Components (gleiche Bytes)
    // werden eingesammelt, andere (anderer Trank) bleiben liegen. Frueher tat der
    // Doppelklick bei Pfeilen mit sanftem Fall gar nichts tat.
    #[test]
    fn doppelklick_sammelt_pfeile_mit_gleichem_trank() {
        let pfeil = |count: i32, trank: u8| {
            let mut s = ItemStack::new(7, count);
            s.has_added_components = true;
            s.added_component_bytes = vec![1, 2, trank];
            s
        };
        let mut inv = plain_inventory();
        inv.cursor = Some(pfeil(3, 30)); // sanfter Fall
        inv.player[PLAYER_MAIN_START] = Some(pfeil(10, 30)); // derselbe Trank
        inv.player[PLAYER_MAIN_START + 1] = Some(pfeil(10, 31)); // anderer Trank
        inv.player[PLAYER_MAIN_START + 2] = Some(ItemStack::new(7, 10)); // Pfeile ohne Trank
        let p = inv.predict_click((PLAYER_MAIN_START + 5) as i16, 0, protocol::packets::CLICK_MODE_PICKUP_ALL).expect("es gibt etwas zu holen");
        inv.apply_prediction(&p);
        assert_eq!(inv.cursor.as_ref().map(|s| s.count), Some(13));
        assert!(inv.player[PLAYER_MAIN_START].is_none());
        assert_eq!(inv.player[PLAYER_MAIN_START + 1].as_ref().map(|s| s.count), Some(10), "anderer Trank bleibt");
        assert_eq!(inv.player[PLAYER_MAIN_START + 2].as_ref().map(|s| s.count), Some(10), "ohne Trank bleibt");
    }

    #[test]
    fn linksklick_nimmt_stapel_auf_und_legt_ihn_ab() {
        let mut inv = plain_inventory();
        inv.player[PLAYER_MAIN_START] = Some(ItemStack::new(1, 64));
        let p = inv.predict_click(PLAYER_MAIN_START as i16, 0, protocol::packets::CLICK_MODE_PICKUP).unwrap();
        assert_eq!(p.cursor.as_ref().map(|s| s.count), Some(64));
        assert_eq!(p.modified, vec![(PLAYER_MAIN_START as i16, None)]);
        inv.apply_prediction(&p);
        assert!(inv.player[PLAYER_MAIN_START].is_none());

        // ... und wieder in einen leeren Slot ablegen
        let target = (PLAYER_MAIN_START + 1) as i16;
        let p = inv.predict_click(target, 0, protocol::packets::CLICK_MODE_PICKUP).unwrap();
        assert!(p.cursor.is_none());
        assert_eq!(p.modified[0].1.as_ref().map(|s| s.count), Some(64));
    }

    #[test]
    fn linksklick_legt_nur_bis_zur_stapelgrenze_zusammen() {
        let mut inv = plain_inventory();
        inv.player[PLAYER_MAIN_START] = Some(ItemStack::new(1, 60)); // 60x Stein im Slot
        inv.cursor = Some(ItemStack::new(1, 20)); // 20x Stein an der Maus
        let p = inv.predict_click(PLAYER_MAIN_START as i16, 0, protocol::packets::CLICK_MODE_PICKUP).unwrap();
        assert_eq!(p.modified[0].1.as_ref().map(|s| s.count), Some(64));
        assert_eq!(p.cursor.as_ref().map(|s| s.count), Some(16)); // Rest bleibt an der Maus
    }

    #[test]
    fn rechtsklick_nimmt_die_haelfte_und_legt_einzeln_ab() {
        let mut inv = plain_inventory();
        inv.player[PLAYER_MAIN_START] = Some(ItemStack::new(1, 7));
        let p = inv.predict_click(PLAYER_MAIN_START as i16, 1, protocol::packets::CLICK_MODE_PICKUP).unwrap();
        assert_eq!(p.cursor.as_ref().map(|s| s.count), Some(4)); // aufgerundet
        assert_eq!(p.modified[0].1.as_ref().map(|s| s.count), Some(3));
        inv.apply_prediction(&p);

        let target = (PLAYER_MAIN_START + 1) as i16;
        let p = inv.predict_click(target, 1, protocol::packets::CLICK_MODE_PICKUP).unwrap();
        assert_eq!(p.modified[0].1.as_ref().map(|s| s.count), Some(1));
        assert_eq!(p.cursor.as_ref().map(|s| s.count), Some(3));
    }

    // Stapel mit gesetzten Components lassen sich nicht exakt hashen -- das betrifft aber NUR
    // die Hash-Felder des Pakets, nicht die Anzeige. Frueher hing beides zusammen und die
    // Vorhersage fiel ganz aus; ausgerechnet bei echter Ausruestung (verzaubert, benannt,
    // beschaedigt) wartete das Inventar dadurch jedes Mal auf die Serverantwort.
    #[test]
    fn stapel_mit_components_wird_lokal_vorhergesagt_aber_nicht_gehasht() {
        let mut inv = plain_inventory();
        let mut enchanted = ItemStack::new(936, 1);
        enchanted.has_added_components = true;
        inv.player[PLAYER_MAIN_START] = Some(enchanted);
        let p = inv.predict_click(PLAYER_MAIN_START as i16, 0, protocol::packets::CLICK_MODE_PICKUP).expect("lokal vorhersagbar");
        assert!(!p.hashable, "kein exakter Hash -- der Server bekommt absichtlich einen abweichenden");
        assert_eq!(p.cursor.as_ref().map(|s| s.item_id), Some(936), "aufgenommen wird trotzdem sofort");
    }

    // Ruestungs-, Craft- und Ergebnisslots haben Annahmeregeln, die wir nicht nachbauen.
    #[test]
    fn sonderslots_werden_nicht_vorhergesagt() {
        let mut inv = plain_inventory();
        inv.cursor = Some(ItemStack::new(1, 1));
        for slot in [PLAYER_CRAFT_RESULT, PLAYER_ARMOR_START] {
            assert!(
                inv.predict_click(slot as i16, 0, protocol::packets::CLICK_MODE_PICKUP).is_none(),
                "Slot {slot} darf nicht vorhergesagt werden"
            );
        }
        // Hauptinventar und Craft-Felder dagegen schon.
        assert!(inv.predict_click(PLAYER_MAIN_START as i16, 0, protocol::packets::CLICK_MODE_PICKUP).is_some());
        assert!(inv.predict_click(1, 0, protocol::packets::CLICK_MODE_PICKUP).is_some());
    }

    // Nur die generischen Kisten-Menues sind reine Lagerplaetze; bei allem anderen (Ofen,
    // Amboss, ...) haelt sich die Vorhersage raus.
    #[test]
    fn nur_generische_container_werden_vorhergesagt() {
        let mut inv = plain_inventory();
        inv.cursor = Some(ItemStack::new(1, 1));
        inv.open = Some(OpenContainer { sync_id: 1, screen_type: 2, title: String::new(), slots: vec![None; 63], revision: 0, data: Vec::new(), merchant: MerchantState::default() });
        assert!(inv.predict_click(0, 0, protocol::packets::CLICK_MODE_PICKUP).is_some());
        if let Some(container) = &mut inv.open {
            container.screen_type = 14; // irgendein Nicht-Kisten-Menue
        }
        assert!(inv.predict_click(0, 0, protocol::packets::CLICK_MODE_PICKUP).is_none());
    }

    // Wegwerfen (Klick neben das Fenster): nur der Cursor aendert sich.
    #[test]
    fn wegwerfen_leert_den_cursor() {
        let mut inv = plain_inventory();
        inv.cursor = Some(ItemStack::new(1, 5));
        let ganz = inv.predict_click(DROP_SLOT, 0, protocol::packets::CLICK_MODE_PICKUP).unwrap();
        assert!(ganz.cursor.is_none());
        assert!(ganz.modified.is_empty());
        let einzeln = inv.predict_click(DROP_SLOT, 1, protocol::packets::CLICK_MODE_PICKUP).unwrap();
        assert_eq!(einzeln.cursor.as_ref().map(|s| s.count), Some(4));
    }

    #[test]
    fn player_slot_mapping_folgt_der_playerinventory_nummerierung() {
        let mut inv = Inventory::default();
        inv.set_player_slot(0, Some(ItemStack::new(1, 1)));
        assert_eq!(inv.hotbar(0).map(|s| s.item_id), Some(1));
        inv.set_player_slot(40, Some(ItemStack::new(2, 1)));
        assert_eq!(inv.player[PLAYER_OFFHAND].as_ref().map(|s| s.item_id), Some(2));
        inv.set_player_slot(39, Some(ItemStack::new(3, 1)));
        assert_eq!(inv.player[PLAYER_ARMOR_START].as_ref().map(|s| s.item_id), Some(3));
    }
}

#[cfg(test)]
mod vorhersage_tests {
    use super::*;

    fn stack(item_id: u32, count: i32) -> ItemStack {
        ItemStack::new(item_id, count)
    }

    fn inventar_mit(slot: usize, inhalt: Option<ItemStack>, cursor: Option<ItemStack>) -> Inventory {
        let mut inv = Inventory::default();
        inv.player[slot] = inhalt;
        inv.cursor = cursor;
        inv
    }

    // Der Alltagsfall: Zeiger voll, Slot voll, anderes Item -> tauschen. Wird der NICHT
    // vorhergesagt, wartet die Anzeige auf den Server und der Tausch fuehlt sich traege an.
    #[test]
    fn tausch_wird_vorhergesagt() {
        let inv = inventar_mit(PLAYER_HOTBAR_START, Some(stack(1, 10)), Some(stack(2, 5)));
        let p = inv.predict_click(PLAYER_HOTBAR_START as i16, 0, protocol::packets::CLICK_MODE_PICKUP);
        let p = p.expect("Tausch muss vorhersagbar sein");
        assert_eq!(p.cursor.as_ref().map(|s| s.item_id), Some(1));
        assert_eq!(p.modified[0].1.as_ref().map(|s| s.item_id), Some(2));
    }

    // Stapel-Verschieben aus der Hotbar landet im Hauptteil (Vanillas PlayerScreenHandler:
    // 36..45 -> 9..36). Ohne Vorhersage wartete jeder Shift-Klick auf den Server.
    #[test]
    fn stapel_verschieben_geht_von_hotbar_in_den_hauptteil() {
        let inv = inventar_mit(PLAYER_HOTBAR_START, Some(stack(1, 10)), None);
        let p = inv
            .predict_click(PLAYER_HOTBAR_START as i16, 0, protocol::packets::CLICK_MODE_QUICK_MOVE)
            .expect("Shift-Klick muss vorhersagbar sein");
        let ziel = p.modified.iter().find(|(slot, _)| *slot != PLAYER_HOTBAR_START as i16).expect("Zielslot");
        assert!((PLAYER_MAIN_START..PLAYER_HOTBAR_START).contains(&(ziel.0 as usize)), "Ziel {} liegt nicht im Hauptteil", ziel.0);
        assert_eq!(ziel.1.as_ref().map(|s| s.count), Some(10));
        // Die Quelle ist danach leer.
        let quelle = p.modified.iter().find(|(slot, _)| *slot == PLAYER_HOTBAR_START as i16).expect("Quellslot");
        assert_eq!(quelle.1, None);
    }

    // Und umgekehrt: aus dem Hauptteil in die Hotbar.
    #[test]
    fn stapel_verschieben_geht_aus_dem_hauptteil_in_die_hotbar() {
        let inv = inventar_mit(PLAYER_MAIN_START, Some(stack(1, 3)), None);
        let p = inv.predict_click(PLAYER_MAIN_START as i16, 0, protocol::packets::CLICK_MODE_QUICK_MOVE).expect("vorhersagbar");
        let ziel = p.modified.iter().find(|(slot, _)| *slot != PLAYER_MAIN_START as i16).expect("Zielslot");
        assert!((PLAYER_HOTBAR_START..PLAYER_OFFHAND).contains(&(ziel.0 as usize)), "Ziel {} nicht in der Hotbar", ziel.0);
    }

    // Erst auf vorhandene gleiche Stapel verteilen, dann erst ein freier Platz -- Vanillas
    // insertItem in zwei Durchgaengen. Andersherum saehe man den Stapel an der falschen
    // Stelle auftauchen und der Server wuerde gleich darauf widersprechen.
    #[test]
    fn stapel_verschieben_fuellt_erst_vorhandene_stapel_auf() {
        let mut inv = inventar_mit(PLAYER_HOTBAR_START, Some(stack(1, 10)), None);
        // Ein Teilstapel derselben Sorte im Hauptteil, weiter hinten.
        inv.player[PLAYER_MAIN_START + 5] = Some(stack(1, 60));
        let p = inv.predict_click(PLAYER_HOTBAR_START as i16, 0, protocol::packets::CLICK_MODE_QUICK_MOVE).expect("vorhersagbar");
        let aufgefuellt = p.modified.iter().find(|(slot, _)| *slot == (PLAYER_MAIN_START + 5) as i16).expect("Teilstapel");
        assert_eq!(aufgefuellt.1.as_ref().map(|s| s.count), Some(64), "bis zur Stapelgrenze auffüllen");
        // Der Rest (10 - 4 = 6) braucht einen freien Platz.
        let rest: i32 = p
            .modified
            .iter()
            .filter(|(slot, _)| *slot != PLAYER_HOTBAR_START as i16 && *slot != (PLAYER_MAIN_START + 5) as i16)
            .filter_map(|(_, s)| s.as_ref().map(|s| s.count))
            .sum();
        assert_eq!(rest, 6);
    }

    // Ist der Zielbereich voll, bleibt der Stapel liegen -- dann darf es KEINE Vorhersage
    // geben, sonst verschwaende das Item lokal und kaeme eine Zehntelsekunde spaeter zurueck.
    #[test]
    fn voller_zielbereich_ergibt_keine_vorhersage() {
        let mut inv = inventar_mit(PLAYER_HOTBAR_START, Some(stack(1, 10)), None);
        for slot in PLAYER_MAIN_START..PLAYER_HOTBAR_START {
            inv.player[slot] = Some(stack(2, 64));
        }
        assert!(inv.predict_click(PLAYER_HOTBAR_START as i16, 0, protocol::packets::CLICK_MODE_QUICK_MOVE).is_none());
    }

    // Ein Stapel mit Components (verzaubert/benannt) ist nicht hashbar -- die Vorhersage
    // muss es trotzdem geben, nur ohne die Hash-Felder im Paket. Frueher fiel sie ganz aus,
    // und gerade echte Ausruestung fuehlte sich dadurch traege an.
    #[test]
    fn nicht_hashbarer_stapel_wird_lokal_trotzdem_vorhergesagt() {
        let mut verzaubert = stack(1, 1);
        verzaubert.has_added_components = true;
        let inv = inventar_mit(PLAYER_HOTBAR_START, Some(verzaubert), Some(stack(2, 1)));
        let p = inv.predict_click(PLAYER_HOTBAR_START as i16, 0, protocol::packets::CLICK_MODE_PICKUP).expect("vorhersagbar");
        assert!(!p.hashable, "mit Components darf nicht gehasht werden");
        assert_eq!(p.cursor.as_ref().map(|s| s.item_id), Some(1), "getauscht wird trotzdem");
    }
}

// --- Verteilen-Ziehen: Vorschau ---------------------------------------------------------

// Wie sich ein gehaltener Stapel auf die ueberfahrenen Slots verteilt.
//
// Vanillas ScreenHandler.calculateStackSize: bei gedrueckter LINKER Taste bekommt jeder Slot
// `floor(gesamt / anzahl)`, bei der rechten genau eins; was nicht untergebracht wird, bleibt
// am Zeiger. Dazu die Annahmeregel aus canInsertItemIntoSlot: ein Slot zaehlt nur mit, wenn
// er leer ist oder denselben Stapel enthaelt, und nie ueber die Stapelgrenze hinaus.
//
// `ziele` ist der Zustand VOR dem Ziehen (Slot + was dort lag), in Ueberfahr-Reihenfolge.
// Bewusst aus dem Vorher-Zustand gerechnet statt fortgeschrieben: kommt ein Slot dazu,
// aendert sich die Aufteilung ALLER vorherigen mit (aus 32+32 wird 21+21+21) -- inkrementell
// muesste man dafuer erst die halbe Verteilung zuruecknehmen.
pub fn drag_preview(
    button: i8,
    cursor: &ItemStack,
    ziele: &[(usize, Option<ItemStack>)],
) -> (Vec<(i16, Option<ItemStack>)>, Option<ItemStack>) {
    let passende: Vec<&(usize, Option<ItemStack>)> =
        ziele.iter().filter(|(_, inhalt)| inhalt.as_ref().is_none_or(|s| s.stackable_with(cursor))).collect();
    if passende.is_empty() {
        return (Vec::new(), Some(cursor.clone()));
    }
    let max = world::item_components::max_stack(cursor);
    let je_slot = match button {
        // Links: gleichmaessig, abgerundet. Rechts: ein Stueck.
        0 => cursor.count / passende.len() as i32,
        _ => 1,
    };

    let mut rest = cursor.count;
    let mut geaendert = Vec::new();
    for (slot, inhalt) in passende {
        if rest == 0 {
            break;
        }
        let vorhanden = inhalt.as_ref().map_or(0, |s| s.count);
        let platz = (max - vorhanden).max(0);
        let menge = je_slot.min(platz).min(rest);
        if menge == 0 {
            continue;
        }
        rest -= menge;
        geaendert.push((*slot as i16, Some(ItemStack { count: vorhanden + menge, ..cursor.clone() })));
    }
    (geaendert, (rest > 0).then(|| ItemStack { count: rest, ..cursor.clone() }))
}

#[cfg(test)]
mod drag_tests {
    use super::*;

    fn stack(item_id: u32, count: i32) -> ItemStack {
        ItemStack::new(item_id, count)
    }

    // Linke Taste teilt gleichmaessig auf und rundet ab -- der Rest bleibt am Zeiger.
    #[test]
    fn links_teilt_gleichmaessig_und_rundet_ab() {
        let (geaendert, rest) = drag_preview(0, &stack(1, 10), &[(3, None), (4, None), (5, None)]);
        assert_eq!(geaendert.len(), 3);
        for (_, inhalt) in &geaendert {
            assert_eq!(inhalt.as_ref().map(|s| s.count), Some(3));
        }
        assert_eq!(rest.map(|s| s.count), Some(1), "10 auf 3 Slots lässt einen übrig");
    }

    // Rechte Taste legt genau ein Stueck je Slot ab.
    #[test]
    fn rechts_legt_ein_stueck_je_slot() {
        let (geaendert, rest) = drag_preview(1, &stack(1, 10), &[(3, None), (4, None)]);
        assert!(geaendert.iter().all(|(_, s)| s.as_ref().map(|s| s.count) == Some(1)));
        assert_eq!(rest.map(|s| s.count), Some(8));
    }

    // Ein Slot mit demselben Item wird AUFGEFUELLT, nicht ueberschrieben.
    #[test]
    fn vorhandener_stapel_wird_aufgefuellt() {
        let (geaendert, _) = drag_preview(1, &stack(1, 5), &[(3, Some(stack(1, 7)))]);
        assert_eq!(geaendert[0].1.as_ref().map(|s| s.count), Some(8));
    }

    // Ein Slot mit einem ANDEREN Item zaehlt gar nicht mit -- weder als Ziel noch als Teiler.
    // Sonst bekaeme jeder Slot zu wenig und der Zeiger behielte zu viel.
    #[test]
    fn fremdes_item_wird_uebersprungen() {
        let (geaendert, rest) = drag_preview(0, &stack(1, 10), &[(3, None), (4, Some(stack(2, 1)))]);
        assert_eq!(geaendert.len(), 1, "nur der leere Slot");
        assert_eq!(geaendert[0].1.as_ref().map(|s| s.count), Some(10), "und der bekommt alles");
        assert_eq!(rest, None);
    }

    // Ein einzelner Slot bekommt bei der linken Taste den GANZEN Stapel -- damit ist ein
    // simpler Klick genau dasselbe wie ein Ziehen ueber einen Slot (so wie in Vanilla).
    #[test]
    fn ein_slot_bekommt_alles() {
        let (geaendert, rest) = drag_preview(0, &stack(1, 42), &[(3, None)]);
        assert_eq!(geaendert[0].1.as_ref().map(|s| s.count), Some(42));
        assert_eq!(rest, None);
    }
}
