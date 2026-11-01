// Passagiere, Bossleisten, Fahrzeuge, Buecher, Handel, Karten und Container.

use super::*;

pub fn read_set_passengers(payload: &[u8]) -> Result<(i32, Vec<i32>), ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let vehicle = r.read_varint()?;
    let count = r.read_varint()?;
    if count < 0 {
        return Err(ReadError);
    }
    let mut ids = Vec::with_capacity(count.min(256) as usize);
    for _ in 0..count {
        ids.push(r.read_varint()?);
    }
    Ok((vehicle, ids))
}

// --- Bossbar ---------------------------------------------------------------------------------
//
// BossBarS2CPacket: UUID, Aktion (VarInt, Reihenfolge von BossBarS2CPacket.Type), dann je
// Aktion ihre Felder. Farbe (pink, blue, red, green, yellow, purple, white) und Stil
// (progress, notched_6/10/12/20) als VarInt-Ordinal, die drei Schalter als Bitmaske
// (1 Himmel verdunkeln, 2 Drachenmusik, 4 Nebel verdichten). ID: Platz 9 in PlayStateFactories.
pub const CB_PLAY_BOSS_EVENT: i32 = 0x09;

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub enum BossEvent {
    Add { name: Vec<crate::nbt::TextSpan>, progress: f32, color: i32, style: i32, flags: u8 },
    Remove,
    Progress(f32),
    Name(Vec<crate::nbt::TextSpan>),
    Style { color: i32, style: i32 },
    Flags(u8),
}

pub fn read_boss_event(payload: &[u8]) -> Result<([u8; 16], BossEvent), ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let uuid = r.read_uuid()?.0;
    let event = match r.read_varint()? {
        0 => {
            let name = read_text_spans(&mut r)?;
            let progress = r.read_f32()?;
            let color = r.read_varint()?;
            let style = r.read_varint()?;
            BossEvent::Add { name, progress, color, style, flags: r.read_u8()? }
        }
        1 => BossEvent::Remove,
        2 => BossEvent::Progress(r.read_f32()?),
        3 => BossEvent::Name(read_text_spans(&mut r)?),
        4 => {
            let color = r.read_varint()?;
            BossEvent::Style { color, style: r.read_varint()? }
        }
        5 => BossEvent::Flags(r.read_u8()?),
        _ => return Err(ReadError),
    };
    Ok((uuid, event))
}

// --- Reiten --------------------------------------------------------------------------------
//
// Lenkt der Client sein Fahrzeug selbst (Boot, gesatteltes Pferd ...), meldet er dessen Stand
// je Tick mit Move Vehicle; der Server antwortet nur, wenn er nicht einverstanden ist, mit
// seinem eigenen Move Vehicle (Stelle, auf die der Client zuruecksetzen soll). IDs aus der
// Registrierungsreihenfolge in PlayStateFactories (1.21.11): serverwaerts direkt hinter
// MOVE_PLAYER_STATUS_ONLY (0x20) MOVE_VEHICLE und PADDLE_BOAT, clientwaerts Platz 55.
pub const SB_PLAY_MOVE_VEHICLE: i32 = 0x21;
pub const SB_PLAY_PADDLE_BOAT: i32 = 0x22;
pub const CB_PLAY_MOVE_VEHICLE: i32 = 0x37;

// VehicleMoveC2SPacket: Vec3d (drei Doubles), Yaw, Pitch (Floats), onGround.
#[must_use] 
pub fn move_vehicle_payload(pos: (f64, f64, f64), yaw_deg: f32, pitch_deg: f32, on_ground: bool) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_MOVE_VEHICLE).write_f64(pos.0).write_f64(pos.1).write_f64(pos.2).write_f32(yaw_deg).write_f32(pitch_deg).write_bool(on_ground);
    w.into_bytes()
}

// BoatPaddleStateC2SPacket: linkes, dann rechtes Ruder in Bewegung.
#[must_use] 
pub fn paddle_boat_payload(left: bool, right: bool) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_PADDLE_BOAT).write_bool(left).write_bool(right);
    w.into_bytes()
}

