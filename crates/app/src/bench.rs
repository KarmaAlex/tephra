//! Typing benchmark (TEPHRA_BENCH=1): types into the open note through the normal
//! edit path, one character per frame, then prints `REPORT {json}` and quits.
//! The note is never saved during the benchmark.

use std::time::Duration;

use gpui_kit::*;
use tephra_core::markdown::find_math;
use tephra_core::{EditKind, Selection};

use crate::editor::Editor;

fn stats(v: &[f64]) -> String {
    if v.is_empty() {
        return "null".into();
    }
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p = |q: f64| s[((s.len() as f64 * q) as usize).min(s.len() - 1)];
    format!(
        "{{\"n\":{},\"p50\":{:.2},\"p95\":{:.2},\"max\":{:.2},\"mean\":{:.2}}}",
        s.len(),
        p(0.5),
        p(0.95),
        p(1.0),
        s.iter().sum::<f64>() / s.len() as f64
    )
}

fn rss_mib() -> f64 {
    std::fs::read_to_string("/proc/self/statm")
        .ok()
        .and_then(|s| s.split_whitespace().nth(1)?.parse::<f64>().ok())
        .map(|pages| pages * 4096.0 / 1048576.0)
        .unwrap_or(0.0)
}

async fn type_text(editor: &WeakEntity<Editor>, cx: &mut AsyncApp, text: &str) -> (Vec<f64>, Vec<f64>) {
    let mut latency = Vec::new();
    let mut work = Vec::new();
    for ch in text.chars() {
        let _ = editor.update(cx, |e, cx| {
            // like a real keystroke: plugin hooks (snippets) first, timed from the key press
            e.perf.pending_edit = Some(std::time::Instant::now());
            let s = ch.to_string();
            if !e.run_char_hooks(&s, cx) {
                let h = e.doc.selection().head;
                e.edit(h..h, &s, EditKind::Insert, cx);
            }
        });
        cx.background_executor().timer(Duration::from_millis(40)).await;
        let _ = editor.update(cx, |e, _| {
            latency.extend(e.perf.keystroke_ms.drain(..));
            work.extend(e.perf.frame_work_ms.drain(..));
        });
    }
    (latency, work)
}

pub fn start(editor: Entity<Editor>, _window: &mut Window, cx: &mut App) {
    let weak = editor.downgrade();
    cx.spawn(async move |cx| {
        cx.background_executor().timer(Duration::from_millis(1500)).await;
        // never let the benchmark touch the file: disable saving by pointing at nothing
        let _ = weak.update(cx, |e, _| e.disable_saving());
        // 1. plain text at the end of a line in the middle of the note
        let _ = weak.update(cx, |e, cx| {
            let mid = e.doc.line_end(e.doc.line_of(e.doc.len() / 2));
            e.set_selection(Selection::caret(mid), false, cx);
            e.perf.keystroke_ms.clear();
            e.perf.frame_work_ms.clear();
            e.perf.projection_ms.clear();
        });
        cx.background_executor().timer(Duration::from_millis(200)).await;
        let plain: String = " the quick brown fox jumps over the lazy dog".chars().cycle().take(100).collect();
        let (plain_lat, plain_work) = type_text(&weak, cx, &plain).await;
        // 2. inside a formula near the middle
        let _ = weak.update(cx, |e, cx| {
            let t = e.doc.text();
            let ms = find_math(&t);
            if let Some(m) = ms.get(ms.len() / 2) {
                let at = m.inner().end;
                e.set_selection(Selection::caret(at), false, cx);
            }
            e.perf.keystroke_ms.clear();
            e.perf.frame_work_ms.clear();
        });
        cx.background_executor().timer(Duration::from_millis(200)).await;
        let (math_lat, math_work) = type_text(&weak, cx, "+\\frac{a}{b}^{2}").await;
        // 3. search across the vault (< 100 ms budget, see AGENTS.md) and a full re-index (< 2 s)
        let (search_ms, index_ms) = weak
            .update(cx, |e, _| {
                let v = e.vault.borrow();
                let queries = ["graph", "tree", "a", "\"divide and conquer\"", "note -draft", "path:examples", "tag:#work", "proof OR lemma", "file:readme"];
                let mut ms = Vec::new();
                for _ in 0..5 {
                    for q in queries {
                        let t = std::time::Instant::now();
                        let n = tephra_core::search::parse_query(q).map(|q| v.index.search(&q).len()).unwrap_or(0);
                        ms.push(t.elapsed().as_secs_f64() * 1000.0);
                        std::hint::black_box(n);
                    }
                }
                let t = std::time::Instant::now();
                let idx = tephra_core::index::VaultIndex::build(&v.root);
                std::hint::black_box(idx.files().count());
                (stats(&ms), format!("{:.1}", t.elapsed().as_secs_f64() * 1000.0))
            })
            .unwrap_or_default();
        let proj = weak.update(cx, |e, _| stats(&e.perf.projection_ms)).unwrap_or_default();
        let phases = weak
            .update(cx, |e, _| {
                let col = |i: usize| e.perf.phases.iter().map(|p| p[i]).collect::<Vec<_>>();
                format!("{{\"render\":{},\"layout\":{},\"prepaint\":{},\"paint\":{}}}", stats(&col(0)), stats(&col(1)), stats(&col(2)), stats(&col(3)))
            })
            .unwrap_or_default();
        println!(
            "REPORT {{\"phases\":{},\"plain_latency\":{},\"plain_frame_work\":{},\"math_latency\":{},\"math_frame_work\":{},\"projection_ms\":{},\"search_ms\":{},\"index_ms\":{},\"rss_mib\":{:.1}}}",
            phases,
            stats(&plain_lat),
            stats(&plain_work),
            stats(&math_lat),
            stats(&math_work),
            proj,
            search_ms,
            index_ms,
            rss_mib()
        );
        let _ = cx.update(|cx| cx.quit());
    })
    .detach();
}
