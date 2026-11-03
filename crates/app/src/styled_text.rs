// Formatierter Text: Abschnitte setzen, messen, umbrechen und kuerzen.

use crate::*;

// Kapitaelchen-s ueber Abschnittsgrenzen hinweg (s. font_atlas::small_caps_s): bei Farbverlaeufen
// ist jeder Buchstabe ein eigener Abschnitt, das Wort sieht man erst in der ganzen Zeile.
pub(crate) fn small_caps_spans<'a>(gpu: &GpuState, spans: &'a [protocol::nbt::TextSpan]) -> std::borrow::Cow<'a, [protocol::nbt::TextSpan]> {
    if !gpu.layout_atlas().has_small_capital_s() {
        return std::borrow::Cow::Borrowed(spans);
    }
    let voll: String = spans.iter().map(|s| s.text.as_str()).collect();
    let std::borrow::Cow::Owned(neu) = font_atlas::small_caps_s(&voll) else { return std::borrow::Cow::Borrowed(spans) };
    let mut zeichen = neu.chars();
    let out = spans
        .iter()
        .map(|s| protocol::nbt::TextSpan { text: zeichen.by_ref().take(s.text.chars().count()).collect(), ..s.clone() })
        .collect();
    std::borrow::Cow::Owned(out)
}

// Eine Zeile aus Farbabschnitten zeichnen, ausgerichtet wie EIN Text (s.
// GpuState::char_positions). Zerlegt wird zusaetzlich an jeder Grenze zwischen Ziffer und
// Nicht-Ziffer: so beginnt jede Zahl an ihrem reservierten Platz, und alles dahinter steht
// fest. Die Ziffern-Stuecke sind reine Zahlen und werden in shape_text nicht noch einmal
// verschoben (s. stable_numbers::pattern).
pub(crate) fn push_stable_spans(
    gpu: &GpuState,
    spans: &[protocol::nbt::TextSpan],
    x: f32,
    y: f32,
    default: [f32; 4],
    scale: f32,
    texts: &mut Vec<UiText>,
    rects: &mut Vec<UiRect>,
) {
    let spans = &*small_caps_spans(gpu, spans);
    let voll: String = spans.iter().map(|s| s.text.as_str()).collect();
    let (pos, gesamt) = gpu.char_positions(&voll, scale);
    // Fette Zeichen haben einen Font-Pixel mehr Vorschub (s. push_bold_marker): wie viele
    // davon vor Zeichen i stehen.
    let mut fette_davor = vec![0usize];
    for span in spans {
        let fett = span.style & protocol::nbt::STYLE_BOLD != 0;
        for _ in span.text.chars() {
            fette_davor.push(fette_davor.last().unwrap() + usize::from(fett));
        }
    }
    let bei = |i: usize| pos.get(i).copied().unwrap_or(gesamt) + fette_davor.get(i).or(fette_davor.last()).copied().unwrap_or(0) as f32 * scale;
    let mut i = 0;
    for span in spans {
        let farbe = span.color.map_or(default, rgb_to_rgba);
        let text = styled_text(span);
        let erstes = i;
        let mut stueck = String::new();
        let mut anfang = i;
        let mut ziffer: Option<bool> = None;
        let mut stuecke = Vec::new();
        for ch in text.chars() {
            let d = ch.is_ascii_digit();
            if ziffer.is_some_and(|z| z != d) {
                stuecke.push((std::mem::take(&mut stueck), x + bei(anfang)));
                anfang = i;
            }
            ziffer = Some(d);
            stueck.push(ch);
            i += 1;
        }
        if !stueck.is_empty() {
            stuecke.push((stueck, x + bei(anfang)));
        }
        for (t, sx) in stuecke {
            push_styled_piece(span.style, span.shadow, t, sx, y, farbe, scale, texts);
        }
        span_lines(gpu, span.style, x + bei(erstes), x + bei(i), y, farbe, scale, &mut |r| rects.push(UiRect { rect: r, color: farbe, ..UiRect::FLAT }));
    }
}