// VehicleMoveS2CPacket: Vec3d, Yaw, Pitch.
pub fn read_move_vehicle(payload: &[u8]) -> Result<((f64, f64, f64), f32, f32), ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let pos = (r.read_f64()?, r.read_f64()?, r.read_f64()?);
    Ok((pos, r.read_f32()?, r.read_f32()?))
}

pub fn read_remove_entities(payload: &[u8]) -> Result<Vec<i32>, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let count = r.read_varint()?;
    if count < 0 {
        return Err(ReadError);
    }
    let mut ids = Vec::with_capacity(count.min(4096) as usize);
    for _ in 0..count {
        ids.push(r.read_varint()?);
    }
    Ok(ids)
}

// --- Inventar / Container -----------------------------------------------------------------
//
// IDs aus PlayStateFactories des 1.21.11-Jars (Registrierungsreihenfolge = Packet-ID, beim
// S2C-Block +1 wegen des vorangestellten Bundle-Packets 0x00), gegengeprueft an den bereits
// verifizierten Ankern SWING 0x3C (C2S) und SET_HEALTH 0x66 (S2C).
//
// Slots kommen als ItemStack.OPTIONAL_PACKET_CODEC (s. item.rs). Ein unlesbarer Stack
// beendet nur die Liste -- die vorher gelesenen Slots bleiben gueltig.

// InventoryS2CPacket: der komplette Inhalt eines Containers (syncId 0 = Spielerinventar).
pub const CB_PLAY_CONTAINER_SET_CONTENT: i32 = 0x12;
// ScreenHandlerSlotUpdateS2CPacket: ein einzelner Slot.
pub const CB_PLAY_CONTAINER_SET_SLOT: i32 = 0x14;
// SetCursorItemS2CPacket: der Stack an der Maus.
pub const CB_PLAY_SET_CURSOR_ITEM: i32 = 0x5E;
// SetPlayerInventoryS2CPacket: ein Slot im Spielerinventar, unabhaengig vom offenen Screen.
pub const CB_PLAY_SET_PLAYER_INVENTORY: i32 = 0x6A;
// UpdateSelectedSlotS2CPacket: der Server setzt den aktiven Hotbar-Slot.
pub const CB_PLAY_SET_CARRIED_ITEM: i32 = 0x67;
// OpenScreenS2CPacket: Container oeffnen (Kiste, Ofen, ...).
pub const CB_PLAY_OPEN_SCREEN: i32 = 0x39;
// ClientboundOpenBookPacket: das Buch in dieser Hand (0 Haupt-, 1 Nebenhand) zum Lesen oeffnen.
pub const CB_PLAY_OPEN_BOOK: i32 = 0x38;

pub fn read_open_book(payload: &[u8]) -> Result<i32, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    r.read_varint()
}
// CloseScreenS2CPacket: Container schliessen.
pub const CB_PLAY_CONTAINER_CLOSE: i32 = 0x11;

// UpdateSelectedSlotC2SPacket: aktiver Hotbar-Slot (Short!).
pub const SB_PLAY_SET_CARRIED_ITEM: i32 = 0x34;
// ClickSlotC2SPacket.
pub const SB_PLAY_CONTAINER_CLICK: i32 = 0x11;
// CloseHandledScreenC2SPacket.
pub const SB_PLAY_CONTAINER_CLOSE: i32 = 0x12;
// ClientboundContainerSetDataPacket: ein Zahlenwert des offenen Containers (Zaubertisch:
// Kosten/Hinweise, Amboss: Erfahrungskosten, Ofen: Brennzeit, ...).
pub const CB_PLAY_CONTAINER_SET_DATA: i32 = 0x13;
// ServerboundContainerButtonClickPacket: Knopf im Container (Zaubertisch: Option 0..2).
pub const SB_PLAY_CONTAINER_BUTTON_CLICK: i32 = 0x10;
// ServerboundRenameItemPacket: Name im Amboss.
pub const SB_PLAY_RENAME_ITEM: i32 = 0x2F;

// (Container-ID, Index, Wert). Container-ID als VarInt (FriendlyByteBuf.readContainerId),
// Index und Wert als Short.
pub fn read_container_set_data(payload: &[u8]) -> Result<(i32, i16, i16), ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let sync_id = r.read_varint()?;
    let index = r.read_u16()? as i16;
    let value = r.read_u16()? as i16;
    Ok((sync_id, index, value))
}

