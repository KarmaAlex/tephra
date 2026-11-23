//! Graph view (Obsidian core plugin): the vault's notes as nodes, links as edges, laid out by a
//! force simulation (`sim`). The global graph opens in a tab; the local graph (the active
//! note's neighbourhood) is a right-sidebar view. Filters (tags, attachments, unresolved links,
//! orphans) and forces start from your `.obsidian/graph.json` (read, never written).
//!
//! Drag the background to pan, scroll to zoom, drag a node to move it, click a node to open
//! it (Ctrl+click: new tab), hover to highlight its links (Ctrl+hover previews the note).

pub mod sim;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;
use std::rc::Rc;
use std::time::Instant;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{h_flex, ActiveTheme as _, Selectable as _, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use obsidilight_core::index::{is_note, VaultIndex};
use obsidilight_core::links::link_path;
use serde_json::{Map, Value};

use super::api::*;
use super::api::Action;
use sim::{Forces, Sim};

pub const VIEW: &str = "graph:graph";

thread_local! {
    /// The last graph rendered (UI tests click its nodes through `graph-click`).
    pub static LAST: RefCell<Option<WeakEntity<GraphView>>> = const { RefCell::new(None) };
}
pub const LOCAL_VIEW: &str = "graph:local";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Note,
    Attachment,
    Unresolved,
    Tag,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GNode {
    /// Vault path, `#tag`, or the unresolved link text.
    pub id: String,
    pub label: String,
    pub kind: Kind,
    pub degree: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Filters {
    pub tags: bool,
    pub attachments: bool,
    pub unresolved: bool,
    pub orphans: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GraphData {
    pub nodes: Vec<GNode>,
    pub edges: Vec<(usize, usize)>,
}

fn label_of(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    if is_note(name) { name[..name.len() - 3].to_string() } else { name.to_string() }
}

/// The graph of the vault (`center` = None) or of a note's neighbourhood up to `depth` links.
pub fn build(index: &VaultIndex, f: &Filters, center: Option<&str>, depth: usize) -> GraphData {
    let mut ids: HashMap<String, usize> = HashMap::new();
    let mut nodes: Vec<GNode> = Vec::new();
    let mut add = |id: String, kind: Kind, nodes: &mut Vec<GNode>| -> usize {
        *ids.entry(id.clone()).or_insert_with(|| {
            let label = match kind {
                Kind::Note | Kind::Attachment => label_of(&id),
                _ => id.clone(),
            };
            nodes.push(GNode { id, label, kind, degree: 0 });
            nodes.len() - 1
        })
    };
    let mut files: Vec<&String> = index.files().collect();
    files.sort();
    for f2 in &files {
        if is_note(f2) {
            add((*f2).clone(), Kind::Note, &mut nodes);
        } else if f.attachments {
            add((*f2).clone(), Kind::Attachment, &mut nodes);
        }
    }
    let mut edges: HashSet<(usize, usize)> = HashSet::new();
    for note in files.iter().filter(|p| is_note(p)) {
        let Some(meta) = index.note(note) else { continue };
        let a = add((*note).clone(), Kind::Note, &mut nodes);
        for l in &meta.links {
            let b = match &l.resolved {
                Some(t) if is_note(t) => add(t.clone(), Kind::Note, &mut nodes),
                Some(t) if f.attachments => add(t.clone(), Kind::Attachment, &mut nodes),
                Some(_) => continue,
                None if f.unresolved => match link_path(&l.target).filter(|p| !p.is_empty()) {
                    Some(p) => add(p.to_string(), Kind::Unresolved, &mut nodes),
                    None => continue,
                },
                None => continue,
            };
            if a != b {
                edges.insert((a.min(b), a.max(b)));
            }
        }
        if f.tags {
            for t in &meta.tags {
                let tag = format!("#{}", t.trim_start_matches('#'));
                let b = add(tag, Kind::Tag, &mut nodes);
                edges.insert((a.min(b), a.max(b)));
            }
        }
    }
    let mut edges: Vec<(usize, usize)> = edges.into_iter().collect();
    edges.sort();
    for &(a, b) in &edges {
        nodes[a].degree += 1;
        nodes[b].degree += 1;
    }
    // which nodes to keep: a neighbourhood, or everything but (optionally) orphans
    let keep: Vec<bool> = match center.and_then(|c| nodes.iter().position(|n| n.id == c)) {
        Some(c) => {
            let mut adj: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
            for &(a, b) in &edges {
                adj[a].push(b);
                adj[b].push(a);
            }
            let mut dist = vec![usize::MAX; nodes.len()];
            dist[c] = 0;
            let mut q = VecDeque::from([c]);
            while let Some(i) = q.pop_front() {
                if dist[i] >= depth {
                    continue;
                }
                for &j in &adj[i] {
                    if dist[j] == usize::MAX {
                        dist[j] = dist[i] + 1;
                        q.push_back(j);
                    }
                }
            }
            dist.iter().map(|d| *d != usize::MAX).collect()
        }
        None if center.is_some() => vec![false; nodes.len()],
        None => nodes.iter().map(|n| f.orphans || n.degree > 0).collect(),
    };
    let mut remap = vec![usize::MAX; nodes.len()];
    let mut out = GraphData::default();
    for (i, n) in nodes.into_iter().enumerate() {
        if keep[i] {
            remap[i] = out.nodes.len();
            out.nodes.push(n);
        }
    }
    out.edges = edges.into_iter().filter(|(a, b)| keep[*a] && keep[*b]).map(|(a, b)| (remap[a], remap[b])).collect();
    // degrees within what is shown
    for n in out.nodes.iter_mut() {
        n.degree = 0;
    }
    for &(a, b) in &out.edges {
        out.nodes[a].degree += 1;
        out.nodes[b].degree += 1;
    }
    out
}

/// The graph's settings as stored (keys follow `.obsidian/graph.json`).
fn obsidian_graph(vault: &Path) -> Map<String, Value> {
    super::templates::obsidian_core_settings(vault, "graph.json")
}

#[derive(Clone, Copy, PartialEq)]
enum Drag {
    None,
    Pan(Point<Pixels>),
    Node { i: usize, at: Point<Pixels>, moved: bool },
}

pub struct GraphView {
    focus: FocusHandle,
    local: bool,
    /// The note a local graph is centred on.
    center: Option<String>,
    data: GraphData,
    sim: Sim,
    /// (index generation, centre, filters, depth) the data was built for.
    key: Option<(u64, Option<String>, Filters, usize)>,
    scale: f32,
    pan: Point<f32>,
    bounds: Bounds<Pixels>,
    hover: Option<usize>,
    drag: Drag,
    /// Keep the whole graph in view until you pan or zoom yourself.
    auto_fit: bool,
    /// Timing for `make perf` / UI tests: ticks run, total tick time, slowest frame.
    ticks: usize,
    tick_ms: f64,
    frame_ms: f64,
    /// Slowest paint (shared with the canvas closure).
    paint_ms: Rc<std::cell::Cell<f64>>,
    reported: bool,
}

fn setting_bool(cx: &App, key: &str, default: bool) -> bool {
    cx.try_global::<super::PluginHost>().and_then(|h| h.setting("graph", key).as_bool()).unwrap_or(default)
}
fn setting_f32(cx: &App, key: &str, default: f32) -> f32 {
    cx.try_global::<super::PluginHost>().and_then(|h| h.setting("graph", key).as_f64()).map(|v| v as f32).unwrap_or(default)
}

impl GraphView {
    pub fn new(local: bool, cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
            local,
            center: None,
            data: GraphData::default(),
            sim: Sim::new(0, &|_| None, vec![], Forces::default()),
            key: None,
            scale: if local { 0.6 } else { 0.35 },
            pan: point(0.0, 0.0),
            bounds: Bounds::default(),
            hover: None,
            drag: Drag::None,
            auto_fit: true,
            ticks: 0,
            tick_ms: 0.0,
            frame_ms: 0.0,
            paint_ms: Rc::default(),
            reported: false,
        }
    }

    /// Where node `id` is drawn (window coordinates).
    pub fn screen_pos(&self, id: &str) -> Option<Point<Pixels>> {
        let i = self.data.nodes.iter().position(|n| n.id == id)?;
        let p = self.to_screen(i);
        Some(point(px(p.x), px(p.y)))
    }

    pub fn set_center(&mut self, c: Option<String>) {
        self.center = c;
    }

    fn filters(&self, cx: &App) -> Filters {
        let p = if self.local { "local" } else { "" };
        let k = |s: &str| if p.is_empty() { s.to_string() } else { format!("{p}{}{}", s[..1].to_uppercase(), &s[1..]) };
        Filters {
            tags: setting_bool(cx, &k("showTags"), false),
            attachments: setting_bool(cx, &k("showAttachments"), false),
            unresolved: !setting_bool(cx, &k("hideUnresolved"), false),
            orphans: setting_bool(cx, &k("showOrphans"), true),
        }
    }

    fn forces(cx: &App) -> Forces {
        let d = Forces::default();
        Forces {
            center: setting_f32(cx, "centerStrength", d.center),
            repel: setting_f32(cx, "repelStrength", d.repel),
            link: setting_f32(cx, "linkStrength", d.link),
            distance: setting_f32(cx, "linkDistance", d.distance),
        }
    }

    /// Rebuilds nodes and edges when the vault, the centre or the filters changed; nodes that
    /// stay keep their place.
    fn refresh(&mut self, cx: &App) {
        let Some(vault) = WorkspaceHandle::current(cx).and_then(|w| w.vault(cx)) else { return };
        let f = self.filters(cx);
        let depth = setting_f32(cx, "localDepth", 1.0).clamp(1.0, 5.0) as usize;
        let gen = vault.borrow().index.generation();
        let center = if self.local { self.center.clone() } else { None };
        if self.local && center.is_none() {
            self.data = GraphData::default();
            self.sim = Sim::new(0, &|_| None, vec![], Forces::default());
            self.key = None;
            return;
        }
        let key = (gen, center.clone(), f.clone(), depth);
        let forces = Self::forces(cx);
        if self.key.as_ref() == Some(&key) {
            if self.sim.forces != forces {
                self.sim.set_forces(forces);
                self.sim.reheat(0.3);
            }
            return;
        }
        let t = Instant::now();
        let data = build(&vault.borrow().index, &f, center.as_deref(), depth);
        let old: HashMap<&str, (f32, f32)> = self.data.nodes.iter().enumerate().map(|(i, n)| (n.id.as_str(), (self.sim.x[i], self.sim.y[i]))).collect();
        let kept = data.nodes.iter().filter(|n| old.contains_key(n.id.as_str())).count();
        let starts: Vec<Option<(f32, f32)>> = data.nodes.iter().map(|n| old.get(n.id.as_str()).copied()).collect();
        let mut sim = Sim::new(data.nodes.len(), &|i| starts[i], data.edges.clone(), forces);
        if kept * 2 > data.nodes.len() {
            sim.alpha = 0.3; // a small change: a gentle re-settle
        }
        if std::env::var_os("OBSIDILIGHT_SCRIPT").is_some() {
            println!("GRAPH build {} nodes={} edges={} ({:.1} ms)", if self.local { "local" } else { "global" }, data.nodes.len(), data.edges.len(), t.elapsed().as_secs_f64() * 1000.0);
        }
        self.data = data;
        self.sim = sim;
        self.key = Some(key);
        self.hover = None;
        self.drag = Drag::None;
        self.reported = false;
    }

    fn radius(&self, i: usize, size_mul: f32) -> f32 {
        let d = self.data.nodes[i].degree as f32;
        (6.0 + d.sqrt() * 2.0).min(30.0) * size_mul
    }

    fn origin(&self) -> Point<f32> {
        let c = self.bounds.center();
        point(f32::from(c.x) + self.pan.x, f32::from(c.y) + self.pan.y)
    }

    fn to_screen(&self, i: usize) -> Point<f32> {
        let o = self.origin();
        point(o.x + self.sim.x[i] * self.scale, o.y + self.sim.y[i] * self.scale)
    }

    fn to_world(&self, p: Point<Pixels>) -> (f32, f32) {
        let o = self.origin();
        ((f32::from(p.x) - o.x) / self.scale, (f32::from(p.y) - o.y) / self.scale)
    }

    fn node_at(&self, p: Point<Pixels>, size_mul: f32) -> Option<usize> {
        let (px_, py_) = (f32::from(p.x), f32::from(p.y));
        let mut best: Option<(f32, usize)> = None;
        for i in 0..self.data.nodes.len() {
            let s = self.to_screen(i);
            let d = ((s.x - px_).powi(2) + (s.y - py_).powi(2)).sqrt();
            let r = (self.radius(i, size_mul) * self.scale).max(2.5) + 3.0;
            if d <= r && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, i));
            }
        }
        best.map(|b| b.1)
    }

    fn open_node(&self, i: usize, new_tab: bool, window: &mut Window, cx: &mut App) {
        let n = &self.data.nodes[i];
        let Some(ws) = WorkspaceHandle::current(cx) else { return };
        match n.kind {
            Kind::Note | Kind::Attachment => ws.open(&n.id, OpenAt::Keep, new_tab, window, cx),
            Kind::Unresolved => {
                let _ = ws.create_note(&n.id, new_tab, window, cx);
            }
            Kind::Tag => ws.show_view(super::search::VIEW, Some(format!("tag:{}", n.id)), cx),
        }
    }

    fn on_down(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        let size_mul = setting_f32(cx, "nodeSizeMultiplier", 1.0);
        self.drag = match self.node_at(ev.position, size_mul) {
            Some(i) => Drag::Node { i, at: ev.position, moved: false },
            None => Drag::Pan(ev.position),
        };
        cx.notify();
    }

    fn on_move(&mut self, ev: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        match self.drag {
            Drag::Pan(last) if ev.pressed_button == Some(MouseButton::Left) => {
                self.auto_fit = false;
                self.pan.x += f32::from(ev.position.x - last.x);
                self.pan.y += f32::from(ev.position.y - last.y);
                self.drag = Drag::Pan(ev.position);
                cx.notify();
            }
            Drag::Node { i, at, moved } if ev.pressed_button == Some(MouseButton::Left) => {
                let far = (f32::from(ev.position.x - at.x)).abs() + (f32::from(ev.position.y - at.y)).abs() > 3.0;
                if moved || far {
                    self.sim.fixed[i] = Some(self.to_world(ev.position));
                    self.sim.set_alpha_target(0.3);
                    self.drag = Drag::Node { i, at, moved: true };
                    cx.notify();
                }
            }
            _ => {
                let size_mul = setting_f32(cx, "nodeSizeMultiplier", 1.0);
                let h = self.node_at(ev.position, size_mul);
                if h != self.hover {
                    self.hover = h;
                    cx.notify();
                }
                if let Some(ws) = WorkspaceHandle::current(cx) {
                    let target = h.filter(|&i| self.data.nodes[i].kind == Kind::Note).map(|i| self.data.nodes[i].id.clone());
                    let want = target.as_ref().is_some_and(|t| super::PluginHost::wants_preview(cx, t, HoverSource::Sidebar, ev.modifiers.secondary(cx)));
                    ws.preview_link(if want { target.map(|t| (t, String::new())) } else { None }, ev.position, cx);
                }
            }
        }
    }

    fn on_up(&mut self, ev: &MouseUpEvent, window: &mut Window, cx: &mut Context<Self>) {
        if let Drag::Node { i, moved, .. } = self.drag {
            self.sim.fixed[i] = None;
            self.sim.set_alpha_target(0.0);
            if !moved {
                self.open_node(i, ev.modifiers.secondary(cx), window, cx);
            }
        }
        self.drag = Drag::None;
        cx.notify();
    }

    fn on_scroll(&mut self, ev: &ScrollEvent, _: &mut Window, cx: &mut Context<Self>) {
        let dy = f32::from(ev.delta.pixel_delta(px(20.)).y);
        let factor = (dy * 0.002).exp();
        let new = (self.scale * factor).clamp(0.02, 8.0);
        self.auto_fit = false;
        // zoom around the mouse: the world point under it stays put
        let (wx, wy) = self.to_world(ev.position);
        self.scale = new;
        let o = self.origin();
        let (sx, sy) = (o.x + wx * new, o.y + wy * new);
        self.pan.x += f32::from(ev.position.x) - sx;
        self.pan.y += f32::from(ev.position.y) - sy;
        cx.notify();
    }

    fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let f = self.filters(cx);
        let p = if self.local { "local" } else { "" };
        let key = move |s: &str| if p.is_empty() { s.to_string() } else { format!("{p}{}{}", s[..1].to_uppercase(), &s[1..]) };
        let toggle = |id: &'static str, label: &'static str, on: bool, setting: String, invert: bool| {
            Button::new(id).label(label).xsmall().ghost().selected(on).on_click(move |_, _, cx| {
                let v = if invert { on } else { !on };
                super::PluginHost::set_setting(cx, "graph", &setting, Value::Bool(v));
            })
        };
        let depth = setting_f32(cx, "localDepth", 1.0) as i64;
        h_flex()
            .absolute()
            .top_1()
            .right_1()
            .gap_0p5()
            .p_0p5()
            .rounded(px(6.))
            .bg(cx.theme().background.opacity(0.85))
            .border_1()
            .border_color(cx.theme().border)
            .child(toggle("g-tags", "Tags", f.tags, key("showTags"), false))
            .child(toggle("g-att", "Attachments", f.attachments, key("showAttachments"), false))
            .child(toggle("g-unres", "Unresolved", f.unresolved, key("hideUnresolved"), true))
            .when(!self.local, |d| d.child(toggle("g-orph", "Orphans", f.orphans, key("showOrphans"), false)))
            .when(self.local, |d| {
                d.child(Button::new("g-depth").label(format!("Depth {depth}")).xsmall().ghost().tooltip("Links away from the note (click to change)").on_click(move |_, _, cx| {
                    super::PluginHost::set_setting(cx, "graph", "localDepth", Value::from(if depth >= 3 { 1 } else { depth + 1 }));
                }))
            })
            .child(Button::new("g-fit").label("Fit").xsmall().ghost().on_click(cx.listener(|g, _, _, cx| {
                g.auto_fit = true;
                g.fit();
                cx.notify();
            })))
    }

    /// Zooms and pans so the whole graph is in view.
    fn fit(&mut self) {
        let n = self.data.nodes.len();
        if n == 0 {
            return;
        }
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for i in 0..n {
            x0 = x0.min(self.sim.x[i]);
            x1 = x1.max(self.sim.x[i]);
            y0 = y0.min(self.sim.y[i]);
            y1 = y1.max(self.sim.y[i]);
        }
        let (w, h) = (f32::from(self.bounds.size.width).max(50.0), f32::from(self.bounds.size.height).max(50.0));
        self.scale = ((w - 60.0) / (x1 - x0).max(1.0)).min((h - 60.0) / (y1 - y0).max(1.0)).clamp(0.02, 2.0);
        self.pan = point(-(x0 + x1) / 2.0 * self.scale, -(y0 + y1) / 2.0 * self.scale);
    }
}