// Ein Stueck Text, fett ueber push_bold_marker.
// `shadow`: die Schattenfarbe des Servers (s. push_span_shadow), fuer beide Durchgaenge.
pub(crate) fn push_styled_piece(style: u8, shadow: Option<u32>, text: String, x: f32, y: f32, color: [f32; 4], scale: f32, texts: &mut Vec<UiText>) {
    if style & protocol::nbt::STYLE_BOLD != 0 {
        push_bold_marker(texts);
    }
    push_span_shadow(texts, shadow);
    texts.push(UiText { text, x, y, color, scale });
}

// Der Text eines Abschnitts, wie er gezeichnet wird: verschleiert (obfuscated) wechselt jedes
// Zeichen ausser Leerzeichen staendig gegen ein zufaelliges (Vanilla: eines gleicher Breite;
// hier eines aus derselben Gruppe, damit die Zeile nicht springt).
pub(crate) fn styled_text(span: &protocol::nbt::TextSpan) -> std::borrow::Cow<'_, str> {
    if span.style & protocol::nbt::STYLE_OBFUSCATED == 0 {
        return std::borrow::Cow::Borrowed(&span.text);
    }
    const GRUPPEN: [&str; 3] = ["0123456789", "abcdeghjmnopqrsuvwxyz", "ABCDEFGHJKLMNOPQRSTUVWXYZ"];
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() / 50) as u64;
    std::borrow::Cow::Owned(
        span.text
            .chars()
            .enumerate()
            .map(|(i, c)| {
                let Some(g) = GRUPPEN.iter().find(|g| g.contains(c)) else { return c };
                let h = (t ^ (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)).wrapping_mul(0x2545_F491_4F6C_DD1D) >> 33;
                g.as_bytes()[(h % g.len() as u64) as usize] as char
            })
            .collect(),
    )
}

// Unter- und Durchstreichen eines Abschnitts von x0 bis x1 (TextRenderer.Drawer: Rechteck vom
// Pixel VOR dem ersten Zeichen bis zum Ende des Vorschubs, Leerzeichen eingeschlossen --
// genau damit bauen Server ihre Trennlinien aus "&m" und Leerzeichen). Durchgestrichen auf
// halber Zeilenhoehe, unterstrichen in der untersten Zeile der Zelle, je einen Font-Pixel dick.
pub(crate) fn span_lines(gpu: &GpuState, style: u8, x0: f32, x1: f32, y: f32, _color: [f32; 4], scale: f32, out: &mut dyn FnMut(Rect)) {
    if style & (protocol::nbt::STYLE_STRIKETHROUGH | protocol::nbt::STYLE_UNDERLINED) == 0 || x1 <= x0 {
        return;
    }
    let px = scale.max(1.0);
    let h = gpu.text_height(scale);
    let (links, breite) = ((x0 - px).round(), (x1 - x0 + px).round());
    if style & protocol::nbt::STYLE_STRIKETHROUGH != 0 {
        out(Rect { x: links, y: (y + h * 0.5 - px * 0.5).round(), w: breite, h: px.round() });
    }
    if style & protocol::nbt::STYLE_UNDERLINED != 0 {
        out(Rect { x: links, y: (y + h - px).round(), w: breite, h: px.round() });
    }
}

// Eine Zeile aus Abschnitten nebeneinander, mit Fettdruck und Linien. Breite wie
// span_row_width. Gibt das Ende der Zeile zurueck.
pub(crate) fn push_styled_row(
    gpu: &GpuState,
    spans: &[protocol::nbt::TextSpan],
    mut x: f32,
    y: f32,
    default: [f32; 4],
    scale: f32,
    texts: &mut Vec<UiText>,
    line: &mut dyn FnMut(Rect, [f32; 4]),
) -> f32 {
    for span in small_caps_spans(gpu, spans).iter() {
        let farbe = span.color.map_or(default, rgb_to_rgba);
        let w = span_width(gpu, span, scale);
        span_lines(gpu, span.style, x, x + w, y, farbe, scale, &mut |r| line(r, farbe));
        push_styled_piece(span.style, span.shadow, styled_text(span).into_owned(), x, y, farbe, scale, texts);
        x += w;
    }
    x
}

