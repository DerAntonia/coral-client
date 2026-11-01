use super::*;

#[test]
fn logos_passen_in_ihren_atlas() {
    assert!(SIDEBAR_LINKS.len() as u32 <= LOGO_GRID * LOGO_GRID);
    for (name, url, png) in SIDEBAR_LINKS {
        let bild = image::load_from_memory_with_format(png, image::ImageFormat::Png).expect(name);
        assert_eq!((bild.width(), bild.height()), (LOGO_PX, LOGO_PX), "{name}");
        assert!(url.starts_with("https://"), "{name}");
    }
}

#[test]
fn logos_liegen_ueber_setup() {
    let setup = App::sidebar_setup_rect(800.0);
    let letzte = App::sidebar_link_rect(SIDEBAR_LINKS.len() - 1, 800.0);
    assert!(letzte.y + letzte.h < setup.y - 9.0, "über der Linie vor Setup");
    assert!(letzte.x + letzte.w <= SIDEBAR_W - SIDEBAR_PAD, "in der Leiste");
}
