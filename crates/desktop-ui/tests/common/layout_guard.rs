//! A guard against overlapping widgets: for a rendered kittest frame it collects the rects of the
//! accesskit nodes and reports every pair that intersects and every node that leaves the viewport.
//!
//! egui reports unclipped bounds, so the content of a scroll area would look like it left the
//! window. In the accesskit tree a scroll bar is a child of the node that holds the scroll
//! content; its y range, up to its right edge, is the area the content is visible in. Content is
//! clipped to that area, and a node with nothing left is scrolled out and skipped. Text runs are
//! the parts of a label and containers hold no widget; both are left out.

use egui::{Rect, pos2};
use egui_kittest::kittest::NodeT as _;
use egui_kittest::{Harness, Node};

/// Overlaps and overshoots smaller than this many points are rounding, not a defect.
const SLACK: f32 = 1.0;

/// One widget as the guard sees it.
#[derive(Clone, Debug)]
pub struct Item {
    pub role: String,
    pub label: String,
    pub rect: Rect,
}

impl std::fmt::Display for Item {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {:?} {:?}", self.role, self.label, self.rect)
    }
}

/// The widgets of the frame with their visible rects (see the module note on scroll areas).
pub fn items<S>(h: &Harness<'_, S>) -> Vec<Item> {
    let mut out = Vec::new();
    walk(&h.root(), None, &mut out);
    out
}

fn bounds(node: &Node<'_>) -> Option<Rect> {
    let b = node.accesskit_node().raw_bounds()?;
    Some(Rect::from_min_max(
        pos2(b.x0 as f32, b.y0 as f32),
        pos2(b.x1 as f32, b.y1 as f32),
    ))
}

fn role(node: &Node<'_>) -> String {
    format!("{:?}", node.accesskit_node().role())
}

fn walk(node: &Node<'_>, clip: Option<Rect>, out: &mut Vec<Item>) {
    let children: Vec<Node<'_>> = node.children().collect();
    // A scroll bar among the children: they are the scroll area's content.
    let clip = children
        .iter()
        .find(|c| role(c) == "ScrollBar")
        .and_then(bounds)
        .map_or(clip, |bar| {
            let area = Rect::from_min_max(pos2(-1.0e6, bar.min.y), pos2(bar.max.x, bar.max.y));
            Some(clip.map_or(area, |c| c.intersect(area)))
        });
    for child in &children {
        let kind = role(child);
        if kind == "ScrollBar" {
            continue;
        }
        if !matches!(kind.as_str(), "TextRun" | "GenericContainer" | "Window" | "Pane" | "Group")
            && !child.accesskit_node().is_hidden()
            && let Some(mut rect) = bounds(child)
        {
            if let Some(c) = clip {
                rect = rect.intersect(c);
            }
            if rect.width() > 0.0 && rect.height() > 0.0 {
                out.push(Item {
                    role: kind.clone(),
                    label: child.accesskit_node().label().unwrap_or_default(),
                    rect,
                });
            }
        }
        if kind != "TextRun" {
            walk(child, clip, out);
        }
    }
}

/// A card-like choice (a radio row, a check row) draws its own title and description inside its
/// rect: a node entirely inside such a widget is part of it, not an overlap.
fn nested(a: &Item, b: &Item) -> bool {
    let inside = |outer: &Item, inner: &Item| {
        matches!(outer.role.as_str(), "RadioButton" | "CheckBox" | "Link" | "Tab")
            && outer.rect.expand(SLACK).contains_rect(inner.rect)
    };
    inside(a, b) || inside(b, a)
}

/// Everything wrong with the layout of the frame, one line each; empty when it is clean.
pub fn problems<S>(h: &Harness<'_, S>, viewport: Rect) -> Vec<String> {
    let items = items(h);
    let mut out = Vec::new();
    for (i, a) in items.iter().enumerate() {
        let outside = a.rect.min.x < viewport.min.x - SLACK
            || a.rect.min.y < viewport.min.y - SLACK
            || a.rect.max.x > viewport.max.x + SLACK
            || a.rect.max.y > viewport.max.y + SLACK;
        if outside {
            out.push(format!("outside the window {viewport:?}: {a}"));
        }
        for b in &items[i + 1..] {
            let both = a.rect.intersect(b.rect);
            if both.width() > SLACK && both.height() > SLACK && !nested(a, b) {
                out.push(format!("overlap {:?}: {a}  <>  {b}", both.size()));
            }
        }
    }
    out
}

/// Panic with every problem, naming `what` was rendered.
#[track_caller]
pub fn assert_clean<S>(h: &Harness<'_, S>, viewport: Rect, what: &str) {
    let found = problems(h, viewport);
    assert!(found.is_empty(), "{what}: {} layout problem(s)\n{}", found.len(), found.join("\n"));
}
