// Was gerade viel nachgestellt wird, in einer kleinen eigenen Crate: eine Zahl hier zu aendern
// baut nur diese Crate und die Stellen neu, die sie benutzen, nicht die ganze Welt. Ist etwas
// fertig eingestellt, darf es wieder dorthin, wo es hingehoert.
pub mod hold;
pub mod juggle;
