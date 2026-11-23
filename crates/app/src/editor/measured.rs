//! Wraps one rendered block and records its laid-out height (for virtualized rendering).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gpui_kit::*;

pub type Heights = Rc<RefCell<HashMap<u64, f32>>>;

pub struct Measured {
    hash: u64,
    /// The block's stable id: the height is recorded under both, so a re-typed block (new
    /// hash) still has its previous height available until it re-measures.
    id: u64,
    heights: Heights,
    id_heights: Heights,
    child: Option<AnyElement>,
}

impl Measured {
    pub fn new(hash: u64, id: u64, heights: Heights, id_heights: Heights, child: AnyElement) -> Self {
        Self { hash, id, heights, id_heights, child: Some(child) }
    }
}

impl IntoElement for Measured {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for Measured {
    type RequestLayoutState = AnyElement;
    type PrepaintState = ();
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    fn request_layout(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, window: &mut Window, cx: &mut App) -> (LayoutId, AnyElement) {
        let mut child = self.child.take().expect("child");
        let id = child.request_layout(window, cx);
        let layout = window.request_layout(Style { size: Size { width: relative(1.).into(), height: Length::Auto }, ..Default::default() }, [id], cx);
        (layout, child)
    }
    fn prepaint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, bounds: Bounds<Pixels>, child: &mut AnyElement, window: &mut Window, cx: &mut App) {
        let h = f32::from(bounds.size.height);
        self.heights.borrow_mut().insert(self.hash, h);
        self.id_heights.borrow_mut().insert(self.id, h);
        child.prepaint(window, cx);
    }
    fn paint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, _: Bounds<Pixels>, child: &mut AnyElement, _: &mut (), window: &mut Window, cx: &mut App) {
        child.paint(window, cx);
    }
}
