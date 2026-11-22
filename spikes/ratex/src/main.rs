//! RaTeX (native Rust KaTeX port) against the vault's math corpus.
//! usage: spike-ratex <formulas.jsonl> [<png-out-dir> N]
//! formulas.jsonl lines: {"display":bool,"tex":string} (generated from the fixture; not committed)
use ratex_layout::{layout, layout_options::LayoutOptions, to_display::to_display_list};
use ratex_types::MathStyle;
use std::time::Instant;

fn render(tex: &str, display: bool) -> Result<ratex_types::DisplayList, String> {
    let nodes = ratex_parser::parse(tex).map_err(|e| format!("{e}"))?;
    let opts = LayoutOptions { style: if display { MathStyle::Display } else { MathStyle::Text }, ..Default::default() };
    let lb = layout(&nodes, &opts);
    Ok(to_display_list(&lb))
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let raw = std::fs::read_to_string(&args[1]).expect("read formulas");
    let items: Vec<(bool, String)> = raw.lines().filter(|l| !l.trim().is_empty()).map(|l| {
        let v: serde_json::Value = serde_json::from_str(l).expect("json");
        (v["display"].as_bool().unwrap_or(false), v["tex"].as_str().unwrap_or("").to_string())
    }).collect();
    std::panic::set_hook(Box::new(|_| {}));
    let (mut errs, mut panics) = (0usize, 0usize);
    let mut cats: std::collections::BTreeMap<String, usize> = Default::default();
    let mut fails: Vec<(usize, String)> = Vec::new();
    let mut times = Vec::with_capacity(items.len());
    let t0 = Instant::now();
    for (i, (display, tex)) in items.iter().enumerate() {
        let t = Instant::now();
        match std::panic::catch_unwind(|| render(tex, *display)) {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => { errs += 1; let c: String = e.chars().take(70).collect(); *cats.entry(c).or_default() += 1; fails.push((i, e)); }
            Err(_) => { panics += 1; *cats.entry("PANIC".into()).or_default() += 1; fails.push((i, "panic".into())); }
        }
        times.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    let total = t0.elapsed().as_secs_f64() * 1000.0;
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p = |q: f64| times[((times.len() as f64 * q) as usize).min(times.len() - 1)];
    println!("formulas={} errors={} panics={} total_ms={:.0} p50_ms={:.3} p95_ms={:.3} max_ms={:.2}", items.len(), errs, panics, total, p(0.5), p(0.95), p(1.0));
    for (c, n) in &cats { println!("  {n:5}  {c}"); }
    if let Ok(path) = std::env::var("FAIL_OUT") {
        let s: String = fails.iter().map(|(i, e)| serde_json::json!({"i": i, "error": e, "tex": items[*i].1}).to_string() + "\n").collect();
        std::fs::write(path, s).expect("write fails");
    }
    if args.len() >= 4 {
        let dir = &args[2];
        let n: usize = args[3].parse().unwrap_or(5);
        std::fs::create_dir_all(dir).expect("mkdir");
        let step = (items.len() / n).max(1);
        for (k, (display, tex)) in items.iter().step_by(step).take(n).enumerate() {
            if let Ok(dl) = render(tex, *display) {
                let opts = ratex_render::RenderOptions { font_size: 28.0, device_pixel_ratio: 1.0, ..Default::default() };
                if let Ok(png) = ratex_render::render_to_png(&dl, &opts) { std::fs::write(format!("{dir}/f{k}.png"), png).ok(); }
            }
        }
    }
}
