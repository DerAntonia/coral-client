// Render-Order (Einstellungen > Render, Nutzer 2026-11-12): eine Tabelle, welche Grafik IMMER
// ueber welcher liegt -- wie zwei Ebenen in einem Malprogramm, auch wenn die untere in der
// Welt davor steht. Zeile = was oben liegt, Spalte = was darunter liegt. Aus heisst: wie
// gewohnt nach der Entfernung. Gezeichnet wird das ueber den Stencil, s. render::render_order.
//
// "Spieler" sind alle PvP-Arten des Hitbox-Moduls ausser den Mobs (s.
// hitbox_kinds::HitboxKind::is_pvp). "Feuer" sind die Flammen an brennenden Entities;
// Feuerbloecke liegen wie Glas ohnehin immer unter Entities und Hitboxen.

use render::render_order::{cover, RenderOrder};

pub(crate) const ROWS: usize = 3;
pub(crate) const COLS: usize = 4;
pub(crate) const COL_NAMES: [&str; COLS] = ["Partikel", "Feuer", "Wasser", "Spieler"];
const COL_BITS: [u8; COLS] = [cover::PARTICLES, cover::FLAMES, cover::WATER, cover::PLAYERS];
const ROW_HITBOX: usize = 2;
const COL_PLAYERS: usize = 3;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct RenderOrderTable(pub(crate) [[bool; COLS]; ROWS]);

impl Default for RenderOrderTable {
    // Wie der Client vor dieser Tabelle zeichnete: alles ueber dem Wasser, sonst nach der
    // Entfernung.
    fn default() -> Self {
        Self([[false, false, true, false]; ROWS])
    }
}

impl RenderOrderTable {
    // Gibt es die Zelle? "Ueber Spielern" koennen nur die Hitboxen liegen.
    pub(crate) const fn has_cell(row: usize, col: usize) -> bool {
        row < ROWS && (col < COL_PLAYERS || (col == COL_PLAYERS && row == ROW_HITBOX))
    }

    pub(crate) const fn get(&self, row: usize, col: usize) -> bool {
        Self::has_cell(row, col) && self.0[row][col]
    }

    pub(crate) const fn toggle(&mut self, row: usize, col: usize) {
        if Self::has_cell(row, col) {
            self.0[row][col] = !self.0[row][col];
        }
    }

    pub(crate) fn to_render(self) -> RenderOrder {
        let bits = |row: usize| (0..COLS).filter(|&c| self.get(row, c)).fold(0, |m, c| m | COL_BITS[c]);
        RenderOrder { pvp: bits(0), block_entity: bits(1), hitbox: bits(ROW_HITBOX) }
    }

    // Einstellungsdatei: je Zeile die Zellen als 0/1, Zeilen mit Komma getrennt.
    pub(crate) fn encode(&self) -> String {
        self.0.iter().map(|z| z.iter().map(|&an| if an { '1' } else { '0' }).collect::<String>()).collect::<Vec<_>>().join(",")
    }

    // Was fehlt oder unlesbar ist, behaelt den Wert aus `base`.
    pub(crate) fn decode(text: &str, base: Self) -> Self {
        let mut out = base;
        for (row, zeile) in text.split(',').take(ROWS).enumerate() {
            for (col, z) in zeile.trim().chars().take(COLS).enumerate() {
                if Self::has_cell(row, col) && matches!(z, '0' | '1') {
                    out.0[row][col] = z == '1';
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vorgabe_ist_das_bisherige_bild() {
        assert_eq!(RenderOrderTable::default().to_render(), RenderOrder::default());
    }

    #[test]
    fn speichern_und_laden() {
        let mut t = RenderOrderTable::default();
        t.toggle(0, 0);
        t.toggle(2, 3);
        t.toggle(1, 2);
        assert_eq!(t.encode(), "1010,0000,0011");
        assert_eq!(RenderOrderTable::decode(&t.encode(), RenderOrderTable::default()), t);
        // Kaputtes behaelt die Vorgabe, die fehlende Zelle bleibt aus.
        assert_eq!(RenderOrderTable::decode("x", RenderOrderTable::default()), RenderOrderTable::default());
        assert!(!RenderOrderTable::decode("1111,1111,1111", RenderOrderTable::default()).get(0, 3));
    }

    #[test]
    fn zellen_setzen_die_bits_ihrer_zeile() {
        let mut t = RenderOrderTable([[false; COLS]; ROWS]);
        t.toggle(0, 0);
        t.toggle(1, 1);
        t.toggle(2, 3);
        // Spieler ueber Spielern gibt es nicht.
        t.toggle(0, 3);
        let r = t.to_render();
        assert_eq!((r.pvp, r.block_entity, r.hitbox), (cover::PARTICLES, cover::FLAMES, cover::PLAYERS));
    }
}
