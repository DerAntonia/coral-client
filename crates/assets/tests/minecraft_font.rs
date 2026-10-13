// Prueft den Font-Loader gegen die echte lokale Installation. Braucht ein installiertes
// 1.21.11, daher #[ignore]: cargo test -p assets -- --ignored --nocapture

const VERSION: &str = "1.21.11";

#[test]
#[ignore]
fn laedt_ascii_font_mit_plausiblen_metriken() {
    let mut installation = assets::MinecraftInstallation::locate(VERSION, None).expect("Installation nicht gefunden");
    let font = installation.load_default_font().expect("Font nicht ladbar");
    let page = font.base_page();

    assert_eq!(page.texture.width, 128, "ascii.png sollte 128 breit sein");
    assert_eq!(page.cell_size(), (8, 8), "16x16-Raster auf 128x128 ergibt 8x8-Zellen");
    assert_eq!(page.ascent, 7, "Vanilla-ascent fuer ascii.png");
    assert_eq!(font.space_advance, 4.0, "Leerzeichen kommt aus dem space-Provider");

    // Zeile 4 des Rasters ist '@ABCDEFGHIJKLMNO'.
    assert_eq!(page.chars[4][1], 'A', "Rasterzuordnung stimmt nicht");

    // Der eigentliche Punkt der gemessenen Breiten: proportionale Schrift. Genau das
    // hat der frueher handgebaute Font falsch gemacht ("i" so breit wie "W").
    let (i_col, i_row) = find(page, 'i');
    let (w_col, w_row) = find(page, 'W');
    let width_i = page.glyph_width(i_col, i_row);
    let width_w = page.glyph_width(w_col, w_row);
    println!("Breite 'i' = {width_i}, 'W' = {width_w}");
    assert!(width_i < width_w, "'i' muss schmaler sein als 'W' (war {width_i} vs {width_w})");

    // Leerzeichen-Zelle ist leer -- deshalb braucht es den space-Provider ueberhaupt.
    let (sp_col, sp_row) = find(page, ' ');
    assert_eq!(page.glyph_width(sp_col, sp_row), 0, "Leerzeichen-Zelle sollte leer sein");
}

// Der Grund, warum ueberhaupt mehr als ascii.png geladen wird: deutscher Text. Fehlt eine
// der Seiten, fehlen die Zeichen ersatzlos -- im Spiel sieht man dann Luecken statt Umlauten.
#[test]
#[ignore]
fn deutsche_sonderzeichen_kommen_aus_ihren_seiten() {
    let mut installation = assets::MinecraftInstallation::locate(VERSION, None).expect("Installation nicht gefunden");
    let font = installation.load_default_font().expect("Font nicht ladbar");
    println!("{} Bitmap-Seiten geladen", font.pages.len());
    assert!(font.pages.len() >= 3, "Vanilla hat drei Bitmap-Seiten, geladen wurden {}", font.pages.len());

    for ch in ['ä', 'ö', 'ü', 'Ä', 'Ö', 'Ü', 'ß', 'é', 'à'] {
        let treffer = font
            .pages
            .iter()
            .enumerate()
            .find_map(|(i, p)| find_opt(p, ch).map(|(c, r)| (i, c, r, p.advance(c, r), p.ascent)));
        let (seite, col, row, advance, ascent) = treffer.unwrap_or_else(|| panic!("{ch:?} steht in keiner Seite"));
        println!("{ch:?}: Seite {seite}, Zelle {col}/{row}, Vorschub {advance}, ascent {ascent}");
        assert!(advance > 1.0, "{ch:?} hat eine leere Zelle (Vorschub {advance})");
    }
}

// Die Zellen der Seiten haben VERSCHIEDENE Groessen und Grundlinien -- genau deshalb muss
// jedes Glyph seine eigene Groesse und seinen eigenen Versatz tragen (s. app::FontAtlas).
#[test]
#[ignore]
fn die_seiten_haben_eigene_zellmasse() {
    let mut installation = assets::MinecraftInstallation::locate(VERSION, None).expect("Installation nicht gefunden");
    let font = installation.load_default_font().expect("Font nicht ladbar");
    for p in &font.pages {
        let (cw, ch) = p.cell_size();
        println!("Seite {}x{}: Zelle {cw}x{ch}, ascent {}, height {}, scale {}", p.texture.width, p.texture.height, p.ascent, p.height, p.scale());
        assert!((p.scale() - 1.0).abs() < 1e-6, "Vanilla-Seiten skalieren nicht (war {})", p.scale());
        assert!(p.ascent <= p.height, "ascent darf nicht ueber der Zielhoehe liegen");
    }
    // Die Umlaut-Seite ist hoeher als die ASCII-Seite -- sonst passten die Puenktchen nicht.
    let umlaut = font.pages.iter().find(|p| find_opt(p, 'ä').is_some()).expect("Seite mit ä");
    assert!(umlaut.ascent > font.base_page().ascent, "die Umlaut-Seite muss hoeher ansetzen");
}

fn find(page: &assets::FontPage, target: char) -> (u32, u32) {
    find_opt(page, target).unwrap_or_else(|| panic!("Zeichen {target:?} nicht im Raster"))
}

fn find_opt(page: &assets::FontPage, target: char) -> Option<(u32, u32)> {
    for (row, chars) in page.chars.iter().enumerate() {
        for (col, &ch) in chars.iter().enumerate() {
            if ch == target {
                return Some((col as u32, row as u32));
            }
        }
    }
    None
}

// Die Zauberschrift des Zaubertischs: 52 Buchstaben in ascii_sga.png, je mit Breite > 0.
#[test]
#[ignore]
fn zauberschrift_hat_alle_buchstaben() {
    let mut installation = assets::MinecraftInstallation::locate(VERSION, None).expect("Installation nicht gefunden");
    let alt = installation.load_alt_font_glyphs().expect("alt.json nicht ladbar");
    assert_eq!(alt.glyphs.len(), 52);
    assert_eq!(alt.sheet_size, [128, 128]);
    for c in ('a'..='z').chain('A'..='Z') {
        let g = alt.glyphs[&c];
        assert!(g.cell[2] > 0 && g.advance > 1.0, "{c}: {g:?}");
    }
    assert_eq!(alt.width("a b"), alt.glyphs[&'a'].advance + 4.0 + alt.glyphs[&'b'].advance);
}
