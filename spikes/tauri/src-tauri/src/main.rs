// Perf spike: bare Tauri window + CM6 + KaTeX showing one note from the fixture.
use std::path::{Component, Path, PathBuf};
use std::time::Instant;

struct Cfg { vault: PathBuf, note: String, bench: bool, t0: Instant }

#[tauri::command]
fn config(cfg: tauri::State<Cfg>) -> serde_json::Value {
    serde_json::json!({ "vault": cfg.vault, "note": cfg.note, "bench": cfg.bench })
}

#[tauri::command]
fn load_note(cfg: tauri::State<Cfg>, rel: String) -> Result<String, String> {
    let p = Path::new(&rel);
    if p.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Err("invalid path".into());
    }
    std::fs::read_to_string(cfg.vault.join(p)).map_err(|e| e.to_string())
}

#[tauri::command]
fn ready(cfg: tauri::State<Cfg>) {
    println!("READY {:.1}", cfg.t0.elapsed().as_secs_f64() * 1000.0);
}

#[tauri::command]
fn report(app: tauri::AppHandle, json: String) {
    println!("REPORT {json}");
    app.exit(0);
}

fn main() {
    let t0 = Instant::now();
    let home = std::env::var("HOME").expect("HOME");
    let vault = std::env::var("VAULT").map(PathBuf::from).unwrap_or_else(|_| Path::new(&home).join("dev/vault-fixture"));
    let note = std::env::var("NOTE").unwrap_or_else(|_| "Altro/Thesis.md".into());
    let bench = std::env::var("BENCH").is_ok();
    tauri::Builder::default()
        .manage(Cfg { vault, note, bench, t0 })
        .invoke_handler(tauri::generate_handler![config, load_note, ready, report])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