// --- Handel mit Dorfbewohnern ---
// IDs aus dem packets.json-Report des 1.21.11-Datengenerators.
pub const CB_PLAY_MERCHANT_OFFERS: i32 = 0x32;
pub const SB_PLAY_SELECT_TRADE: i32 = 0x32;

// Ein Angebot (TradeOffer.PACKET_CODEC). Die Preise kommen als Grundpreis; was der Bildschirm
// zeigt, rechnet displayed_cost_a daraus (Nachfrage, Rabatt).
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct TradeOffer {
    pub cost_a: ItemStack,
    pub cost_b: Option<ItemStack>,
    pub result: Option<ItemStack>,
    pub out_of_stock: bool,
    pub uses: i32,
    pub max_uses: i32,
    pub xp: i32,
    pub special_price: i32,
    pub price_multiplier: f32,
    pub demand: i32,
}

impl TradeOffer {
    // Ausverkauft: Vanilla prueft dafuer nur uses >= max_uses (TradeOffer.isDisabled). Das
    // mitgeschickte Flag setzt beim Lesen genau diesen Zustand.
    #[must_use] 
    pub const fn disabled(&self) -> bool {
        self.out_of_stock || self.uses >= self.max_uses
    }

    // Angezeigter erster Preis: Grundpreis plus Nachfrage-Aufschlag (nie negativ) plus
    // Sonderpreis (Rabatt nach Heldentat/Heilung negativ), auf 1..Stapelgroesse begrenzt.
    #[must_use] 
    pub fn displayed_cost_a(&self, max_stack: i32) -> i32 {
        let basis = self.cost_a.count;
        let aufschlag = (((basis * self.demand) as f32 * self.price_multiplier).floor() as i32).max(0);
        (basis + aufschlag + self.special_price).clamp(1, max_stack.max(1))
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct MerchantOffers {
    pub sync_id: i32,
    pub offers: Vec<TradeOffer>,
    pub level: i32,
    pub xp: i32,
    pub show_progress: bool,
    pub can_restock: bool,
}

pub(crate) fn read_trade_offer(r: &mut PacketReader) -> Result<TradeOffer, SlotError> {
    let cost_a = crate::item::read_item_cost(r)?;
    let result = read_optional_slot(r)?;
    let cost_b = if r.read_bool()? { Some(crate::item::read_item_cost(r)?) } else { None };
    let out_of_stock = r.read_bool()?;
    let uses = r.read_i32()?;
    let max_uses = r.read_i32()?;
    let xp = r.read_i32()?;
    let special_price = r.read_i32()?;
    let price_multiplier = r.read_f32()?;
    let demand = r.read_i32()?;
    Ok(TradeOffer { cost_a, cost_b, result, out_of_stock, uses, max_uses, xp, special_price, price_multiplier, demand })
}

// ClientboundMapItemDataPacket: Kartenstand -- Massstab, Sperre, (wenn mitgeschickt) die
// Markierungen und ein Rechteck neuer Farben.
pub const CB_PLAY_MAP_ITEM_DATA: i32 = 0x31;

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct MapDecoration {
    // Index in der Registry map_decoration_type (eingebaut, feste Reihenfolge).
    pub kind: i32,
    // -128..127 je Achse, Kartenmitte 0 (ein Schritt = halber Kartenpixel).
    pub x: i8,
    pub y: i8,
    // Drehung in 16teln einer Umdrehung.
    pub rot: u8,
    pub name: Option<Vec<nbt::TextSpan>>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct MapPatch {
    pub width: u8,
    pub height: u8,
    pub x: u8,
    pub y: u8,
    // Farb-IDs (MapColor packed: Grundfarbe * 4 + Helligkeit), zeilenweise.
    pub colors: Vec<u8>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct MapItemData {
    pub id: i32,
    pub scale: u8,
    pub locked: bool,
    pub decorations: Option<Vec<MapDecoration>>,
    pub patch: Option<MapPatch>,
}

pub fn read_map_item_data(payload: &[u8]) -> Result<MapItemData, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let id = r.read_varint()?;
    let scale = r.read_u8()?;
    let locked = r.read_bool()?;
    let decorations = if r.read_bool()? {
        let n = r.read_varint()?;
        if !(0..=4096).contains(&n) {
            return Err(ReadError);
        }
        let mut liste = Vec::with_capacity(n as usize);
        for _ in 0..n {
            let kind = r.read_varint()?;
            let x = r.read_i8()?;
            let y = r.read_i8()?;
            let rot = r.read_u8()? & 15;
            let name = if r.read_bool()? { Some(read_text_spans(&mut r)?) } else { None };
            liste.push(MapDecoration { kind, x, y, rot, name });
        }
        Some(liste)
    } else {
        None
    };
    // MapPatch: Breite 0 heisst "kein Rechteck".
    let width = r.read_u8()?;
    let patch = if width > 0 {
        let height = r.read_u8()?;
        let x = r.read_u8()?;
        let y = r.read_u8()?;
        let len = r.read_varint()?;
        if len < 0 || len as usize != width as usize * height as usize {
            return Err(ReadError);
        }
        let colors = r.read_bytes(len as usize)?.to_vec();
        Some(MapPatch { width, height, x, y, colors })
    } else {
        None
    };
    Ok(MapItemData { id, scale, locked, decorations, patch })
}

pub fn read_merchant_offers(payload: &[u8]) -> Result<MerchantOffers, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let sync_id = r.read_varint()?;
    let n = r.read_varint()?;
    if n < 0 {
        return Err(ReadError);
    }
    let mut offers = Vec::with_capacity(n.min(64) as usize);
    for _ in 0..n {
        offers.push(read_trade_offer(&mut r).map_err(|_| ReadError)?);
    }
    let level = r.read_varint()?;
    let xp = r.read_varint()?;
    let show_progress = r.read_bool()?;
    let can_restock = r.read_bool()?;
    Ok(MerchantOffers { sync_id, offers, level, xp, show_progress, can_restock })
}

// Handel auswaehlen (Index in der Angebotsliste); der Server legt dann die passenden Items
// aus dem Inventar in die Bezahlslots.
#[must_use] 
pub fn select_trade_payload(index: i32) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_SELECT_TRADE).write_varint(index);
    w.into_bytes()
}

