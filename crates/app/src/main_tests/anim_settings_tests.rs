use super::*;
use world::more_anims as anims;

#[test]
fn animationen_ueberstehen_speichern_und_laden() {
    let mut s = Settings::parse_onto(Settings::base(), "show_hud=true\n");
    assert_eq!(s.anim_flags, anims::DEFAULT);
    s.anim_flags &= !anims::PET;
    let gelesen = Settings::parse_onto(Settings::base(), &s.serialize());
    assert_eq!(gelesen.anim_flags, s.anim_flags);
}

// Die alte Fassung kannte zwoelf Animationen: deren Stand bleibt, die neuen bekommen ihre
// Vorgabe.
#[test]
fn alte_animations_zeile_wird_uebernommen() {
    let s = Settings::parse_onto(Settings::base(), "anim_flags=4095\n");
    assert_eq!(s.anim_flags & 0xFFF, 0xFFF);
    assert_eq!(s.anim_flags & !0xFFF, anims::DEFAULT & !0xFFF);
}