impl Focusable for GraphView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for GraphView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let frame = Instant::now();
        self.refresh(cx);
        // run the simulation a few ticks per frame while it is warm
        if self.sim.running() && !self.data.nodes.is_empty() {
            let t = Instant::now();
            let per_frame = if self.data.nodes.len() > 800 { 2 } else { 3 };
            for _ in 0..per_frame {
                self.sim.tick();
                self.ticks += 1;
            }
            self.tick_ms += t.elapsed().as_secs_f64() * 1000.0;
            window.request_animation_frame();
        } else if !self.reported && !self.data.nodes.is_empty() {
            self.reported = true;
            if std::env::var_os("OBSIDILIGHT_SCRIPT").is_some() || std::env::var_os("OBSIDILIGHT_TIMING").is_some() {
                println!("GRAPH settled {} nodes={} ticks={} tick_avg_ms={:.2} frame_max_ms={:.1} paint_max_ms={:.1}", if self.local { "local" } else { "global" }, self.data.nodes.len(), self.ticks, self.tick_ms / self.ticks.max(1) as f64, self.frame_ms, self.paint_ms.get());
            }
        }
        if self.auto_fit && self.bounds.size.width > px(0.) {
            self.fit();
        }
        let theme = cx.theme().clone();
        let size_mul = setting_f32(cx, "nodeSizeMultiplier", 1.0);
        let line_mul = setting_f32(cx, "lineSizeMultiplier", 1.0);
        let active = WorkspaceHandle::current(cx).and_then(|w| w.active_note(cx));
        // what to draw, in screen coordinates
        let n = self.data.nodes.len();
        let pos: Vec<Point<f32>> = (0..n).map(|i| self.to_screen(i)).collect();
        let radii: Vec<f32> = (0..n).map(|i| (self.radius(i, size_mul) * self.scale).max(2.5)).collect();
        let hover = self.hover;
        let neighbours: HashSet<usize> = match hover {
            Some(h) => self.data.edges.iter().filter_map(|&(a, b)| if a == h { Some(b) } else if b == h { Some(a) } else { None }).collect(),
            None => HashSet::new(),
        };
        let edges = self.data.edges.clone();
        let colors = (theme.muted_foreground, theme.primary, theme.warning, theme.success, theme.muted_foreground.opacity(0.4), theme.border);
        let kinds: Vec<Kind> = self.data.nodes.iter().map(|n| n.kind).collect();
        let current: Option<usize> = active.as_ref().and_then(|a| self.data.nodes.iter().position(|n| &n.id == a));
        // labels: zoomed in enough, or the hovered node and its neighbours
        let show_all_labels = self.scale >= 0.55 || n <= 40;
        let labels: Vec<(usize, SharedString)> = (0..n)
            .filter(|&i| show_all_labels || Some(i) == hover || neighbours.contains(&i) || Some(i) == current)
            .map(|i| (i, SharedString::from(self.data.nodes[i].label.clone())))
            .collect();
        let font = window.text_style().font();
        let fg = theme.foreground;
        let this = cx.entity().downgrade();
        if !self.local {
            LAST.with(|l| *l.borrow_mut() = Some(this.clone()));
        }
        let paint_ms = self.paint_ms.clone();
        let painter = canvas(
            move |bounds, _, cx| {
                let _ = this.update(cx, |g, _| g.bounds = bounds);
            },
            move |bounds, _, window, cx| {
                let started = Instant::now();
                let inside = |p: Point<f32>, r: f32| p.x + r >= f32::from(bounds.left()) && p.x - r <= f32::from(bounds.right()) && p.y + r >= f32::from(bounds.top()) && p.y - r <= f32::from(bounds.bottom());
                let (note_c, accent, attach_c, tag_c, unres_c, line_c) = colors;
                window.with_content_mask(Some(ContentMask { bounds }), |window| {
                    // edges: one path per few thousand segments (16-bit vertex indices)
                    let lw = px((1.0 * line_mul).max(0.5));
                    for (hl, color) in [(false, line_c), (true, accent)] {
                        for chunk in edges.chunks(2000) {
                            let mut pb = PathBuilder::stroke(lw);
                            let mut any = false;
                            for &(a, b) in chunk {
                                let on = hover.is_some_and(|h| a == h || b == h);
                                if on != hl || (!inside(pos[a], 0.0) && !inside(pos[b], 0.0)) {
                                    continue;
                                }
                                pb.move_to(point(px(pos[a].x), px(pos[a].y)));
                                pb.line_to(point(px(pos[b].x), px(pos[b].y)));
                                any = true;
                            }
                            if any {
                                if let Ok(path) = pb.build() {
                                    window.paint_path(path, color);
                                }
                            }
                        }
                    }
                    // nodes
                    for i in 0..pos.len() {
                        let (p, r) = (pos[i], radii[i]);
                        if !inside(p, r) {
                            continue;
                        }
                        let base = match kinds[i] {
                            Kind::Note => note_c,
                            Kind::Attachment => attach_c,
                            Kind::Tag => tag_c,
                            Kind::Unresolved => unres_c,
                        };
                        let dim = hover.is_some() && Some(i) != hover && !neighbours.contains(&i);
                        let c = if Some(i) == hover || Some(i) == current { accent } else if dim { base.opacity(0.35) } else { base };
                        let b = Bounds::new(point(px(p.x - r), px(p.y - r)), size(px(2.0 * r), px(2.0 * r)));
                        window.paint_quad(fill(b, c).corner_radii(px(r)));
                    }
                    // labels under their nodes
                    let fs = px(12.);
                    for (i, text) in &labels {
                        let (p, r) = (pos[*i], radii[*i]);
                        if !inside(p, r + 60.0) {
                            continue;
                        }
                        let dim = hover.is_some() && Some(*i) != hover && !neighbours.contains(i);
                        let color = if dim { fg.opacity(0.3) } else { fg };
                        let run = TextRun { len: text.len(), font: font.clone(), color, background_color: None, underline: None, strikethrough: None };
                        let line = window.text_system().shape_line(text.clone(), fs, &[run], None);
                        let origin = point(px(p.x) - line.width / 2.0, px(p.y + r + 2.0));
                        let _ = line.paint(origin, px(14.), TextAlign::Left, None, window, cx);
                    }
                });
                paint_ms.set(paint_ms.get().max(started.elapsed().as_secs_f64() * 1000.0));
            },
        )
        .size_full();
        let empty = self.data.nodes.is_empty();
        let msg = if self.local && self.center.is_none() { "Open a note to see its links." } else { "Nothing to show with these filters." };
        let el = div()
            .id(if self.local { "local-graph" } else { "graph" })
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .min_h(px(120.))
            .overflow_hidden()
            .bg(theme.background)
            .cursor(if self.hover.is_some() { CursorStyle::PointingHand } else { CursorStyle::Arrow })
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_down))
            .on_mouse_move(cx.listener(Self::on_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_up))
            .on_scroll(cx.listener(Self::on_scroll))
            .on_hover(cx.listener(|g, hovered: &bool, window, cx| {
                if !*hovered {
                    g.hover = None;
                    if let Some(ws) = WorkspaceHandle::current(cx) {
                        ws.preview_link(None, window.mouse_position(), cx);
                    }
                    cx.notify();
                }
            }))
            .child(painter)
            .when(empty, |d| d.child(div().absolute().inset_0().flex().items_center().justify_center().text_sm().text_color(theme.muted_foreground).child(msg)))
            .child(self.toolbar(cx));
        self.frame_ms = self.frame_ms.max(frame.elapsed().as_secs_f64() * 1000.0);
        el
    }
}