// Vorschub eines Abschnitts: fett je Zeichen einen Font-Pixel mehr (Glyph.getAdvance(bold)).
pub(crate) fn span_width(gpu: &GpuState, span: &protocol::nbt::TextSpan, scale: f32) -> f32 {
    let w = gpu.measure_text(&span.text, scale);
    if span.style & protocol::nbt::STYLE_BOLD != 0 {
        w + span.text.chars().count() as f32 * scale
    } else {
        w
    }
}

pub(crate) fn span_row_width(gpu: &GpuState, spans: &[protocol::nbt::TextSpan], scale: f32) -> f32 {
    small_caps_spans(gpu, spans).iter().map(|s| span_width(gpu, s, scale)).sum()
}

// Breite einer Zeile, wie push_stable_spans sie setzt: als EIN Text gemessen (reservierter
// Platz wechselnder Zahlen zaehlt mit), plus der Fett-Vorschub.
pub(crate) fn stable_row_width(gpu: &GpuState, spans: &[protocol::nbt::TextSpan], scale: f32) -> f32 {
    let spans = &*small_caps_spans(gpu, spans);
    let voll: String = spans.iter().map(|s| s.text.as_str()).collect();
    let fette: usize = spans.iter().filter(|s| s.style & protocol::nbt::STYLE_BOLD != 0).map(|s| s.text.chars().count()).sum();
    gpu.measure_text(&voll, scale) + fette as f32 * scale
}

// Umbrochen wird der UEBERSETZTE Text (s. i18n) -- die einzelnen Zeilen kennt die Tabelle nicht.
pub(crate) fn wrap_text(gpu: &GpuState, text: &str, scale: f32, max_width: f32) -> Vec<String> {
    wrap_measured(|t| gpu.measure_raw(t, scale), &i18n::tr(text), max_width)
}

