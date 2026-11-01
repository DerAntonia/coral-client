use super::*;

#[test]
fn kugelbild_nach_wert() {
    assert_eq!(experience_orb_icon(1), 0);
    assert_eq!(experience_orb_icon(3), 1);
    assert_eq!(experience_orb_icon(16), 2);
    assert_eq!(experience_orb_icon(17), 3);
    assert_eq!(experience_orb_icon(2477), 10);
    assert_eq!(experience_orb_icon(30000), 10);
}
