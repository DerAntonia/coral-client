// Ein Resourcepack-Zip in einen Ordner entpacken -- fuer den Skin-Ordner, in den man ein
// heruntergeladenes Pack einfach hineinlegt (s. app::skin_zips).
//
// Gelesen wird mit demselben nachsichtigen Leser wie fuer Server-Packs (s. javazip): Packs
// aus dem Netz sind oft absichtlich verbogen, und was Minecraft laden kann, soll sich hier
// auch entpacken lassen. Daraus folgen zwei Sonderfaelle:
//
//   - Ein Name mit Schraegstrich am Ende ist nur dann ein Ordner, wenn er leer ist; mit
//     Inhalt ist es eine getarnte Datei ("pack.mcmeta/") und wird ohne Strich geschrieben.
//   - Ordner-Eintraege selbst braucht es nicht, die Ordner entstehen mit ihren Dateien.
//
// Viele Packs stecken in einem zusaetzlichen Ordner ("Mein Pack/assets/...", so wie man einen
// Ordner zippt). Dann wird dieser Ordner zur Wurzel, damit das Ergebnis direkt als Pack
// erkannt wird (s. pack_root).
//
// Geschrieben wird zuerst in einen versteckten Nachbarordner, der erst am Ende umbenannt
// wird: die Skin-Liste sieht nie ein halb entpacktes Pack.

use std::path::{Path, PathBuf};

use crate::javazip::JavaZip;

// Entpackt `zip` als neuen Ordner unter `parent` und gibt dessen Pfad zurueck. Der Name ist
// der des Zips ohne Endung; ist er vergeben, kommt " (2)", " (3)" ... dahinter.
pub fn extract_resource_pack(zip: &Path, parent: &Path) -> Result<PathBuf, String> {
    let archiv = JavaZip::open(zip).map_err(|e| format!("nicht lesbar: {e}"))?;
    let wurzel = pack_root(archiv.names()).ok_or_else(|| "kein Resourcepack (kein assets/-Ordner)".to_string())?;
    let stamm = zip.file_stem().and_then(|s| s.to_str()).filter(|s| !s.is_empty()).unwrap_or("Pack");
    let ziel = free_name(parent, stamm);
    let tmp = parent.join(format!(".{stamm}.entpacken"));
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).map_err(|e| format!("alter Zwischenordner {}: {e}", tmp.display()))?;
    }
    let ergebnis = write_entries(&archiv, &wurzel, &tmp).and_then(|()| std::fs::rename(&tmp, &ziel).map_err(|e| format!("umbenennen: {e}")));
    if let Err(e) = ergebnis {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(e);
    }
    Ok(ziel)
}