#[must_use] 
pub fn container_button_click_payload(sync_id: i32, button: i32) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_CONTAINER_BUTTON_CLICK).write_varint(sync_id).write_varint(button);
    w.into_bytes()
}

// ServerboundEditBookPacket: das Buch mit Feder in Inventarplatz `slot` (Hotbar 0..8, Zweithand
// 40) bekommt diese Seiten (hoechstens 100 zu je 1024 Zeichen); mit Titel (hoechstens 32) wird
// es signiert.
pub const SB_PLAY_EDIT_BOOK: i32 = 0x17;

#[must_use] 
pub fn edit_book_payload(slot: i32, pages: &[String], title: Option<&str>) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_EDIT_BOOK).write_varint(slot).write_varint(pages.len() as i32);
    for seite in pages {
        w.write_string(seite);
    }
    w.write_bool(title.is_some());
    if let Some(t) = title {
        w.write_string(t);
    }
    w.into_bytes()
}

#[must_use] 
pub fn rename_item_payload(name: &str) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_RENAME_ITEM).write_string(name);
    w.into_bytes()
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct ContainerContent {
    pub sync_id: i32,
    // "Revision" (state id): muss bei jedem Klick zurueckgeschickt werden, sonst schickt der
    // Server den ganzen Container erneut.
    pub revision: i32,
    pub slots: Vec<Option<ItemStack>>,
    pub cursor: Option<ItemStack>,
    // Erster Slot, an dem das Lesen abgebrochen ist (unbekannter Component-Typ) -- die App
    // loggt das einmal, damit fehlende Components auffallen statt still zu verschwinden.
    pub truncated_at: Option<(usize, SlotError)>,
    // Beim Abbruch: wo genau (s. item::last_slot_failure) und die Rohbytes ab dem
    // unlesbaren Slot -- zum Nachsehen, welches Format dort wirklich steht.
    pub truncated_detail: Option<(String, Vec<u8>)>,
}

