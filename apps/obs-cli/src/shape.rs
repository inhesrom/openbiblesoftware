//! `obs-cli shape`: shape lines of text with the bundled fonts and print glyphs, or write an
//! SVG drawn from the glyph outlines.

use std::process::ExitCode;

use obs_fonts::{
    FaceId, Script, ShapeStyle, lines_svg, script_of, shape_uncached, small_caps_source,
};

const USAGE: &str = "usage: obs-cli shape [--size PX] [--svg OUT.svg] [--smcp] <text>...
  Each text is one line. Hebrew is shaped right-to-left with Ezra SIL, everything else with
  Gentium Plus. --smcp lowercases ASCII and turns on small capitals (as for LORD).";

pub fn shape(args: &[String]) -> ExitCode {
    let mut size = 32.0f32;
    let mut svg: Option<String> = None;
    let mut smcp = false;
    let mut texts: Vec<&str> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--size" => match it.next().and_then(|v| v.parse().ok()) {
                Some(v) => size = v,
                None => {
                    eprintln!("obs-cli shape: --size needs a number\n{USAGE}");
                    return ExitCode::from(2);
                }
            },
            "--svg" => match it.next() {
                Some(v) => svg = Some(v.clone()),
                None => {
                    eprintln!("obs-cli shape: --svg needs a file\n{USAGE}");
                    return ExitCode::from(2);
                }
            },
            "--smcp" => smcp = true,
            t => texts.push(t),
        }
    }
    if texts.is_empty() {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }
    let mut lines = Vec::new();
    for text in texts {
        let rtl = script_of(text) == Script::Hebrew;
        let face = FaceId::for_script(script_of(text), false, false);
        let style = ShapeStyle::new(face, size, smcp, rtl);
        let source = if smcp {
            small_caps_source(text)
        } else {
            text.to_string()
        };
        let run = match shape_uncached(&source, style) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("obs-cli shape: {e}");
                return ExitCode::FAILURE;
            }
        };
        println!(
            "{text}\n  face {:?}, {} glyphs, {} missing, width {:.1}px, {}",
            face,
            run.glyphs.len(),
            run.missing_glyphs(),
            run.width,
            if rtl { "rtl" } else { "ltr" }
        );
        for g in &run.glyphs {
            let ch = source[g.cluster as usize..].chars().next().unwrap_or(' ');
            println!(
                "    glyph {:>5}  x {:>7.2}  y {:>6.2}  adv {:>6.2}  byte {:>3} {ch:?}",
                g.id, g.x, g.y, g.advance, g.cluster
            );
        }
        lines.push((run, style));
    }
    if let Some(path) = svg {
        let doc = match lines_svg(&lines) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("obs-cli shape: {e}");
                return ExitCode::FAILURE;
            }
        };
        if let Err(e) = std::fs::write(&path, doc) {
            eprintln!("obs-cli shape: {path}: {e}");
            return ExitCode::FAILURE;
        }
        println!("wrote {path}");
    }
    ExitCode::SUCCESS
}