fn write_entries(archiv: &JavaZip, wurzel: &str, tmp: &Path) -> Result<(), String> {
    std::fs::create_dir_all(tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
    for name in archiv.names() {
        let Some(rest) = name.strip_prefix(wurzel) else { continue };
        let Some(teile) = safe_parts(rest.trim_end_matches('/')) else { continue };
        let Some(daten) = archiv.read(name) else { continue };
        if name.ends_with('/') && daten.is_empty() {
            continue;
        }
        let pfad = teile.iter().fold(tmp.to_path_buf(), |p, t| p.join(t));
        if let Some(eltern) = pfad.parent() {
            std::fs::create_dir_all(eltern).map_err(|e| format!("{}: {e}", eltern.display()))?;
        }
        // Ein Name, der schon als Ordner existiert (Tarn-Eintrag neben echtem Ordner), wird
        // uebergangen statt das ganze Pack abzubrechen.
        if pfad.is_dir() {
            continue;
        }
        std::fs::write(&pfad, daten).map_err(|e| format!("{}: {e}", pfad.display()))?;
    }
    Ok(())
}

// Die Pfadteile eines Eintrags, oder None, wenn er aus dem Zielordner herausfuehren
// koennte (".." , absolute Pfade, Laufwerksbuchstaben) -- solche Eintraege kann Minecraft
// ohnehin nicht als Ressource ansprechen.
fn safe_parts(rest: &str) -> Option<Vec<&str>> {
    let teile: Vec<&str> = rest.split('/').filter(|t| !t.is_empty()).collect();
    let ok = !teile.is_empty() && teile.iter().all(|t| *t != "." && *t != ".." && !t.contains('\\') && !t.contains(':'));
    ok.then_some(teile)
}

// Das Praefix, unter dem das Pack liegt: "" bei assets/ auf oberster Ebene, sonst der
// kuerzeste Ordner, der assets/ enthaelt.
fn pack_root(names: &[String]) -> Option<String> {
    names
        .iter()
        .filter_map(|n| {
            let i = if n.starts_with("assets/") { Some(0) } else { n.find("/assets/").map(|i| i + 1) }?;
            Some(n[..i].to_string())
        })
        .min_by_key(|p| (p.matches('/').count(), p.len()))
}

fn free_name(parent: &Path, stamm: &str) -> PathBuf {
    let erster = parent.join(stamm);
    if !erster.exists() {
        return erster;
    }
    (2..).map(|n| parent.join(format!("{stamm} ({n})"))).find(|p| !p.exists()).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn zip_bauen(pfad: &Path, eintraege: &[(&str, &[u8])]) {
        let mut z = zip::ZipWriter::new(std::fs::File::create(pfad).unwrap());
        for (name, daten) in eintraege {
            z.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(daten).unwrap();
        }
        z.finish().unwrap();
    }

    fn arbeitsordner(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("coral-client-pack-unzip-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn wurzel_oben_oder_im_einzigen_ordner() {
        let n = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(pack_root(&n(&["pack.mcmeta", "assets/minecraft/x.png"])).as_deref(), Some(""));
        assert_eq!(pack_root(&n(&["Mein Pack/pack.mcmeta", "Mein Pack/assets/minecraft/x.png"])).as_deref(), Some("Mein Pack/"));
        assert_eq!(pack_root(&n(&["a/b/assets/x", "a/assets/y"])).as_deref(), Some("a/"));
        assert_eq!(pack_root(&n(&["readme.txt", "bild.png"])), None);
    }

    #[test]
    fn entpackt_ohne_huelle_und_ohne_ausbruch() {
        let d = arbeitsordner("huelle");
        let zip = d.join("Schoenes Pack.zip");
        zip_bauen(
            &zip,
            &[
                ("Huelle/pack.mcmeta", b"{}"),
                ("Huelle/assets/minecraft/textures/a.png", b"png"),
                ("Huelle/../boese.txt", b"x"),
                ("anderes/datei.txt", b"y"),
            ],
        );
        let ziel = extract_resource_pack(&zip, &d).unwrap();
        assert_eq!(ziel, d.join("Schoenes Pack"));
        assert_eq!(std::fs::read(ziel.join("pack.mcmeta")).unwrap(), b"{}");
        assert_eq!(std::fs::read(ziel.join("assets/minecraft/textures/a.png")).unwrap(), b"png");
        assert!(!d.join("boese.txt").exists());
        assert!(!ziel.join("anderes").exists());
        assert!(!d.join(".Schoenes Pack.entpacken").exists());
        // Zweites Mal: neuer Name statt Ueberschreiben.
        assert_eq!(extract_resource_pack(&zip, &d).unwrap(), d.join("Schoenes Pack (2)"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn getarnte_datei_mit_schraegstrich() {
        let d = arbeitsordner("tarn");
        let zip = d.join("p.zip");
        zip_bauen(&zip, &[("pack.mcmeta/", b"{\"pack\":{}}"), ("assets/minecraft/a.json", b"{}")]);
        let ziel = extract_resource_pack(&zip, &d).unwrap();
        assert_eq!(std::fs::read(ziel.join("pack.mcmeta")).unwrap(), b"{\"pack\":{}}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn kein_pack_bleibt_unangetastet() {
        let d = arbeitsordner("keinpack");
        let zip = d.join("fotos.zip");
        zip_bauen(&zip, &[("bild.png", b"x")]);
        assert!(extract_resource_pack(&zip, &d).is_err());
        assert!(!d.join("fotos").exists());
        let _ = std::fs::remove_dir_all(&d);
    }
}