pub fn read_container_content(payload: &[u8]) -> Result<ContainerContent, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let sync_id = r.read_varint()?;
    let revision = r.read_varint()?;
    let count = r.read_varint()?;
    if count < 0 {
        return Err(ReadError);
    }
    let mut slots = Vec::with_capacity(count.min(256) as usize);
    let mut truncated_at = None;
    let mut truncated_detail = None;
    for index in 0..count as usize {
        let ab_hier = r.remaining().to_vec();
        match read_optional_slot(&mut r) {
            Ok(stack) => slots.push(stack),
            Err(err) => {
                let wo = crate::item::last_slot_failure().unwrap_or_else(|| "ohne Component-Angabe".to_string());
                truncated_detail = Some((wo, ab_hier));
                truncated_at = Some((index, err));
                break;
            }
        }
    }
    // Der Cursor steht hinter der Liste -- nach einem Abbruch ist die Leseposition dahin,
    // dann bleibt er leer und wird ohnehin gleich per Set Cursor Item nachgereicht.
    let cursor = if truncated_at.is_none() { read_optional_slot(&mut r).unwrap_or(None) } else { None };
    Ok(ContainerContent { sync_id, revision, slots, cursor, truncated_at, truncated_detail })
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct ContainerSlotUpdate {
    pub sync_id: i32,
    pub revision: i32,
    pub slot: i16,
    pub stack: Option<ItemStack>,
}

pub fn read_container_set_slot(payload: &[u8]) -> Result<ContainerSlotUpdate, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let sync_id = r.read_varint()?;
    let revision = r.read_varint()?;
    let slot = r.read_i16()?;
    let stack = read_optional_slot(&mut r).map_err(|_| ReadError)?;
    Ok(ContainerSlotUpdate { sync_id, revision, slot, stack })
}

pub fn read_cursor_item(payload: &[u8]) -> Result<Option<ItemStack>, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    read_optional_slot(&mut r).map_err(|_| ReadError)
}

// (Slot im Spielerinventar, Stack)
pub fn read_player_inventory_slot(payload: &[u8]) -> Result<(i32, Option<ItemStack>), ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let slot = r.read_varint()?;
    let stack = read_optional_slot(&mut r).map_err(|_| ReadError)?;
    Ok((slot, stack))
}

pub fn read_set_carried_item(payload: &[u8]) -> Result<i32, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    r.read_varint()
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct OpenScreen {
    pub sync_id: i32,
    // ScreenHandlerType-Registry-ID -- bestimmt Groesse und Aufbau des Containers.
    pub screen_type: i32,
    pub title: String,
}

pub fn read_open_screen(payload: &[u8]) -> Result<OpenScreen, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let sync_id = r.read_varint()?;
    let screen_type = r.read_varint()?;
    // Mit Paragraphen-Codes, damit die Farben des Titels ueberleben (s. legacy_to_spans).
    let title = read_text_component_legacy(&mut r)?;
    Ok(OpenScreen { sync_id, screen_type, title })
}

pub fn read_container_close(payload: &[u8]) -> Result<i32, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    r.read_varint()
}

// ServerboundSelectBundleItemPacket: im Buendel in Slot `slot` (Menue-Slot) ist Item `index`
// gewaehlt, -1 = keins.
pub const SB_PLAY_SELECT_BUNDLE_ITEM: i32 = 0x02;

#[must_use] 
pub fn select_bundle_item_payload(slot: i32, index: i32) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_SELECT_BUNDLE_ITEM).write_varint(slot).write_varint(index);
    w.into_bytes()
}

// Aktiven Hotbar-Slot melden (0..8). Vanilla schreibt hier einen Short, keinen VarInt.
#[must_use] 
pub fn set_carried_item_payload(slot: u16) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_SET_CARRIED_ITEM).write_u16(slot);
    w.into_bytes()
}

#[must_use] 
pub fn close_container_payload(sync_id: i32) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_CONTAINER_CLOSE).write_varint(sync_id);
    w.into_bytes()
}

