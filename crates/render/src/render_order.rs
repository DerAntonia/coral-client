// Render-Order (Einstellungen > Render): welche Grafik IMMER ueber welcher liegt, auch wenn
// die andere in der Welt davor steht -- wie zwei Ebenen in einem Malprogramm.
//
// Umgesetzt ueber den Stencil des Welt-Durchgangs. Wer oben liegen kann (Spieler und die
// anderen PvP-Entities, Blockentities, Hitboxen, uebrige Entities), schreibt auf seinen Pixeln
// die Menge dessen, was er dort verdeckt (ein Bit je Art, s. cover). Was verdeckt werden kann
// (Partikel, Flammen, Wasser, uebriges Durchscheinendes, bei den Hitboxen auch die Spieler),
// laeuft DANACH und laesst Pixel aus, auf denen sein Bit steht. Ohne Bit entscheidet wie
// immer die Tiefe.

pub mod cover {
    pub const PARTICLES: u8 = 1;
    // Die Flammen an brennenden Entities.
    pub const FLAMES: u8 = 2;
    pub const WATER: u8 = 4;
    // Nur fuer die Hitboxen: ueber den Spielern (s. BlockRenderer::draw_after_terrain).
    pub const PLAYERS: u8 = 8;
    // Glas, Eis, Feuerbloecke -- alles Durchscheinende des Gelaendes ausser Wasser. Liegt immer
    // unter Entities und Hitboxen (wie bisher, als es schlicht vor ihnen gezeichnet wurde),
    // nicht aber unter Blockentities: die verdeckt Glas nach der Tiefe (Nutzerwunsch).
    pub const REST: u8 = 16;
    // Kein Verdecken, sondern ein Merker: hier steht eine Hitbox-Kante (fuers scharfe
    // Nachzeichnen nach der Bewegungsunschaerfe, s. render_hitboxes).
    pub const MARK: u8 = 32;
    // Was die Verdecker ueberschreiben: alles ausser dem Merker.
    pub const WRITTEN: u8 = PARTICLES | FLAMES | WATER | PLAYERS | REST;
}

// Klassen der Entity-Vertices und Skinning-Instanzen.
pub const CLASS_OTHER: u8 = 0;
pub const CLASS_PVP: u8 = 1;
pub const CLASS_BLOCK_ENTITY: u8 = 2;
// Flammen an Entities: verdecken nichts, werden verdeckt.
pub const CLASS_FLAME: u8 = 3;

// Je Verdecker die Bits aus `cover`, die er ueberdeckt. REST setzt of_class/of_hitbox dazu,
// ausser bei Blockentities.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RenderOrder {
    pub pvp: u8,
    pub block_entity: u8,
    pub hitbox: u8,
}

impl Default for RenderOrder {
    // Wie der Client vor dieser Einstellung zeichnete: alles ueber Wasser, sonst nach der Tiefe.
    fn default() -> Self {
        Self { pvp: cover::WATER, block_entity: cover::WATER, hitbox: cover::WATER }
    }
}

impl RenderOrder {
    // Der Stencil-Wert einer Entity-Klasse.
    #[must_use]
    pub const fn of_class(&self, class: u8) -> u8 {
        match class {
            CLASS_PVP => cover::REST | (self.pvp & (cover::PARTICLES | cover::FLAMES | cover::WATER)),
            // Blockentities ohne REST: Glas davor verdeckt sie nach der Tiefe.
            CLASS_BLOCK_ENTITY => self.block_entity & (cover::PARTICLES | cover::FLAMES | cover::WATER),
            // Uebrige Entities (Mobs ...): wie bisher ueber Wasser, sonst nach der Tiefe.
            _ => cover::REST | cover::WATER,
        }
    }

    #[must_use]
    pub const fn of_hitbox(&self) -> u8 {
        cover::REST | (self.hitbox & cover::WRITTEN)
    }

    #[must_use]
    pub const fn hitbox_over_players(&self) -> bool {
        self.hitbox & cover::PLAYERS != 0
    }