pub struct Graph {
    manifest: PluginManifest,
}

impl Graph {
    pub fn new(vault: &Path) -> Self {
        let g = obsidian_graph(vault);
        let b = |k: &str, d: bool| Value::Bool(g.get(k).and_then(Value::as_bool).unwrap_or(d));
        let n = |k: &str, d: f64| Value::from(g.get(k).and_then(Value::as_f64).unwrap_or(d));
        let toggle = |key, label, description, default| SettingSpec { key, label, description, kind: SettingKind::Toggle, default };
        let number = |key, label, description, default| SettingSpec { key, label, description, kind: SettingKind::Number, default };
        let hidden = |key, default| SettingSpec { key, label: "", description: "", kind: SettingKind::Hidden, default };
        Self {
            manifest: PluginManifest {
                id: "graph",
                name: "Graph view",
                version: "0.1.0",
                description: "Your notes and their links as a graph: the whole vault in a tab, or the active note's neighbourhood in the right sidebar.",
                enabled_by_default: true,
                settings: vec![
                    toggle("showTags", "Tags", "Show tags as nodes.", b("showTags", false)),
                    toggle("showAttachments", "Attachments", "Show images and other files.", b("showAttachments", false)),
                    toggle("hideUnresolved", "Existing files only", "Hide links to notes that don't exist.", b("hideUnresolved", false)),
                    toggle("showOrphans", "Orphans", "Show notes without links.", b("showOrphans", true)),
                    number("centerStrength", "Center force", "0–1: how strongly nodes are pulled to the middle.", n("centerStrength", 0.52)),
                    number("repelStrength", "Repel force", "0–20: how strongly nodes push each other away.", n("repelStrength", 10.0)),
                    number("linkStrength", "Link force", "0–1: how strongly links pull.", n("linkStrength", 1.0)),
                    number("linkDistance", "Link distance", "30–500: the length links settle at.", n("linkDistance", 250.0)),
                    number("nodeSizeMultiplier", "Node size", "Multiplier.", n("nodeSizeMultiplier", 1.0)),
                    number("lineSizeMultiplier", "Link thickness", "Multiplier.", n("lineSizeMultiplier", 1.0)),
                    number("localDepth", "Local graph depth", "1–3: links away from the note.", Value::from(1)),
                    hidden("localShowTags", Value::Bool(false)),
                    hidden("localShowAttachments", Value::Bool(false)),
                    hidden("localHideUnresolved", Value::Bool(false)),
                ],
                code_languages: &[],
            },
        }
    }
}