// Klick-Modus (SlotActionType).
pub const CLICK_MODE_PICKUP: i32 = 0;
pub const CLICK_MODE_QUICK_MOVE: i32 = 1;
pub const CLICK_MODE_SWAP: i32 = 2;
pub const CLICK_MODE_CLONE: i32 = 3;
pub const CLICK_MODE_THROW: i32 = 4;
pub const CLICK_MODE_QUICK_CRAFT: i32 = 5;
pub const CLICK_MODE_PICKUP_ALL: i32 = 6;

// ClickSlotC2SPacket. Seit 1.21.5 beschreibt der Client zu jedem Klick, WIE das Ergebnis
// seiner Meinung nach aussieht: fuer jeden Slot, den er selbst veraendert glaubt, und fuer
// den Cursor je ein "ItemStackHash". Der Server fuehrt den Klick ohnehin aus; die Hashes
// gehen nur in seine Abgleichspruefung (TrackedSlot::isInSync). Stimmen sie, schweigt er --
// stimmen sie nicht, schickt er den betroffenen Slot korrigierend nach.
//
// ItemStackHash (leer):     bool false
// ItemStackHash (gefuellt): bool true, varint item_id, varint count,
//                           varint n_added + je (varint type_id, i32 hash),
//                           varint n_removed + je varint type_id
//
// Den Prüfwert eines GESETZTEN Components koennen wir nicht bilden (er laeuft ueber
// Vanillas HashOps auf dem Component-INHALT, den item.rs bewusst nicht aufhebt) -- solche
// Stapel sind nicht exakt hashbar (s. ItemStack::hashable).
//
// Trotzdem wird auch fuer sie der GEFUELLTE Hash geschrieben, nur eben mit leerer
// Component-Liste. Das ist eine bewusst falsche Angabe: der Server bildet den Hash MIT
// seinen Components, findet eine Abweichung und schickt den Slot korrigierend nach -- genau
// das, was wir wollen, wenn wir nichts vorhersagen koennen.
//
// ECHTER BUG gewesen (nach dem Ablegen blieb der Mauszeiger noch das Item, etwa bei
// gefaerbter Lederruestung): frueher fiel dieser Fall auf den LEEREN Hash zurueck
// -- und "leer" heisst nicht "weiss ich nicht", sondern "der Cursor ist danach leer". Legt
// man ein solches Item ab, ist der Server-Cursor danach tatsaechlich leer, die Falschaussage
// stimmte also zufaellig, die Korrektur blieb aus, und das Item klebte lokal am Zeiger fest.
// "Leer" darf deshalb nur noch heissen, dass wirklich nichts da ist.
pub(crate) fn write_item_stack_hash(w: &mut PacketWriter, stack: Option<&ItemStack>) {
    match stack {
        Some(stack) => {
            w.write_bool(true).write_varint(stack.item_id as i32).write_varint(stack.count);
            // Gesetzte Components: bei hashbaren Stapeln gibt es tatsaechlich keine, bei den
            // uebrigen ist die leere Liste die absichtliche Abweichung von oben.
            w.write_varint(0);
            w.write_varint(stack.removed_components.len() as i32);
            for type_id in &stack.removed_components {
                w.write_varint(*type_id);
            }
        }
        None => {
            w.write_bool(false);
        }
    }
}

// `modified` sind die Slots, die der Client nach dem Klick veraendert erwartet (Slotnummer
// + vorhergesagter Inhalt), `cursor` der erwartete Stapel an der Maus. Wer nichts
// vorhersagen kann/will, uebergibt eine leere Liste und None -- dann schickt der Server
// nach jedem Klick den neuen Zustand, was ebenfalls korrekt ist, nur gespraechiger.
#[must_use] 
pub fn click_container_payload(
    sync_id: i32,
    revision: i32,
    slot: i16,
    button: i8,
    mode: i32,
    modified: &[(i16, Option<ItemStack>)],
    cursor: Option<&ItemStack>,
) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_CONTAINER_CLICK)
        .write_varint(sync_id)
        .write_varint(revision)
        .write_u16(slot as u16)
        .write_u8(button as u8)
        .write_varint(mode)
        .write_varint(modified.len() as i32);
    for (slot, stack) in modified {
        w.write_u16(*slot as u16);
        write_item_stack_hash(&mut w, stack.as_ref());
    }
    write_item_stack_hash(&mut w, cursor);
    w.into_bytes()
}
