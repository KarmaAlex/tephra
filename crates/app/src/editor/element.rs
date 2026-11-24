//! The editor's element: renders the projected blocks, collects their leaf geometry
//! (gpui-kit editor-host API) and paints the caret on top.

use std::collections::HashMap;
use std::ops::Range;

use gpui_kit::base::text::editor_host::{collect_leaf_geometry, Caret};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;

use super::Editor;

pub struct EditorElement {
    editor: Entity<Editor>,
    child: Option<AnyElement>,
    decorations: HashMap<u64, Vec<(Range<usize>, Hsla)>>,
    /// Wavy underlines: (leaf key, leaf range, colour).
    squiggles: Vec<(u64, Range<usize>, Hsla)>,
    scroll: ScrollHandle,
}

impl EditorElement {
    pub fn new(editor: Entity<Editor>, child: AnyElement, decorations: HashMap<u64, Vec<(Range<usize>, Hsla)>>, squiggles: Vec<(u64, Range<usize>, Hsla)>, scroll: ScrollHandle) -> Self {
        Self { editor, child: Some(child), decorations, squiggles, scroll }
    }
}

/// A wavy line from `x0` to `x1` with its top at `y`.
fn paint_squiggle(window: &mut Window, x0: Pixels, x1: Pixels, y: Pixels, color: Hsla) {
    let (step, amp) = (px(2.), px(1.5));
    let mut b = PathBuilder::stroke(px(1.));
    b.move_to(point(x0, y + amp));
    let mut x = x0;
    let mut up = true;
    while x < x1 {
        x = (x + step).min(x1);
        b.line_to(point(x, if up { y } else { y + amp }));
        up = !up;
    }
    if let Ok(p) = b.build() {
        window.paint_path(p, color);
    }
}

impl IntoElement for EditorElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for EditorElement {
    type RequestLayoutState = AnyElement;
    type PrepaintState = Option<Bounds<Pixels>>;

    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, window: &mut Window, cx: &mut App) -> (LayoutId, AnyElement) {
        let mut child = self.child.take().expect("child");
        (child.request_layout(window, cx), child)
    }

    fn prepaint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, _: Bounds<Pixels>, child: &mut AnyElement, window: &mut Window, cx: &mut App) -> Option<Bounds<Pixels>> {
        let prepaint_start = std::time::Instant::now();
        let decorations = std::mem::take(&mut self.decorations);
        let (_, mut geometry) = collect_leaf_geometry(decorations, || child.prepaint(window, cx));
        // nested Markdown renders (plugin blocks) report their read-only leaves too: keep only
        // the editor's own, so hit-testing and caret placement never land on nested text
        geometry.retain(|k, _| *k < super::projection::NESTED_KEY_BASE);
        let scroll = self.scroll.clone();
        self.editor.update(cx, |e, _| {
            e.perf.prepaint_start = Some(prepaint_start);
            e.geometry = geometry;
            let caret = e.caret_bounds(e.doc.selection().head);
            if std::mem::take(&mut e.autoscroll) {
                if let Some(c) = caret {
                    let view = scroll.bounds();
                    let margin = c.size.height * 2.;
                    let mut off = scroll.offset();
                    if c.top() - margin < view.top() {
                        off.y += view.top() - (c.top() - margin);
                    } else if c.bottom() + margin > view.bottom() {
                        off.y -= (c.bottom() + margin) - view.bottom();
                    }
                    off.y = off.y.min(px(0.)).max(-scroll.max_offset().y);
                    if off != scroll.offset() {
                        scroll.set_offset(off);
                        // a refresh requested mid-draw is dropped; ask for the next frame instead
                        window.request_animation_frame();
                    }
                }
            }
            caret
        })
    }

    fn paint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, bounds: Bounds<Pixels>, child: &mut AnyElement, caret: &mut Option<Bounds<Pixels>>, window: &mut Window, cx: &mut App) {
        let paint_start = std::time::Instant::now();
        child.paint(window, cx);
        {
            let e = self.editor.read(cx);
            let lines: Vec<(Pixels, Pixels, Pixels, Hsla)> = self
                .squiggles
                .iter()
                .filter_map(|(k, r, c)| {
                    let g = e.geometry.get(k)?;
                    let a = g.position_for_caret(Caret::new(r.start));
                    let b = g.position_for_caret(Caret::new(r.end));
                    // a word does not wrap; skip anything split across lines
                    ((a.y - b.y).abs() < px(1.) && b.x > a.x).then(|| (a.x, b.x, a.y + g.caret_height() - px(2.), *c))
                })
                .collect();
            for (x0, x1, y, c) in lines {
                paint_squiggle(window, x0, x1, y, c);
            }
        }
        let (focus, caret_color) = {
            let e = self.editor.read(cx);
            (e.focus.clone(), cx.theme().caret)
        };
        let reading = self.editor.read(cx).mode == super::ViewMode::Reading;
        if focus.is_focused(window) && !reading {
            if let Some(c) = *caret {
                window.paint_quad(fill(c, caret_color));
            }
        }
        window.handle_input(&focus, ElementInputHandler::new(bounds, self.editor.clone()), cx);
        self.editor.update(cx, |e, _| {
            if let Some(t) = e.perf.pending_edit.take() {
                e.perf.keystroke_ms.push(t.elapsed().as_secs_f64() * 1000.0);
                if let Some(r) = e.perf.render_start {
                    e.perf.frame_work_ms.push(r.elapsed().as_secs_f64() * 1000.0);
                    let ms = |a: std::time::Instant, b: std::time::Instant| b.saturating_duration_since(a).as_secs_f64() * 1000.0;
                    if let (Some(re), Some(pp)) = (e.perf.render_end, e.perf.prepaint_start) {
                        let now = std::time::Instant::now();
                        e.perf.phases.push([ms(r, re), ms(re, pp), ms(pp, paint_start), ms(paint_start, now)]);
                    }
                }
            }
        });
    }
}