impl Plugin for Graph {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }
    fn on_load(&mut self, _settings: &Map<String, Value>, reg: &mut Registrar) {
        reg.tab_view(TabViewType {
            id: "graph".into(),
            title: "Graph view".into(),
            create: Rc::new(|_, _, cx| {
                let v = cx.new(|cx| GraphView::new(false, cx));
                let f = v.read(cx).focus.clone();
                (v.into(), f)
            }),
        });
        let open: Action = Rc::new(|a: &mut ActionCtx| {
            if let Some(ws) = a.workspace() {
                ws.open_view(VIEW, None, a.window, a.cx);
            }
        });
        reg.command(Command { id: "open".into(), name: "Open graph view".into(), hotkey: Some("ctrl-g".into()), run: open.clone() });
        reg.ribbon(RibbonItem { id: "graph".into(), icon: gpui_kit::assets::IconName::GitFork, tooltip: "Open graph view".into(), run: open });
        // the local graph: one view for the sidebar, made on first use, following the active note
        let local: Rc<RefCell<Option<Entity<GraphView>>>> = Rc::default();
        reg.sidebar_view(SidebarView {
            id: "local".into(),
            title: "Local graph".into(),
            icon: gpui_kit::assets::IconName::GitFork,
            side: Side::Right,
            render: Rc::new(move |ctx, _window, cx| {
                let v = local.borrow_mut().get_or_insert_with(|| cx.new(|cx| GraphView::new(true, cx))).clone();
                let center = ctx.active_note.clone();
                if v.read(cx).center != center {
                    v.update(cx, |g, _| g.set_center(center));
                }
                div().size_full().child(v).into_any_element()
            }),
        });
        reg.command(Command {
            id: "local".into(),
            name: "Open local graph".into(),
            hotkey: None,
            run: Rc::new(|a: &mut ActionCtx| {
                if let Some(ws) = a.workspace() {
                    ws.show_view(LOCAL_VIEW, None, a.cx);
                }
            }),
        });
    }
}

