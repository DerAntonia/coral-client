// Halten und Ansehen in der Ego-Ansicht (Sichtraum, -Z nach vorn, rechte Hand; links gespiegelt).

// Beim Essen und Trinken sitzt die Faust so weit weiter rechts (nach aussen) am Item, voll vor
// dem Mund (s. app tuning_share).
pub const EAT_FIST_RIGHT: f32 = 0.15;

// Beim Hochwerfen der Werkzeuge (item_inspect TOOL_TOSS) steht die Hand so weit weiter rechts
// und oben (x, y).
pub const TOOL_TOSS_HAND: [f32; 2] = [0.06, 0.06];

// Modified View Model, Bogen spannen: der Bogen bleibt in seiner Halte-Lage und wandert so weit
// (x zur Bildmitte, y hoch, z) vor die Augen, in so vielen Ticks ab Spannbeginn.
pub const BOW_DRAW_TO: [f32; 3] = [-0.5, 0.25, 0.0];
pub const BOW_DRAW_TICKS: f32 = 6.0;

// Das Nah-an-die-Kamera-Ansehen (item_inspect::Kind::CloseLook) bringt die Mitte des Items
// hierhin: waagerecht mittig, auf gut 70 % der Bildhoehe (Hand-Sichtfeld 70 Grad: y/-z =
// -0.39 * tan 35), weit genug weg, dass man daneben noch etwas sieht.
pub const CLOSE_LOOK_TARGET: [f32; 3] = [0.0, -0.23, -0.85];

// Und verkleinert das Item dabei auf diesen Anteil (um seine Mitte).
pub const CLOSE_LOOK_SCALE: f32 = 0.85;

// Dabei haelt die Faust das Item von unten: so weit unter seiner Mitte (in voller Groesse,
// schrumpft mit CLOSE_LOOK_SCALE).
pub const CLOSE_LOOK_FIST_BELOW: f32 = 0.17;