// Dieselbe Umbruchregel, aber mit einer MESSFUNKTION statt der GPU -- so laesst sie sich
// pruefen, ohne ein Fenster und einen Font-Atlas aufzubauen (dieselbe Trennung wie bei
// chat_input::wrap_input_lines).
pub(crate) fn wrap_measured(measure: impl Fn(&str) -> f32, text: &str, max_width: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        // Ein Wort, das ALLEIN schon zu breit ist, kann keine Wortgrenze retten -- es wird
        // hart zerlegt. Ohne das laeuft genau das aus dem Bild, was man am dringendsten
        // lesen will: JSON-Kick-Gruende, Dateipfade, lange Adressen (etwa eine
        // Kick-Meldung, die rechts aus dem Fenster lief).
        if measure(word) > max_width {
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
            }
            lines.extend(hard_break(&measure, word, max_width));
            // Der letzte Rest darf weiterwachsen, damit das naechste Wort noch anhaengt.
            current = lines.pop().unwrap_or_default();
            continue;
        }
        let candidate = if current.is_empty() { word.to_string() } else { format!("{current} {word}") };
        if measure(&candidate) > max_width && !current.is_empty() {
            lines.push(current);
            current = word.to_string();
        } else {
            current = candidate;
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

// Ein einzelnes, zu breites Wort zeichenweise auf `max_width` zerlegen. Zeichenweise und
// nicht byteweise: ein abgeschnittenes UTF-8-Zeichen waere kein Text mehr.
//
// Gibt IMMER mindestens eine Zeile zurueck -- auch wenn nicht einmal ein einzelnes Zeichen
// passt (winziges Panel, riesige Schrift). Sonst verschwaende der Text stillschweigend.
pub(crate) fn hard_break(measure: impl Fn(&str) -> f32, word: &str, max_width: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for ch in word.chars() {
        let candidate = format!("{current}{ch}");
        if !current.is_empty() && measure(&candidate) > max_width {
            lines.push(std::mem::take(&mut current));
            current.push(ch);
        } else {
            current = candidate;
        }
    }
    if !current.is_empty() || lines.is_empty() {
        lines.push(current);
    }
    lines
}

// Zeigt den laengsten Suffix eines (oft sehr langen) Tokens, der noch in `max_width` Pixel
// passt -- so bleibt beim Tippen/Einfuegen das Ende samt Caret sichtbar, statt aus dem Feld
// zu laufen.
pub(crate) fn token_display(gpu: &GpuState, token: &str, max_width: f32) -> String {
    if gpu.measure_text(token, UI_SCALE_SMALL) <= max_width {
        return token.to_string();
    }
    let chars: Vec<char> = token.chars().collect();
    for start in 0..chars.len() {
        let suffix: String = chars[start..].iter().collect();
        if gpu.measure_text(&suffix, UI_SCALE_SMALL) <= max_width {
            return suffix;
        }
    }
    String::new()
}

// Zu langen Text auf `max_width` kuerzen und mit "..." markieren -- anders als
// token_display (das VORNE abschneidet, weil beim Token das Ende interessiert) bleibt hier
// der ANFANG stehen: bei einem Servernamen/einer Adresse ist das der aussagekraeftige Teil.
// Wie ellipsize, fuer eine Zeile aus farbigen Abschnitten: was nicht passt, endet mit "..."
// in der Farbe des Abschnitts, in dem abgeschnitten wurde.
// MOTD-Zeilen ohne den Leerraum, mit dem Server sie in Vanillas Liste zentrieren: gleich viele
// fuehrende Leerzeichen in allen Zeilen (die Zeilen bleiben zueinander ausgerichtet) und die am
// Ende. Sonst zaehlt der Rand mit, und eine eingerueckte MOTD passt nicht mehr gross in die Karte.
pub(crate) fn trim_motd_padding(zeilen: &mut [Vec<protocol::nbt::TextSpan>]) {
    let fuehrend = |z: &[protocol::nbt::TextSpan]| z.iter().flat_map(|s| s.text.chars()).take_while(|&c| c == ' ').count();
    let gemeinsam = zeilen.iter().filter(|z| z.iter().any(|s| !s.text.trim().is_empty())).map(|z| fuehrend(z)).min().unwrap_or(0);
    for z in zeilen.iter_mut() {
        let mut weg = gemeinsam;
        for span in z.iter_mut() {
            let n = span.text.chars().take(weg).take_while(|&c| c == ' ').count();
            span.text.drain(..n);
            weg -= n;
            if weg == 0 {
                break;
            }
        }
        while let Some(letzter) = z.last_mut() {
            let ende = letzter.text.trim_end_matches(' ').len();
            letzter.text.truncate(ende);
            if !letzter.text.is_empty() {
                break;
            }
            z.pop();
        }
        z.retain(|s| !s.text.is_empty());
    }
}

pub(crate) fn ellipsize_spans(gpu: &GpuState, spans: &[protocol::nbt::TextSpan], scale: f32, max_width: f32) -> Vec<protocol::nbt::TextSpan> {
    if span_row_width(gpu, spans, scale) <= max_width {
        return spans.to_vec();
    }
    let mut out: Vec<protocol::nbt::TextSpan> = Vec::new();
    let mut x = 0.0;
    for span in spans {
        let w = span_width(gpu, span, scale);
        let punkte = protocol::nbt::TextSpan { text: "...".to_string(), ..span.clone() };
        if x + w + span_width(gpu, &punkte, scale) <= max_width {
            out.push(span.clone());
            x += w;
            continue;
        }
        let chars: Vec<char> = span.text.chars().collect();
        for ende in (0..=chars.len()).rev() {
            let teil = protocol::nbt::TextSpan { text: chars[..ende].iter().collect::<String>() + "...", ..span.clone() };
            if x + span_width(gpu, &teil, scale) <= max_width || ende == 0 {
                out.push(teil);
                break;
            }
        }
        break;
    }
    out
}

pub(crate) fn ellipsize(gpu: &GpuState, text: &str, scale: f32, max_width: f32) -> String {
    if gpu.measure_text(text, scale) <= max_width {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    for end in (0..chars.len()).rev() {
        let candidate: String = chars[..end].iter().collect::<String>() + "...";
        if gpu.measure_text(&candidate, scale) <= max_width {
            return candidate;
        }
    }
    String::new()
}

// HUD-Editor: "Fertig"-Button unten links. Das Raster (zum Ausrichten) und das
// verschiebbare Element zeichnet build_hud_editor direkt.
pub(crate) const HUD_EDITOR_GRID_PX: f32 = 40.0;
pub(crate) const fn hud_done_button() -> Rect {
    Rect { x: 40.0, y: 30.0, w: 160.0, h: 44.0 }
}

// HUD-Editor: alle Elemente stehen da und lassen sich direkt ziehen, sonst nichts. Ein
// Rechtsklick auf ein Element oeffnet seine Einstellungen als Fenster in der Mitte ueber dem
// abgedunkelten Editor (s. App::hud_selected); Klick daneben, Enter oder Escape schliesst es,
// Escape ohne Fenster verlaesst den Editor.

// Labels + Hover-Tooltips der Haupt-Einstellungen-Zeilen (index-gleich, s. build_settings_general_tab
// und hovered_tooltip). Die Werte selbst (FPS, Render-Ahead, ...) sind Laufzeitdaten und
// bleiben dort -- hier steht nur, was ohnehin schon feststeht.
pub(crate) const SETTINGS_LABELS: [&str; SETTINGS_ROW_COUNT] = [
    "FPS-Limit",
    "Render-Ahead",
    "Mesh-Budget",
    "Chunk-Update-Threads",
    "Höhlen-Culling",
    "Mipmapping",
    "Motion Blur",
    "Tearing-Test",
    "TearFree",
    "Scanout-Timing",
    "Riss-Timing",
    "Riss-Reserve",
    "CPU",
    "GPU",
    "RAM",
    "Swap",
    "Compositing",
];
pub(crate) const SETTINGS_TOOLTIPS: [&str; SETTINGS_ROW_COUNT] = [
    "Begrenzt die Bildrate über einen Timer. Bei selbsttaktenden Present-Modi (Fifo/FifoRelaxed) wirkungslos -- die Zeile blendet dann ab. Zahl anklicken für einen eigenen Wert; 0 = Monitor (Bildrate des Monitors), -1 = unbegrenzt.",
    "Wie viele Frames die CPU maximal vorproduzieren darf, bevor sie auf die GPU/Anzeige wartet. 0 ist Sodiums 'CPU Render-Ahead Limit: 0' nachgebaut -- eine echte Wartezeit auf das GPU-Ende des vorherigen Frames vor jedem neuen (kostet spürbar FPS/Glätte, minimiert aber die Latenz). 1-4 überlassen die Drosselung stattdessen der Swapchain -- höher kann Ruckler unter Last glätten, kostet aber zusätzliche Eingabelatenz.",
    "Chunk-Spalten, die pro Frame als Mesh-Job an den Worker-Pool geschickt werden. Meshen selbst kostet keine Frame-Zeit mehr -- das begrenzt nur, wie viele Jobs auf einmal losgeschickt werden. Zahl anklicken für einen eigenen Wert.",
    "Größe des dedizierten Worker-Pools fürs Chunk-Meshing (nicht das Netzwerk, das läuft getrennt). Mehr Threads bauen Chunks parallel schneller auf, konkurrieren dabei aber mit allem anderen auf der CPU (Physik, Netzwerk-Decoding, Spiel selbst).",
    "Zeichnet nur die 16er-Abschnitte der Welt, die von der Kamera aus durch Luft, Wasser oder Glas überhaupt zu sehen sein können -- Höhlen unter der Oberfläche, und von unten die Oberfläche, fallen weg. Spart GPU-Zeit, ohne dass sich etwas Sichtbares ändert. AUS zum Vergleichen.",
    "Verkleinerte Fassungen der Blocktexturen für entfernte Blöcke, wie in Vanilla. AUS nimmt immer die volle Auflösung: in der Ferne flimmert es dann stärker.",
    "Bewegungsunschärfe: verwischt die Welt entlang der Kamerabewegung, wie das Auge schnelle Bewegung sieht -- Hand und HUD bleiben scharf. 100 % = die Bewegung während eines Monitorbilds; läuft das Spiel schneller als der Monitor, gleicht die Stärke das aus. Ein Durchgang auf der Grafikkarte ohne gespeicherte alte Bilder, also keine zusätzliche Verzögerung; steht die Kamera still, fällt er ganz weg. Nur mit normaler Projektion.",
    "Farbige Balken, die über den Bildschirm wandern -- macht Tearing (gerissene Bildzeilen) sichtbar, um TearFree und die Riss-Steuerung zu testen. Läuft im Menü und im Spiel.",
    "Globale Anzeigeeinstellung des Grafiktreibers (xrandr) gegen Tearing bei fehlendem Hardware-Flip -- nur bedienbar, wenn der Treiber sie hat (amdgpu, radeon, neuere modesetting; nicht Nvidia).  Gilt nur, während Coral Client im Vordergrund läuft -- danach automatisch zurück auf den Wert von vor dem Start.",
    "Legt bei Immediate (mit FPS-Limit) jeden Bildwechsel knapp vor die Austastlücke des Monitors -- dort ist er unsichtbar, es gibt keinen Riss mehr im Bild. Misst dafür die VBlanks des Monitors (Linux, /dev/dri) und präsentiert jeden Frame auf die Mikrosekunde zu einem daraus berechneten Zeitpunkt; die Bildrate folgt dann exakt dem Monitor (bzw. jedem n-ten Bild bei niedrigerem FPS-Limit). Kostet etwas Latenz (der Frame wird mit Vorlauf fertig gebaut). Bei Fifo wirkungslos -- dort gibt es keinen Riss. Im Frame-Profiler (F6) sieht man, wo er landet.",
    "Verschiebt den Bildwechsel bei \"Scanout-Timing\": Minus = früher, der Riss wandert nach oben; Plus = später, er wandert nach unten. Ohne Hardware-Flip setzt der X-Server den Wechsel erst eine Weile nach dem Present um, dann sitzt der Riss bei 0 oben im Bild -- so weit ins Minus drehen, bis er unten auftaucht, und dann ein Stück zurück: dann liegt er am Anfang der Austastlücke, und ein verspäteter Frame rutscht nur tiefer in die Lücke.",
    "Wie viel Zeit ein Frame bei \"Scanout-Timing\" vor seinem Termin fertig sein soll. Genau diese Zeit kommt als Verzögerung dazu. Weniger = etwas direkter, aber ein Frame, der wegen eines Rucklers zu spät fertig wird, verpasst seinen Termin -- für diesen einen Frame ist der Riss dann kurz im Bild. Mehr = nie ein Riss, etwas mehr Verzögerung. Nach einem Ruckler erhöht sich der Vorlauf von selbst und geht danach langsam wieder zurück.",
    "Prozessor dieses Rechners und wie viele Threads er gleichzeitig ausführt. Reine Information.",
    "Reine Information (Treiber/Vendor), kein Bedienelement.",
    "Belegter und gesamter Arbeitsspeicher des ganzen Rechners (nicht nur des Clients). Belegt zählt ohne den Cache, den das System jederzeit freigeben kann.",
    "Auslagerung auf die Festplatte. Wird hier viel belegt, reicht der Arbeitsspeicher nicht und es kann ruckeln.",
    "Wie das Coral-Fenster gerade auf den Schirm kommt. \"Zusammengesetzt von\": ein Compositor setzt es zusammen und entscheidet, wann ein Bild erscheint -- dann wirken Scanout-Timing und Riss-Timing nicht (Cinnamon: Einstellungen, Allgemein, \"Komposit für Vollbildfenster deaktivieren\" an). Sonst kommt es ohne Compositor per Hardware-Flip oder als Kopie des X-Servers, und dahinter steht, wo der Riss der letzten Bilder lag (gemessen an den Meldungen des X-Servers). Bleibt er bei jeder Einstellung an derselben Stelle, auch wenn man Scanout-Timing und Riss-Timing verstellt, hält der X-Server die Bilder fest -- dann hilft ein Neustart (oder Ab- und wieder Anmelden).",
];
// Zeilen des Render-Tabs (Reihenfolge wie SETTINGS_LABELS), in Abschnitten: Bildrate,
// Chunks, Effekte, Tearing, System. Sichtweiten, Entity-Weite und Block-Rotation stehen im Video-Tab.
pub(crate) const FPS_LIMIT_ROW: usize = 0;
pub(crate) const FRAME_LATENCY_ROW: usize = 1;
pub(crate) const MESH_BUDGET_ROW: usize = 2;
pub(crate) const CHUNK_THREADS_ROW: usize = 3;
// Hoehlen-Culling an/aus (Settings::cave_culling), s. render::occlusion.
pub(crate) const CAVE_CULLING_ROW: usize = 4;
pub(crate) const MIPMAPPING_ROW: usize = 5;
// Abschnitt Effekte: Bewegungsunschaerfe (Settings::motion_blur_percent).
pub(crate) const MOTION_BLUR_ROW: usize = 6;
pub(crate) const MOTION_BLUR_MAX: u32 = 300;
pub(crate) const MOTION_BLUR_STEP: u32 = 10;
pub(crate) const TEARING_TEST_ROW: usize = 7;
pub(crate) const TEARFREE_ROW: usize = 8;
// Abschnitt System: reine Anzeige (s. system_info).
pub(crate) const CPU_ROW: usize = 12;
pub(crate) const GPU_ROW: usize = 13;
pub(crate) const RAM_ROW: usize = 14;
pub(crate) const SWAP_ROW: usize = 15;
pub(crate) const COMPOSITING_ROW: usize = 16;
pub(crate) const BLOCK_ROTATION_TOOLTIP: &str = "Vanilla dreht/spiegelt die Textur mancher Vollwürfel (Erde, Sand, Stein, Grasblock, ...) pseudozufällig je nach Weltposition, damit sich das Muster nicht sichtbar wiederholt. AUS zeigt stattdessen für jeden Block immer dieselbe Ausrichtung. Änderung baut alle geladenen Chunks neu.";
pub(crate) const ENTITY_DISTANCE_TOOLTIP: &str = "Bis zu welcher Entfernung Entities gezeichnet werden (Spieler, Mobs, Items, Displays, Namensschilder, Hitboxen), in Blöcken. Weiter entfernte bleiben geladen und anvisierbar, sie werden nur nicht gezeichnet. Zahl anklicken für einen eigenen Wert, -1 = unbegrenzt.";
pub(crate) const BLOCK_ENTITY_DISTANCE_TOOLTIP: &str = "Bis zu welcher Entfernung Block-Entities gezeichnet werden (Truhen, Betten, Shulkerkisten, Köpfe, Statuen, Schilder), in Blöcken. Außerhalb der Render-Distance nie. Zahl anklicken für einen eigenen Wert, -1 = unbegrenzt.";
// Renderpfad: vorerst aus den Einstellungen genommen (der Shader-Pfad ruht), Zeile samt Knopf
// zu den Shadereinstellungen stehen auskommentiert in buttons().
#[allow(dead_code)]
pub(crate) const RENDER_PATH_TOOLTIP: &str = "PvP: der schnelle Forward-Renderer, gebaut für niedrige Frametime und Latenz. Shader: der Renderer mit Effekten für Survival (Licht, Schatten, Nebel, ...) -- noch im Aufbau, bis dahin zeichnet er wie PvP. Auf PvP-Servern PvP wählen.";
// Shader-Pfad: so viele Sections schreibt das Voxel-Abbild hoechstens je Frame (je 8 KB). Ein
// Sectionwechsel der Kamera bringt bis zu 256 neue, davon meist die Haelfte Luft.
pub(crate) const VOXEL_SECTIONS_PER_FRAME: usize = 96;

// Shadereinstellungen: Lichtstreuung in Prozent (s. Settings::shader_scattering_percent).
pub(crate) const SCATTERING_MAX: u32 = 400;
pub(crate) const SCATTERING_STEP: u32 = 10;
// Schattenweichheit (Sonnendurchmesser) in Zehntelgrad.
pub(crate) const SUN_SIZE_MAX_TENTHS: u32 = 80;
pub(crate) const SUN_SIZE_STEP_TENTHS: u32 = 5;
// Lichtstaerken je Lichtquelle (Settings::light_factors) in Prozent. Ueber Stufe 15 hinaus
// wird eine Quelle zum Fernlicht (s. world::light::emission_level): Glowstone auf 1000 %
// leuchtet 150 Bloecke weit. Bis 200 % in kleinen Schritten, darueber in grossen.
pub(crate) const LIGHT_FACTOR_MAX: u32 = 1000;

pub(crate) const fn light_factor_step(pct: u32, up: bool) -> u32 {
    let gross = if up { pct >= 200 } else { pct > 200 };
    if gross { 50 } else { 10 }
}