#[cfg(test)]
mod tests {
    // no `gpui_kit::*` glob here: its `test` macro would shadow the built-in one
    use super::{build, Filters, GraphData, Kind};
    use obsidilight_core::index::VaultIndex;

    fn vault(files: &[(&str, &str)]) -> (std::path::PathBuf, VaultIndex) {
        let dir = std::env::temp_dir().join(format!("ol-graph-{}-{}", std::process::id(), files.len()));
        let _ = std::fs::remove_dir_all(&dir);
        for (p, t) in files {
            let f = dir.join(p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, t).unwrap();
        }
        let ix = VaultIndex::build(&dir);
        (dir, ix)
    }

    #[test]
    fn nodes_edges_filters_and_neighbourhoods() {
        let (dir, ix) = vault(&[
            ("A.md", "[[B]] [[B#h]] ![[img.png]] [[Missing]] #topic"),
            ("B.md", "[[C]]"),
            ("C.md", ""),
            ("Lonely.md", "nothing"),
            ("img.png", ""),
        ]);
        let all = Filters { tags: false, attachments: false, unresolved: true, orphans: true };
        let g = build(&ix, &all, None, 1);
        let ids = |g: &GraphData| g.nodes.iter().map(|n| n.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(&g), ["A.md", "B.md", "C.md", "Lonely.md", "Missing"]);
        assert_eq!(g.edges.len(), 3, "A-B once (two links), B-C, A-Missing");
        assert_eq!(g.nodes[0].degree, 2);
        let more = Filters { tags: true, attachments: true, unresolved: false, orphans: false };
        let g = build(&ix, &more, None, 1);
        assert_eq!(ids(&g), ["A.md", "B.md", "C.md", "img.png", "#topic"]);
        assert_eq!(g.nodes.iter().find(|n| n.id == "#topic").unwrap().kind, Kind::Tag);
        // local: depth 1 from C is B; depth 2 adds A (and A's unresolved link at depth 3 only)
        assert_eq!(ids(&build(&ix, &all, Some("C.md"), 1)), ["B.md", "C.md"]);
        assert_eq!(ids(&build(&ix, &all, Some("C.md"), 2)), ["A.md", "B.md", "C.md"]);
        assert!(build(&ix, &all, Some("Nope.md"), 1).nodes.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