    // Reicht EIN Zug fuer das Durchscheinende des Gelaendes (sortiert, Maske WATER|REST)? Nur
    // wenn jeder Verdecker Wasser und Glas gleich behandelt: Spieler, Mobs und Hitboxen liegen
    // immer ueber Glas, also muessen sie ueber Wasser liegen; Blockentities nie ueber Glas,
    // also auch nicht ueber Wasser. Sonst Wasser und Rest getrennt (s. draw_after_terrain).
    #[must_use]
    pub const fn single_translucent_pass(&self) -> bool {
        self.pvp & self.hitbox & cover::WATER != 0 && self.block_entity & cover::WATER == 0
    }
}

const fn face(compare: wgpu::CompareFunction, pass_op: wgpu::StencilOperation) -> wgpu::StencilFaceState {
    wgpu::StencilFaceState { compare, fail_op: wgpu::StencilOperation::Keep, depth_fail_op: wgpu::StencilOperation::Keep, pass_op }
}

// Verdecker (Entities): schreibt seinen Wert (Referenz), laesst aber Pixel aus, auf denen eine
// Hitbox "ueber Spielern" steht -- die Referenz traegt das Bit PLAYERS nie.
pub(crate) const fn stencil_writer() -> wgpu::StencilState {
    let f = face(wgpu::CompareFunction::Equal, wgpu::StencilOperation::Replace);
    wgpu::StencilState { front: f, back: f, read_mask: cover::PLAYERS as u32, write_mask: cover::WRITTEN as u32 }
}

// Verdecktes: nur dort, wo `bits` nicht stehen (Referenz 0), schreibt nichts.
pub(crate) const fn stencil_under(bits: u8) -> wgpu::StencilState {
    let f = face(wgpu::CompareFunction::Equal, wgpu::StencilOperation::Keep);
    wgpu::StencilState { front: f, back: f, read_mask: bits as u32, write_mask: 0 }
}

// Hitbox-Kanten: schreiben immer, samt Merker.
pub(crate) const fn stencil_hitbox() -> wgpu::StencilState {
    let f = face(wgpu::CompareFunction::Always, wgpu::StencilOperation::Replace);
    wgpu::StencilState { front: f, back: f, read_mask: 0xFF, write_mask: 0xFF }
}

// Nur auf dem Merker (Referenz MARK).
pub(crate) const fn stencil_marked() -> wgpu::StencilState {
    let f = face(wgpu::CompareFunction::Equal, wgpu::StencilOperation::Keep);
    wgpu::StencilState { front: f, back: f, read_mask: cover::MARK as u32, write_mask: 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vorgabe_wie_bisher() {
        let o = RenderOrder::default();
        // Blockentities ueber Wasser, aber nicht ueber Glas: zwei Zuege.
        assert!(!o.single_translucent_pass());
        assert!(!o.hitbox_over_players());
        assert_eq!(o.of_class(CLASS_OTHER), cover::REST | cover::WATER);
        assert_eq!(o.of_class(CLASS_PVP), cover::REST | cover::WATER);
        assert_eq!(o.of_class(CLASS_BLOCK_ENTITY), cover::WATER);
    }

    #[test]
    fn spieler_bit_nie_im_wert_der_entities() {
        let o = RenderOrder { pvp: 0xFF, block_entity: 0xFF, hitbox: 0xFF };
        for k in [CLASS_OTHER, CLASS_PVP, CLASS_BLOCK_ENTITY] {
            assert_eq!(o.of_class(k) & (cover::PLAYERS | cover::MARK), 0);
        }
        assert_eq!(o.of_hitbox() & cover::MARK, 0);
        assert!(o.hitbox_over_players());
    }

    #[test]
    fn ohne_wasser_getrennt() {
        let o = RenderOrder { pvp: 0, ..RenderOrder::default() };
        assert!(!o.single_translucent_pass());
        let o = RenderOrder { block_entity: 0, ..RenderOrder::default() };
        assert!(o.single_translucent_pass());
    }
}
