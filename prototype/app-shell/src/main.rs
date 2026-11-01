//! PROTOTYPE — throwaway app shell for issue #13 ("What should the main window feel like?").
//! Not production code: no tests beyond smoke checks, no persistence, minimal error handling.
//! Run: `./run.sh` (prepares data on first run) or `cargo run --release`.

#![allow(deprecated, dead_code)]

mod app;
mod data;
mod doc;
mod present;
mod refs;
mod text;

fn main() -> eframe::Result {
    if std::env::args().any(|a| a == "--dump") {
        dump();
        return Ok(());
    }
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1560.0, 960.0])
            .with_title("Open Bible — app shell PROTOTYPE (#13)"),
        ..Default::default()
    };
    eframe::run_native("obs-app-shell-prototype", opts, Box::new(|cc| Ok(Box::new(app::App::new(cc)))))
}

/// Headless check: lay out + raster a few blocks to PNG (target/dump-*.png).
fn dump() {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let lib = data::Library::load(&base.join("data"));
    let fonts = text::Fonts::load(&base.join("assets/fonts"));
    let s = present::TextSettings::default();
    for (name, res) in [("kjv", &lib.kjv), ("tahot", &lib.tahot), ("jfb", &lib.jfb), ("mhcc", &lib.mhcc)] {
        let mut hs: Vec<(f32, usize)> = (0..res.units.len())
            .map(|ui| {
                let mut objs = vec![];
                let para = present::unit_para(res, ui, &s, &mut objs);
                (text::layout(&fonts, &para, 450.0).height, ui)
            })
            .collect();
        hs.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
        let total: f32 = hs.iter().map(|h| h.0).sum();
        println!("{name}: {} blocks, total {:.0}px at 450pt wide, tallest {:?}", hs.len(), total, &hs[..3]);
    }
    for (name, res, units) in [("kjv", &lib.kjv, 0..4), ("tahot", &lib.tahot, 0..3), ("jfb", &lib.jfb, 0..4)] {
        for ui in units {
            let mut objs = vec![];
            let para = present::unit_para(res, ui, &s, &mut objs);
            let t = std::time::Instant::now();
            let laid = text::layout(&fonts, &para, 620.0);
            let lt = t.elapsed();
            let t = std::time::Instant::now();
            let (px, [w, h]) = text::raster(&fonts, &laid, 2.0);
            let rt = t.elapsed();
            // composite on white
            let mut rgb = Vec::with_capacity(w * h * 3);
            for c in px.chunks_exact(4) {
                let a = c[3] as u32;
                for k in 0..3 {
                    rgb.push((c[k] as u32 + (255 - a)) as u8);
                }
            }
            let path = base.join(format!("target/dump-{name}-{ui}.ppm"));
            let mut f = format!("P6 {w} {h} 255\n").into_bytes();
            f.extend(rgb);
            std::fs::write(&path, f).unwrap();
            println!("{name} unit {ui}: {} lines, {:.0}x{:.0}, layout {:?}, raster {:?}", laid.lines.len(), laid.width, laid.height, lt, rt);
        }
    }
}
