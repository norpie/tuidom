//! View-layer tests. Everything is asserted through the headless screen — painted cells,
//! not internals — because that is the only ground truth for "the signal reached the node".

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tuidom::animation::{
    AnimatableProperty, Easing, KeyframeAnimation, TransitionConfig, TransitionProperty,
};
use tuidom::event::{KeyCode, MouseButton};
use tuidom::headless::{HeadlessRuntime, ScreenColor};
use tuidom::style::{Color, FlexDirection, FlexGap, FlexWrap, Length, Overflow, Style};
use tuidom::{Document, NodeId};

use super::{
    Children, View, boxed, dyn_text, dynamic, fragment, keyed, mount, node_ref, text, use_document,
    virtual_list, virtual_list_measured,
};
use crate::testing::Harness;
use crate::{Callback, Signal, state};

/// A column container with a one-row gap between its children.
fn gapped_column() -> Style {
    let mut style = Style::new();
    style.flex_direction(FlexDirection::Column);
    style.gap(FlexGap::new(1, 0));
    style
}

/// A column container with no gap, so each child lands on its own contiguous row.
fn column() -> Style {
    let mut style = Style::new();
    style.flex_direction(FlexDirection::Column);
    style
}

/// A box sized to cover a few cells, so a click can land on it.
fn sized_box_style() -> Style {
    let mut style = Style::new();
    style.width(Length::Cells(4));
    style.height(Length::Cells(1));
    style
}

/// Read the glyphs of the first `n` cells of row 0 as a string.
fn row0(rt: &HeadlessRuntime, n: u16) -> String {
    read_row(rt, 0, n)
}

/// Read the glyphs of the first `n` cells of row `y` as a string.
fn read_row(rt: &HeadlessRuntime, y: i32, n: u16) -> String {
    (0..n as i32)
        .filter_map(|x| rt.get_cell(x, y).map(|c| c.text))
        .collect()
}

// ---- Step 1: static mount ---------------------------------------------------------------

#[test]
fn a_static_text_paints() {
    let doc = Document::new().unwrap();
    let _mounted = mount(&doc, doc.root(), text("Hi")).unwrap();

    let mut rt = HeadlessRuntime::new(doc, 10, 1);
    rt.render().unwrap();

    assert_eq!(rt.get_cell(0, 0).unwrap().text, "H");
    assert_eq!(rt.get_cell(1, 0).unwrap().text, "i");
}

#[test]
fn a_fragment_mounts_siblings_in_order_with_no_wrapper() {
    let doc = Document::new().unwrap();
    // A fragment adds no flex level, so the two texts sit adjacently under the root.
    let _mounted = mount(&doc, doc.root(), fragment([text("ab"), text("cd")])).unwrap();
    // Two direct children of the root, not one wrapper holding them.
    assert_eq!(doc.get_children(doc.root()).len(), 2);

    let mut rt = HeadlessRuntime::new(doc, 10, 1);
    rt.render().unwrap();
    assert_eq!(row0(&rt, 4), "abcd");
}

#[test]
fn nested_elements_mount_under_their_parent() {
    let doc = Document::new().unwrap();
    let view = boxed().child(text("x")).child(boxed().child(text("y")));
    let _mounted = mount(&doc, doc.root(), view).unwrap();

    // root > box > { text("x"), box > text("y") }
    let outer = doc.get_children(doc.root());
    assert_eq!(outer.len(), 1);
    assert_eq!(doc.get_children(outer[0]).len(), 2);
}

#[test]
fn unmount_removes_every_node() {
    let doc = Document::new().unwrap();
    let mounted = mount(&doc, doc.root(), boxed().child(text("Hi"))).unwrap();

    let mut rt = HeadlessRuntime::new(doc.clone(), 10, 1);
    rt.render().unwrap();
    assert_eq!(rt.get_cell(0, 0).unwrap().text, "H");

    mounted.unmount();
    assert!(doc.get_children(doc.root()).is_empty());

    rt.render().unwrap();
    assert_ne!(rt.get_cell(0, 0).unwrap().text, "H");
}

// ---- Step 2: reactive bindings ----------------------------------------------------------

#[test]
fn a_text_binding_tracks_its_signal() {
    let doc = Document::new().unwrap();
    let label = state(String::from("A"));
    let _mounted = mount(&doc, doc.root(), dyn_text(move || label.get())).unwrap();

    let mut rt = HeadlessRuntime::new(doc, 10, 1);
    rt.render().unwrap();
    assert_eq!(rt.get_cell(0, 0).unwrap().text, "A");

    label.set(String::from("B")); // flush reruns the text effect, which repaints
    rt.render().unwrap();
    assert_eq!(rt.get_cell(0, 0).unwrap().text, "B");
}

#[test]
fn a_focusable_binding_marks_the_node() {
    let doc = Document::new().unwrap();
    let mounted = mount(&doc, doc.root(), boxed().focusable(true)).unwrap();
    let node = mounted.roots()[0];

    assert!(doc.is_focusable(node).unwrap());
}

#[test]
fn each_binding_is_its_own_effect() {
    // A box with a reactive style and a reactive-text child. Changing the text signal must
    // rerun only the text binding, never re-evaluate the style — the granularity guarantee.
    let doc = Document::new().unwrap();
    let label = state(String::from("a"));
    let bg = state(0i64);
    let style_runs = Arc::new(AtomicUsize::new(0));

    let sr = style_runs.clone();
    let view = boxed()
        .dyn_style(move || {
            sr.fetch_add(1, Ordering::SeqCst);
            let _ = bg.get(); // the style's only dependency
            Style::new()
        })
        .child(dyn_text(move || label.get()));
    let _mounted = mount(&doc, doc.root(), view).unwrap();

    let mut rt = HeadlessRuntime::new(doc, 10, 1);
    rt.render().unwrap();
    assert_eq!(style_runs.load(Ordering::SeqCst), 1); // ran once at mount

    label.set(String::from("b")); // text dep only
    assert_eq!(style_runs.load(Ordering::SeqCst), 1); // style untouched

    bg.set(1); // style dep
    assert_eq!(style_runs.load(Ordering::SeqCst), 2); // style reran
}

// ---- Step 3: listeners through the owner scope ------------------------------------------

#[test]
fn a_click_listener_fires() {
    let doc = Document::new().unwrap();
    let clicks = Arc::new(AtomicUsize::new(0));

    let c = clicks.clone();
    let view = boxed().style(sized_box_style()).on_click(move |_| {
        c.fetch_add(1, Ordering::SeqCst);
    });
    let _mounted = mount(&doc, doc.root(), view).unwrap();

    let mut rt = HeadlessRuntime::new(doc, 10, 1);
    rt.render().unwrap(); // lay the box out before hit-testing
    rt.simulate_click(0, 0);

    assert_eq!(clicks.load(Ordering::SeqCst), 1);
}

#[test]
fn unmount_removes_the_listener() {
    let doc = Document::new().unwrap();
    let clicks = Arc::new(AtomicUsize::new(0));

    let c = clicks.clone();
    let view = boxed().style(sized_box_style()).on_click(move |_| {
        c.fetch_add(1, Ordering::SeqCst);
    });
    let mounted = mount(&doc, doc.root(), view).unwrap();

    let mut rt = HeadlessRuntime::new(doc, 10, 1);
    rt.render().unwrap();
    rt.simulate_click(0, 0);
    assert_eq!(clicks.load(Ordering::SeqCst), 1);

    // Dispose runs the scope cleanup that removes the listener, and removes the node. The
    // node's removal is co-sufficient to stop the click, so this asserts the *outcome* the
    // cleanup exists to guarantee — no stale handler firing — with the mechanism itself
    // unit-tested in the runtime's cleanup tests.
    mounted.unmount();
    rt.render().unwrap();
    rt.simulate_click(0, 0);
    assert_eq!(clicks.load(Ordering::SeqCst), 1);
}

// ---- Step 4: the anchor and dynamic blocks ----------------------------------------------

#[test]
fn a_dynamic_block_swaps_content_on_a_structural_change() {
    let doc = Document::new().unwrap();
    let show = state(true);
    let _mounted = mount(
        &doc,
        doc.root(),
        dynamic(move || if show.get() { text("Y") } else { text("N") }),
    )
    .unwrap();

    let mut rt = HeadlessRuntime::new(doc, 10, 1);
    rt.render().unwrap();
    assert_eq!(rt.get_cell(0, 0).unwrap().text, "Y");

    show.set(false); // reruns the block: old text unmounted, new mounted before the marker
    rt.render().unwrap();
    assert_eq!(rt.get_cell(0, 0).unwrap().text, "N");
}

#[test]
fn an_empty_dynamic_block_costs_no_layout() {
    // The load-bearing anchor test. In a gap:1 column, an *out-of-flow* marker adds no flow
    // slot, so "B" sits one gap below "A" — row 2. An in-flow marker (even zero-sized) would
    // draw a gap around itself and push "B" further down. Row 2 proves the marker is absolute.
    let doc = Document::new().unwrap();
    let view = boxed()
        .style(gapped_column())
        .child(text("A"))
        .child(dynamic(|| fragment(Vec::<View>::new())))
        .child(text("B"));
    let _mounted = mount(&doc, doc.root(), view).unwrap();

    let mut rt = HeadlessRuntime::new(doc, 10, 5);
    rt.render().unwrap();

    assert_eq!(rt.get_cell(0, 0).unwrap().text, "A");
    assert_eq!(rt.get_cell(0, 2).unwrap().text, "B");
}

#[test]
fn content_bindings_update_without_remounting_the_block() {
    // A signal read only by the content's *binding* (not while producing the view) updates
    // in place: the text changes, but `produce` is not rerun — the block is not remounted.
    let doc = Document::new().unwrap();
    let inner = state(String::from("x"));
    let produce_runs = Arc::new(AtomicUsize::new(0));

    let pr = produce_runs.clone();
    let _mounted = mount(
        &doc,
        doc.root(),
        dynamic(move || {
            pr.fetch_add(1, Ordering::SeqCst);
            dyn_text(move || inner.get())
        }),
    )
    .unwrap();

    let mut rt = HeadlessRuntime::new(doc, 10, 1);
    rt.render().unwrap();
    assert_eq!(rt.get_cell(0, 0).unwrap().text, "x");
    assert_eq!(produce_runs.load(Ordering::SeqCst), 1);

    inner.set(String::from("y"));
    rt.render().unwrap();
    assert_eq!(rt.get_cell(0, 0).unwrap().text, "y");
    assert_eq!(produce_runs.load(Ordering::SeqCst), 1); // not remounted
}

#[test]
fn sibling_dynamic_blocks_are_independent() {
    let doc = Document::new().unwrap();
    let a = state(0i64);
    let b = state(0i64);
    let a_runs = Arc::new(AtomicUsize::new(0));
    let b_runs = Arc::new(AtomicUsize::new(0));

    let ar = a_runs.clone();
    let br = b_runs.clone();
    let view = boxed()
        .child(dynamic(move || {
            ar.fetch_add(1, Ordering::SeqCst);
            let _ = a.get();
            text("a")
        }))
        .child(dynamic(move || {
            br.fetch_add(1, Ordering::SeqCst);
            let _ = b.get();
            text("b")
        }));
    let _mounted = mount(&doc, doc.root(), view).unwrap();

    assert_eq!(a_runs.load(Ordering::SeqCst), 1);
    assert_eq!(b_runs.load(Ordering::SeqCst), 1);

    a.set(1); // reruns only block a
    assert_eq!(a_runs.load(Ordering::SeqCst), 2);
    assert_eq!(b_runs.load(Ordering::SeqCst), 1);
}

// ---- Step 5: deferred children as a thunk -----------------------------------------------

#[test]
fn a_children_thunk_is_deferred_until_mount() {
    let doc = Document::new().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));

    let c = calls.clone();
    let children = Children::new(move || {
        c.fetch_add(1, Ordering::SeqCst);
        text("x")
    });
    assert_eq!(calls.load(Ordering::SeqCst), 0); // constructing the thunk did not call it

    let container = boxed().child(children);
    assert_eq!(calls.load(Ordering::SeqCst), 0); // building the container did not call it

    let _mounted = mount(&doc, doc.root(), container).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1); // the framework calls it at mount
}

#[test]
fn the_container_survives_a_children_swap() {
    // Deferred children mount through a dynamic slot, so swapping them leaves the container
    // box in place — the invariant that keeps scroll, focus, and transitions alive.
    let doc = Document::new().unwrap();
    let which = state(true);
    let container = boxed().child(Children::new(move || {
        if which.get() { text("A") } else { text("B") }
    }));
    let mounted = mount(&doc, doc.root(), container).unwrap();
    let container_node = mounted.roots()[0];

    let mut rt = HeadlessRuntime::new(doc.clone(), 10, 1);
    rt.render().unwrap();
    assert_eq!(rt.get_cell(0, 0).unwrap().text, "A");

    which.set(false); // children swap
    rt.render().unwrap();
    assert_eq!(rt.get_cell(0, 0).unwrap().text, "B");

    // Same container node id before and after — it was never torn down and rebuilt.
    assert_eq!(doc.get_children(doc.root()), vec![container_node]);
}

// ---- Step 6: in-listener flush, teardown ------------------------------------------------

#[test]
fn a_write_inside_a_listener_updates_the_view_in_the_same_frame() {
    // The listener runs on the event path; its signal write flushes inline there (F2
    // unchanged), so the bound text is already updated by the time we render the next frame.
    let doc = Document::new().unwrap();
    let count = state(0i64);

    let view = boxed()
        .focusable(true)
        .on_key_press(move |_| count.update(|n| *n += 1))
        .child(dyn_text(move || count.get().to_string()));
    let mounted = mount(&doc, doc.root(), view).unwrap();
    let node = mounted.roots()[0];

    let mut rt = HeadlessRuntime::new(doc.clone(), 10, 1);
    rt.render().unwrap();
    assert_eq!(rt.get_cell(0, 0).unwrap().text, "0");

    doc.focus(node).unwrap();
    rt.simulate_key(KeyCode::Char('x')); // listener writes count, flush runs inline
    rt.render().unwrap();
    assert_eq!(rt.get_cell(0, 0).unwrap().text, "1");
}

#[test]
fn unmount_leaves_no_orphaned_nodes() {
    let doc = Document::new().unwrap();
    assert_eq!(doc.node_count(), 1); // just the root

    let show = state(true);
    let mounted = mount(
        &doc,
        doc.root(),
        boxed().child(text("hi")).child(dynamic(move || {
            if show.get() {
                text("Y")
            } else {
                fragment(Vec::<View>::new())
            }
        })),
    )
    .unwrap();
    assert!(doc.node_count() > 1);

    // Tears down the box, the text, the dynamic block's marker, and its mounted content.
    mounted.unmount();
    assert_eq!(doc.node_count(), 1);
}

// ---- F4 step 1: the `view!` macro toolchain ---------------------------------------------

#[test]
fn view_macro_mounts_a_text_node() {
    // Proves the whole chain: parse → emit `::tuidom_ui::text(..)` → resolve in-crate via
    // `extern crate self` → `View` → mount → paint.
    let doc = Document::new().unwrap();
    let _mounted = mount(&doc, doc.root(), crate::view! { text("hi") }).unwrap();

    let mut rt = HeadlessRuntime::new(doc, 10, 1);
    rt.render().unwrap();
    assert_eq!(rt.get_cell(0, 0).unwrap().text, "h");
}

#[test]
fn view_macro_mounts_an_empty_box() {
    // `box` is a reserved keyword; this exercises the `parse_any` path that accepts it.
    let doc = Document::new().unwrap();
    let _mounted = mount(&doc, doc.root(), crate::view! { box {} }).unwrap();

    assert_eq!(doc.get_children(doc.root()).len(), 1);
}

// ---- F4 step 2: element skeleton and static children ------------------------------------

#[test]
fn view_macro_mounts_nested_children() {
    // box > { text("x"), box > text("y") } — the same shape as the hand-built
    // `nested_elements_mount_under_their_parent`, now written as a template.
    let doc = Document::new().unwrap();
    let view = crate::view! {
        box {
            text("x"),
            box { text("y") },
        }
    };
    let _mounted = mount(&doc, doc.root(), view).unwrap();

    let outer = doc.get_children(doc.root());
    assert_eq!(outer.len(), 1);
    assert_eq!(doc.get_children(outer[0]).len(), 2);

    let mut rt = HeadlessRuntime::new(doc, 10, 1);
    rt.render().unwrap();
    assert_eq!(row0(&rt, 2), "xy");
}

#[test]
fn view_macro_accepts_bare_string_literal_children() {
    // A bare `"literal"` child lowers to a static text node, no `text(..)` spelled out.
    let doc = Document::new().unwrap();
    let _mounted = mount(&doc, doc.root(), crate::view! { box { "hi" } }).unwrap();

    let mut rt = HeadlessRuntime::new(doc, 10, 1);
    rt.render().unwrap();
    assert_eq!(row0(&rt, 2), "hi");
}

#[test]
fn view_macro_mounts_an_input_element() {
    // `input {}` maps to the input builder, which creates an editable node — `input_value`
    // succeeds only for one, so it doubles as the assertion that the node is an input.
    let doc = Document::new().unwrap();
    let mounted = mount(&doc, doc.root(), crate::view! { input {} }).unwrap();
    let node = mounted.roots()[0];

    assert_eq!(doc.input_value(node).unwrap(), "");
    assert!(doc.is_focusable(node).unwrap()); // inputs are focusable by default
}

// ---- F4 step 3: attributes, events, and reactive text holes -----------------------------

#[test]
fn view_macro_literal_attr_is_static() {
    // A literal attribute value applies once, no effect — `focusable: true`.
    let doc = Document::new().unwrap();
    let mounted = mount(&doc, doc.root(), crate::view! { box { focusable: true } }).unwrap();
    let node = mounted.roots()[0];

    assert!(doc.is_focusable(node).unwrap());
}

#[test]
fn view_macro_text_hole_tracks_its_signal() {
    // `text(expr)` with a non-string, non-literal argument is a reactive hole: it stringifies
    // through `Display` and repaints on change, unlike the static `text("literal")` form.
    let doc = Document::new().unwrap();
    let n = state(0i64);
    let _mounted = mount(&doc, doc.root(), crate::view! { text(n.get()) }).unwrap();

    let mut rt = HeadlessRuntime::new(doc, 10, 1);
    rt.render().unwrap();
    assert_eq!(rt.get_cell(0, 0).unwrap().text, "0");

    n.set(5);
    rt.render().unwrap();
    assert_eq!(rt.get_cell(0, 0).unwrap().text, "5");
}

#[test]
fn view_macro_text_hole_does_not_reapply_a_sibling_style_hole() {
    // The load-bearing granularity test, written through the macro: a reactive style attribute
    // and a reactive-text sibling are separate effects. Changing the text signal must repaint
    // the text without re-evaluating the style — emergent coupling here is what silently resets
    // scroll, focus, and running transitions.
    let doc = Document::new().unwrap();
    let label = state(String::from("a"));
    let bg = state(0i64);
    let style_runs = Arc::new(AtomicUsize::new(0));

    let sr = style_runs.clone();
    let view = crate::view! {
        box {
            style: {
                sr.fetch_add(1, Ordering::SeqCst);
                let _ = bg.get(); // the style hole's only dependency
                Style::new()
            },
            text(label.get()),
        }
    };
    let _mounted = mount(&doc, doc.root(), view).unwrap();

    let mut rt = HeadlessRuntime::new(doc, 10, 1);
    rt.render().unwrap();
    assert_eq!(rt.get_cell(0, 0).unwrap().text, "a");
    assert_eq!(style_runs.load(Ordering::SeqCst), 1); // ran once at mount

    label.set(String::from("b")); // text dep only
    rt.render().unwrap();
    assert_eq!(rt.get_cell(0, 0).unwrap().text, "b"); // text repainted
    assert_eq!(style_runs.load(Ordering::SeqCst), 1); // style untouched

    bg.set(1); // style dep
    assert_eq!(style_runs.load(Ordering::SeqCst), 2); // style reran
}

#[test]
fn view_macro_wraps_a_zero_arg_event_closure() {
    // `on_click: move || …` has no room for the event argument the engine listener requires;
    // the macro wraps it to `move |_| …`. The style attribute (a helper-fn value, hence a
    // reactive hole) sizes the box so the click can land.
    let doc = Document::new().unwrap();
    let clicks = Arc::new(AtomicUsize::new(0));

    let c = clicks.clone();
    let view = crate::view! {
        box {
            style: sized_box_style(),
            on_click: move || {
                c.fetch_add(1, Ordering::SeqCst);
            },
        }
    };
    let _mounted = mount(&doc, doc.root(), view).unwrap();

    let mut rt = HeadlessRuntime::new(doc, 10, 1);
    rt.render().unwrap();
    rt.simulate_click(0, 0);

    assert_eq!(clicks.load(Ordering::SeqCst), 1);
}

#[test]
fn view_macro_composes_an_inline_style_macro() {
    // The explicit step-3 deliverable: an engine `style!{ … }` drops straight into a `style`
    // attribute. It sizes the box, which a click landing on cell (0,0) confirms was applied.
    let doc = Document::new().unwrap();
    let clicks = Arc::new(AtomicUsize::new(0));

    let c = clicks.clone();
    let view = crate::view! {
        box {
            style: tuidom::style!{ width: 4, height: 1 },
            on_click: move |_| { c.fetch_add(1, Ordering::SeqCst); },
        }
    };
    let _mounted = mount(&doc, doc.root(), view).unwrap();

    let mut rt = HeadlessRuntime::new(doc, 10, 1);
    rt.render().unwrap();
    rt.simulate_click(0, 0);

    assert_eq!(clicks.load(Ordering::SeqCst), 1);
}

#[test]
fn view_macro_key_handler_with_event_arg_drives_a_signal() {
    // A one-argument handler (`move |_ev| …`) passes through untouched, and its in-listener
    // write flushes inline so the bound text is current on the next frame.
    let doc = Document::new().unwrap();
    let count = state(0i64);

    let view = crate::view! {
        box {
            focusable: true,
            on_key_press: move |_ev| count.update(|n| *n += 1),
            text(count.get()),
        }
    };
    let mounted = mount(&doc, doc.root(), view).unwrap();
    let node = mounted.roots()[0];

    let mut rt = HeadlessRuntime::new(doc.clone(), 10, 1);
    rt.render().unwrap();
    assert_eq!(rt.get_cell(0, 0).unwrap().text, "0");

    doc.focus(node).unwrap();
    rt.simulate_key(KeyCode::Char('x'));
    rt.render().unwrap();
    assert_eq!(rt.get_cell(0, 0).unwrap().text, "1");
}

// ---- F4 step 4: reactive control flow ---------------------------------------------------

#[test]
fn view_macro_if_else_swaps_on_signal() {
    let doc = Document::new().unwrap();
    let show = state(true);
    let view = crate::view! {
        if show.get() { text("Y") } else { text("N") }
    };
    let _mounted = mount(&doc, doc.root(), view).unwrap();

    let mut rt = HeadlessRuntime::new(doc, 10, 1);
    rt.render().unwrap();
    assert_eq!(rt.get_cell(0, 0).unwrap().text, "Y");

    show.set(false);
    rt.render().unwrap();
    assert_eq!(rt.get_cell(0, 0).unwrap().text, "N");
}

#[test]
fn view_macro_else_if_chain_selects_the_right_branch() {
    let doc = Document::new().unwrap();
    let n = state(0i64);
    let view = crate::view! {
        if n.get() == 0 { text("z") } else if n.get() == 1 { text("o") } else { text("m") }
    };
    let _mounted = mount(&doc, doc.root(), view).unwrap();

    let mut rt = HeadlessRuntime::new(doc, 10, 1);
    rt.render().unwrap();
    assert_eq!(rt.get_cell(0, 0).unwrap().text, "z");

    n.set(1);
    rt.render().unwrap();
    assert_eq!(rt.get_cell(0, 0).unwrap().text, "o");

    n.set(9);
    rt.render().unwrap();
    assert_eq!(rt.get_cell(0, 0).unwrap().text, "m");
}

#[test]
fn view_macro_if_without_else_renders_nothing_when_false() {
    // A missing `else` lowers to an empty fragment, so a false condition mounts no content —
    // only the block's out-of-flow marker, which paints nothing.
    let doc = Document::new().unwrap();
    let show = state(false);
    let view = crate::view! {
        if show.get() { text("B") }
    };
    let _mounted = mount(&doc, doc.root(), view).unwrap();

    let mut rt = HeadlessRuntime::new(doc, 10, 1);
    rt.render().unwrap();
    assert_ne!(rt.get_cell(0, 0).unwrap().text, "B"); // nothing painted

    show.set(true);
    rt.render().unwrap();
    assert_eq!(rt.get_cell(0, 0).unwrap().text, "B");
}

#[test]
fn view_macro_match_selects_arm_reactively() {
    let doc = Document::new().unwrap();
    let sel = state(0i64);
    let view = crate::view! {
        match sel.get() {
            0 => text("zero"),
            1 => text("one"),
            _ => text("many"),
        }
    };
    let _mounted = mount(&doc, doc.root(), view).unwrap();

    let mut rt = HeadlessRuntime::new(doc, 10, 1);
    rt.render().unwrap();
    assert_eq!(row0(&rt, 4), "zero");

    sel.set(1);
    rt.render().unwrap();
    assert_eq!(row0(&rt, 3), "one");

    sel.set(7);
    rt.render().unwrap();
    assert_eq!(row0(&rt, 4), "many");
}

#[test]
fn view_macro_view_hole_mounts_an_expression() {
    // `{ expr }` is a view hole: the expression is coerced through `View::from` — here a
    // `String`, via the F3-step-2 `From<String>` impl — and re-mounts when a read signal changes.
    let doc = Document::new().unwrap();
    let msg = state(String::from("hi"));
    let view = crate::view! {
        { msg.get() }
    };
    let _mounted = mount(&doc, doc.root(), view).unwrap();

    let mut rt = HeadlessRuntime::new(doc, 10, 1);
    rt.render().unwrap();
    assert_eq!(row0(&rt, 2), "hi");

    msg.set(String::from("yo"));
    rt.render().unwrap();
    assert_eq!(row0(&rt, 2), "yo");
}

#[test]
fn view_macro_control_flow_is_granular() {
    // The load-bearing granularity test: an outer `if` and an inner reactive text are separate
    // effects. Changing the inner signal repaints the text without re-evaluating the outer
    // condition; only changing the condition's own signal re-runs the outer block.
    let doc = Document::new().unwrap();
    let cond = state(true);
    let inner = state(0i64);
    let outer_runs = Arc::new(AtomicUsize::new(0));

    let or = outer_runs.clone();
    let view = crate::view! {
        if tick_and(&or, cond) {
            text(inner.get())
        } else {
            text("off")
        }
    };
    let _mounted = mount(&doc, doc.root(), view).unwrap();

    let mut rt = HeadlessRuntime::new(doc, 10, 1);
    rt.render().unwrap();
    assert_eq!(rt.get_cell(0, 0).unwrap().text, "0");
    assert_eq!(outer_runs.load(Ordering::SeqCst), 1); // outer ran once at mount

    inner.set(5); // inner text dep only
    rt.render().unwrap();
    assert_eq!(rt.get_cell(0, 0).unwrap().text, "5"); // inner repainted
    assert_eq!(outer_runs.load(Ordering::SeqCst), 1); // outer condition not re-evaluated

    cond.set(false); // the outer block's own dependency
    rt.render().unwrap();
    assert_eq!(rt.get_cell(0, 0).unwrap().text, "o"); // "off"
    assert_eq!(outer_runs.load(Ordering::SeqCst), 2); // outer re-ran
}

/// Count each evaluation of an `if` condition, then return the flag's value — so a test can
/// assert how often the enclosing block re-ran. Reads `flag` inside, so the block subscribes
/// to it and nothing else.
fn tick_and(counter: &AtomicUsize, flag: crate::Signal<bool>) -> bool {
    counter.fetch_add(1, Ordering::SeqCst);
    flag.get()
}

// ---- F4 step 6: the worked counter example ----------------------------------------------

#[test]
fn view_macro_counter_worked_example() {
    // The canonical counter, written entirely in `view!`: a focusable box, a key handler that
    // mutates a signal, and a reactive text hole that reflects it. This is the example the
    // macro docs show; it lives here as a test so the doc stays true.
    let doc = Document::new().unwrap();
    let count = state(0i64);

    let view = crate::view! {
        box {
            focusable: true,
            on_key_press: move |ev| match ev.code {
                KeyCode::Char('+') => count.update(|n| *n += 1),
                KeyCode::Char('-') => count.update(|n| *n -= 1),
                _ => {}
            },

            text("count: "),
            text(count.get()),
        }
    };
    let mounted = mount(&doc, doc.root(), view).unwrap();
    let node = mounted.roots()[0];

    let mut rt = HeadlessRuntime::new(doc.clone(), 20, 1);
    rt.render().unwrap();
    assert_eq!(row0(&rt, 8), "count: 0");

    doc.focus(node).unwrap();
    rt.simulate_key(KeyCode::Char('+'));
    rt.simulate_key(KeyCode::Char('+'));
    rt.render().unwrap();
    assert_eq!(row0(&rt, 8), "count: 2");

    rt.simulate_key(KeyCode::Char('-'));
    rt.render().unwrap();
    assert_eq!(row0(&rt, 8), "count: 1");
}

// ---- F4 step 5: `each` ------------------------------------------------------------------

#[test]
fn view_macro_unkeyed_each_reuses_nodes_by_index() {
    // Unkeyed `each` keys by position: appending keeps the existing item nodes and mounts only the
    // new tail. `_, item` drops the index; the list lays out adjacently (default row).
    let doc = Document::new().unwrap();
    let items = state(vec![String::from("a"), String::from("b")]);
    let view = crate::view! {
        each _, item in items.get() {
            text(item),
        }
    };
    let _mounted = mount(&doc, doc.root(), view).unwrap();

    let mut rt = HeadlessRuntime::new(doc.clone(), 10, 1);
    rt.render().unwrap();
    assert_eq!(row0(&rt, 2), "ab");
    let before = item_roots(&doc, doc.root());

    items.set(vec![
        String::from("a"),
        String::from("b"),
        String::from("c"),
    ]);
    rt.render().unwrap();
    assert_eq!(row0(&rt, 3), "abc");
    // a and b are the very same nodes; only c was mounted.
    let after = item_roots(&doc, doc.root());
    assert_eq!(after.len(), 3);
    assert_eq!(&after[..2], &before[..]);
}

#[test]
fn view_macro_unkeyed_each_keeps_stale_content_when_a_value_changes() {
    // The unkeyed hazard: with no key, a value that changes at a stable index reuses that
    // position's node — built once, never re-run — so it keeps its original glyph. The remedy is
    // a `key`, or an item that reads its value through a signal.
    let doc = Document::new().unwrap();
    let items = state(vec![String::from("a"), String::from("b")]);
    let view = crate::view! {
        each _, item in items.get() {
            text(item),
        }
    };
    let _mounted = mount(&doc, doc.root(), view).unwrap();

    let mut rt = HeadlessRuntime::new(doc, 10, 1);
    rt.render().unwrap();
    assert_eq!(row0(&rt, 2), "ab");

    // Same length, different values: indices 0 and 1 survive and keep "a"/"b".
    items.set(vec![String::from("x"), String::from("y")]);
    rt.render().unwrap();
    assert_eq!(row0(&rt, 2), "ab");
}

#[test]
fn view_macro_keyed_each_preserves_identity_across_a_reorder() {
    // `key = item.clone()` makes the list keyed: a reorder moves the existing nodes instead of
    // rebuilding, so each item keeps its identity — and, because the key is the value, its content
    // stays correct.
    let doc = Document::new().unwrap();
    let items = state(vec![
        String::from("a"),
        String::from("b"),
        String::from("c"),
    ]);
    let view = crate::view! {
        each _, item in items.get() key = item.clone() {
            text(item),
        }
    };
    let _mounted = mount(&doc, doc.root(), view).unwrap();

    let mut rt = HeadlessRuntime::new(doc.clone(), 10, 1);
    rt.render().unwrap();
    assert_eq!(row0(&rt, 3), "abc");
    let before = item_roots(&doc, doc.root());

    items.set(vec![
        String::from("c"),
        String::from("a"),
        String::from("b"),
    ]);
    rt.render().unwrap();
    assert_eq!(row0(&rt, 3), "cab");
    let after = item_roots(&doc, doc.root());
    assert_eq!(after, vec![before[2], before[0], before[1]]);
}

#[test]
fn view_macro_each_binds_the_index() {
    // `i` binds the enumerate index (a `usize`, stringified by the text hole).
    let doc = Document::new().unwrap();
    let items = state(vec![String::from("x"), String::from("y")]);
    let view = crate::view! {
        each i, item in items.get() key = item.clone() {
            text(i),
            text(item),
        }
    };
    let _mounted = mount(&doc, doc.root(), view).unwrap();

    let mut rt = HeadlessRuntime::new(doc, 10, 1);
    rt.render().unwrap();
    assert_eq!(row0(&rt, 4), "0x1y"); // (index, item) per row, laid out adjacently
}

#[test]
fn view_macro_each_item_keeps_its_own_reactive_holes() {
    // An item template is ordinary `view!`, so its holes bind per item. Here every row nests a
    // box with a reactive-text child; the list mounts one node per item under the block.
    let doc = Document::new().unwrap();
    let items = state(vec![
        String::from("a"),
        String::from("b"),
        String::from("c"),
    ]);
    let view = crate::view! {
        box {
            each _, item in items.get() {
                text(item),
            }
        }
    };
    let mounted = mount(&doc, doc.root(), view).unwrap();
    let container = mounted.roots()[0];

    let mut rt = HeadlessRuntime::new(doc.clone(), 10, 1);
    rt.render().unwrap();
    assert_eq!(row0(&rt, 3), "abc");

    // Three item text nodes plus the block's anchor marker, all under the container box.
    assert_eq!(doc.get_children(container).len(), 4);
}

// ---- F5 step 1: component skeleton and signal-backed props ------------------------------

/// A minimal component: one prop, read as a signal, painted through a text hole.
#[crate::component]
fn Labelled(n: i32) -> View {
    // `n` is a `Signal<i32>` here — the macro wrapped the declared `i32`.
    crate::view! { text(n.get()) }
}

/// A component whose body is a fragment, to prove instantiation adds no wrapper node.
#[crate::component]
fn TwoBits() -> View {
    fragment([text("x"), text("y")])
}

/// A component owning internal state driven by a key handler, to prove the body runs under a
/// live scope where effects and listeners work.
#[crate::component]
fn Counter(start: i32) -> View {
    let count = state(start.get());
    crate::view! {
        box {
            focusable: true,
            on_key_press: move |ev| {
                if ev.code == KeyCode::Char('+') {
                    count.update(|c| *c += 1);
                }
            },
            text(count.get()),
        }
    }
}

#[test]
fn component_mounts_and_reads_a_prop_as_a_signal() {
    let doc = Document::new().unwrap();
    let _mounted = mount(&doc, doc.root(), crate::view! { Labelled { n: 7 } }).unwrap();

    let mut rt = HeadlessRuntime::new(doc, 10, 1);
    rt.render().unwrap();
    assert_eq!(row0(&rt, 1), "7");
}

#[test]
fn component_adds_no_wrapper_node() {
    // The body's two texts mount directly under the root — the component boundary is a
    // fragment, not a phantom flex level.
    let doc = Document::new().unwrap();
    let _mounted = mount(&doc, doc.root(), crate::view! { TwoBits {} }).unwrap();

    assert_eq!(doc.get_children(doc.root()).len(), 2);

    let mut rt = HeadlessRuntime::new(doc, 10, 1);
    rt.render().unwrap();
    assert_eq!(row0(&rt, 2), "xy");
}

#[test]
fn component_unmount_disposes_its_child_scope() {
    // The component runs under a child of the mount scope, so disposing the mount cascades to
    // it: the subtree is removed and the body's signals freed.
    let doc = Document::new().unwrap();
    let mounted = mount(&doc, doc.root(), crate::view! { Labelled { n: 1 } }).unwrap();
    assert_eq!(doc.get_children(doc.root()).len(), 1);

    mounted.unmount();
    assert_eq!(doc.get_children(doc.root()).len(), 0);
}

#[test]
fn component_body_owns_working_reactive_state() {
    let doc = Document::new().unwrap();
    let mounted = mount(&doc, doc.root(), crate::view! { Counter { start: 0 } }).unwrap();
    let node = mounted.roots()[0];

    let mut rt = HeadlessRuntime::new(doc.clone(), 10, 1);
    rt.render().unwrap();
    assert_eq!(row0(&rt, 1), "0");

    doc.focus(node).unwrap();
    rt.simulate_key(KeyCode::Char('+'));
    rt.render().unwrap();
    assert_eq!(row0(&rt, 1), "1");
}

// ---- F5 step 2: reactive props ----------------------------------------------------------

#[test]
fn component_reactive_prop_tracks_the_parent_signal() {
    // A non-literal prop value (`source.get()`) is kept synced to the parent signal by an
    // effect, so changing the parent updates the child's bound text.
    let doc = Document::new().unwrap();
    let source = state(3i32);
    let _mounted = mount(
        &doc,
        doc.root(),
        crate::view! { Labelled { n: source.get() } },
    )
    .unwrap();

    let mut rt = HeadlessRuntime::new(doc, 10, 1);
    rt.render().unwrap();
    assert_eq!(row0(&rt, 1), "3");

    source.set(8);
    rt.render().unwrap();
    assert_eq!(row0(&rt, 1), "8");
}

#[test]
fn component_reactive_prop_does_not_rerun_the_body() {
    // The load-bearing property: a prop change flows through the prop signal to the body's
    // binding, without re-running the body. If the body re-ran, its text node would be a fresh
    // one; instead the *same* node is patched in place. Node identity is the proof.
    let doc = Document::new().unwrap();
    let source = state(1i32);
    let mounted = mount(
        &doc,
        doc.root(),
        crate::view! { Labelled { n: source.get() } },
    )
    .unwrap();
    let node = mounted.roots()[0];
    assert_eq!(doc.get_children(doc.root()), vec![node]);

    let mut rt = HeadlessRuntime::new(doc.clone(), 10, 1);
    rt.render().unwrap();
    assert_eq!(row0(&rt, 1), "1");

    source.set(9);
    rt.render().unwrap();

    // Same single node, its content updated — the body was not rebuilt.
    assert_eq!(doc.get_children(doc.root()), vec![node]);
    assert_eq!(row0(&rt, 1), "9");
}

// ---- F5 step 3: optional props & defaults -----------------------------------------------

/// All props optional: `#[prop(default)]` (→ `Default::default()`), `#[prop(default = expr)]`,
/// and a bare `Option<T>` (→ `None`). Zero required props exercises the builder's no-typestate
/// path. Rendered left-to-right (default flex row) so a single row reads all three.
#[crate::component]
fn Defaults(#[prop(default)] a: i32, #[prop(default = 9)] b: i32, note: Option<String>) -> View {
    crate::view! {
        box {
            text(a.get()),
            text(b.get()),
            text(match note.get() {
                Some(s) => s,
                None => String::from("-"),
            }),
        }
    }
}

/// One required prop and one optional — exercises the single-marker typestate and default fill.
#[crate::component]
fn Mixed(label: i32, #[prop(default = 7)] pad: i32) -> View {
    crate::view! { box { text(label.get()), text(pad.get()) } }
}

/// Two required props — the critical typestate path: the call site may set them in any order.
#[crate::component]
fn TwoReq(a: i32, b: i32) -> View {
    crate::view! { box { text(a.get()), text(b.get()) } }
}

#[test]
fn component_omitted_optionals_use_their_defaults() {
    let doc = Document::new().unwrap();
    let _mounted = mount(&doc, doc.root(), crate::view! { Defaults {} }).unwrap();

    let mut rt = HeadlessRuntime::new(doc, 10, 1);
    rt.render().unwrap();
    // a = i32::default() = 0, b = 9, note = None → "-".
    assert_eq!(row0(&rt, 3), "09-");
}

#[test]
fn component_supplied_optionals_override_defaults() {
    let doc = Document::new().unwrap();
    let view = crate::view! { Defaults { a: 1, b: 2, note: Some(String::from("X")) } };
    let _mounted = mount(&doc, doc.root(), view).unwrap();

    let mut rt = HeadlessRuntime::new(doc, 10, 1);
    rt.render().unwrap();
    assert_eq!(row0(&rt, 3), "12X");
}

#[test]
fn component_required_and_optional_mix() {
    let doc = Document::new().unwrap();
    // `pad` omitted → default 7; only the required `label` is supplied.
    let _mounted = mount(&doc, doc.root(), crate::view! { Mixed { label: 3 } }).unwrap();

    let mut rt = HeadlessRuntime::new(doc, 10, 1);
    rt.render().unwrap();
    assert_eq!(row0(&rt, 2), "37");
}

#[test]
fn component_required_props_can_be_supplied_in_any_order() {
    // The typestate builder accepts required props out of declaration order — each setter
    // transitions only its own marker.
    let doc = Document::new().unwrap();
    let _mounted = mount(&doc, doc.root(), crate::view! { TwoReq { b: 2, a: 1 } }).unwrap();

    let mut rt = HeadlessRuntime::new(doc, 10, 1);
    rt.render().unwrap();
    assert_eq!(row0(&rt, 2), "12");
}

// ---- F5 step 4: the `children` prop -----------------------------------------------------

/// A container component: builds its own shell once and mounts caller-supplied children into it.
/// `children.clone()` is the idiom — `Children` is `Arc`-backed and the hole closure must be `Fn`.
#[crate::component]
fn Panel(title: String, children: Children) -> View {
    crate::view! {
        box {
            text(title.get()),
            { children.clone() },
        }
    }
}

#[test]
fn component_children_render_inside_the_container() {
    let doc = Document::new().unwrap();
    let view = crate::view! {
        Panel {
            title: String::from("T"),
            text("a"),
            text("b"),
        }
    };
    let _mounted = mount(&doc, doc.root(), view).unwrap();

    let mut rt = HeadlessRuntime::new(doc, 10, 1);
    rt.render().unwrap();
    // Title, then the two children, laid out in a row: "Tab".
    assert_eq!(row0(&rt, 3), "Tab");
}

#[test]
fn component_children_swap_under_a_stable_panel_node() {
    // The panel's shell is built once by the component primitive, so its box keeps identity while
    // the children swap beneath it — the whole point of receiving children as a deferred thunk.
    let doc = Document::new().unwrap();
    let show = state(true);
    let view = crate::view! {
        Panel {
            title: String::from("T"),
            if show.get() { text("a") } else { text("b") },
        }
    };
    let mounted = mount(&doc, doc.root(), view).unwrap();
    let panel = mounted.roots()[0];

    let mut rt = HeadlessRuntime::new(doc.clone(), 10, 1);
    rt.render().unwrap();
    assert_eq!(row0(&rt, 2), "Ta");

    show.set(false);
    rt.render().unwrap();
    assert_eq!(row0(&rt, 2), "Tb");
    // Same panel box across the child swap.
    assert_eq!(doc.get_children(doc.root()), vec![panel]);
}

#[test]
fn component_with_a_children_prop_but_none_supplied_is_empty() {
    // No body nodes → the `children` setter is never called → the prop's empty default stands.
    let doc = Document::new().unwrap();
    let view = crate::view! { Panel { title: String::from("T") } };
    let _mounted = mount(&doc, doc.root(), view).unwrap();

    let mut rt = HeadlessRuntime::new(doc, 10, 1);
    rt.render().unwrap();
    assert_eq!(row0(&rt, 1), "T");
}

// ---- F5 step 5: callback props ----------------------------------------------------------

/// A child that talks back to its parent through a `Callback` prop rather than reaching into
/// parent state. `on_fire` is stored raw (not signal-wrapped) and invoked directly.
#[crate::component]
fn Emitter(on_fire: Callback<i32>) -> View {
    crate::view! {
        box {
            focusable: true,
            on_key_press: move |_| on_fire.call(7),
        }
    }
}

/// An optional callback: `#[prop(default)]` falls back to the no-op `Callback`. The handler still
/// bumps the component's own counter, so a test can prove it ran past the default call.
#[crate::component]
fn MaybeEmit(#[prop(default)] on_fire: Callback<i32>) -> View {
    let count = state(0i64);
    crate::view! {
        box {
            focusable: true,
            on_key_press: move |_| {
                count.update(|n| *n += 1);
                on_fire.call(count.get() as i32);
            },
            text(count.get()),
        }
    }
}

#[test]
fn component_callback_prop_talks_back_to_the_parent() {
    // `view!` lowers the parent's closure to a `Callback::new(..)`; the child calls it, and the
    // parent's closure writes a parent signal a sibling binding reflects — child-to-parent flow
    // with no shared state threaded through props.
    let doc = Document::new().unwrap();
    let received = state(0i64);

    let view = crate::view! {
        box {
            Emitter { on_fire: move |n| received.set(n as i64) },
            text(received.get()),
        }
    };
    let mounted = mount(&doc, doc.root(), view).unwrap();
    let outer = mounted.roots()[0];
    // The Emitter adds no wrapper node, so its focusable box is the outer box's first child.
    let emitter_box = doc.get_children(outer)[0];

    let mut rt = HeadlessRuntime::new(doc.clone(), 4, 1);
    rt.render().unwrap();
    assert_eq!(row0(&rt, 1), "0");

    doc.focus(emitter_box).unwrap();
    rt.simulate_key(KeyCode::Char('x'));
    rt.render().unwrap();
    assert_eq!(row0(&rt, 1), "7");
}

#[test]
fn component_omitted_callback_prop_defaults_to_a_noop() {
    // No `on_fire` supplied → the `#[prop(default)]` no-op `Callback` stands. The handler still
    // runs to completion (the internal counter advances), proving the default is callable rather
    // than a hole that panics.
    let doc = Document::new().unwrap();
    let mounted = mount(&doc, doc.root(), crate::view! { MaybeEmit {} }).unwrap();
    let node = mounted.roots()[0];

    let mut rt = HeadlessRuntime::new(doc.clone(), 4, 1);
    rt.render().unwrap();
    assert_eq!(row0(&rt, 1), "0");

    doc.focus(node).unwrap();
    rt.simulate_key(KeyCode::Char('x'));
    rt.render().unwrap();
    assert_eq!(row0(&rt, 1), "1");
}

// ---- F5 step 7: lifecycle (on_mount / on_cleanup) ---------------------------------------

/// Marks its body and its mount through a `mark` callback, then instantiates `Inner`. It provides
/// the callback as context — proving a context can carry behavior, not just data — so `Inner`
/// injects the same sink without a prop threaded through.
#[crate::component]
fn Outer(mark: Callback<&'static str>) -> View {
    crate::provide_context(mark.clone());
    mark.call("outer-body");
    let mounted = mark.clone();
    crate::on_mount(move || mounted.call("outer-mount"));
    crate::view! { Inner {} }
}

/// Injects the `mark` callback `Outer` provided and marks its own body and mount.
#[crate::component]
fn Inner() -> View {
    let mark: Callback<&'static str> = crate::use_context().unwrap();
    mark.call("inner-body");
    let mounted = mark.clone();
    crate::on_mount(move || mounted.call("inner-mount"));
    crate::view! { text("x") }
}

#[test]
fn on_mount_is_deferred_and_nests_inner_before_outer() {
    // Bodies run top-down as the tree is built; each `on_mount` waits until its own subtree is
    // placed, so mounts fire bottom-up — the inner component's before the outer's, and every
    // mount after every body.
    let doc = Document::new().unwrap();
    let sink: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));

    let s = sink.clone();
    let _mounted = mount(
        &doc,
        doc.root(),
        crate::view! { Outer { mark: move |m| s.lock().unwrap().push(m) } },
    )
    .unwrap();

    assert_eq!(
        *sink.lock().unwrap(),
        vec!["outer-body", "inner-body", "inner-mount", "outer-mount"],
    );
}

/// A component that pings a callback from `on_mount` and paints a reactive prop, so a test can
/// change the prop and confirm the body (and thus `on_mount`) does not re-run.
#[crate::component]
fn OnceMounter(label: i32, on_ping: Callback<()>) -> View {
    let ping = on_ping.clone();
    crate::on_mount(move || ping.call(()));
    crate::view! { text(label.get()) }
}

#[test]
fn on_mount_runs_exactly_once_across_prop_changes() {
    // A prop change flows through the kept-synced signal and repaints, but never re-runs the
    // body — so `on_mount` fires exactly once for the component's lifetime, not per render.
    let doc = Document::new().unwrap();
    let mounts = Arc::new(AtomicUsize::new(0));
    let label = state(1i32);

    let m = mounts.clone();
    let _mounted = mount(
        &doc,
        doc.root(),
        crate::view! {
            OnceMounter { label: label.get(), on_ping: move |_| { m.fetch_add(1, Ordering::SeqCst); } }
        },
    )
    .unwrap();

    let mut rt = HeadlessRuntime::new(doc, 4, 1);
    rt.render().unwrap();
    assert_eq!(row0(&rt, 1), "1");
    assert_eq!(mounts.load(Ordering::SeqCst), 1);

    label.set(2); // repaints through the prop signal, but the body stays put
    rt.render().unwrap();
    assert_eq!(row0(&rt, 1), "2");
    assert_eq!(mounts.load(Ordering::SeqCst), 1);
}

/// Registers an ambient `on_cleanup` (the free function, not the `Scope` method) that pings a
/// callback, to prove it fires when the component's scope disposes on unmount.
#[crate::component]
fn CleanupComp(on_gone: Callback<()>) -> View {
    let gone = on_gone.clone();
    crate::on_cleanup(move || gone.call(()));
    crate::view! { text("x") }
}

#[test]
fn on_cleanup_free_fn_fires_on_unmount() {
    let doc = Document::new().unwrap();
    let cleaned = Arc::new(AtomicUsize::new(0));

    let c = cleaned.clone();
    let mounted = mount(
        &doc,
        doc.root(),
        crate::view! { CleanupComp { on_gone: move |_| { c.fetch_add(1, Ordering::SeqCst); } } },
    )
    .unwrap();
    assert_eq!(cleaned.load(Ordering::SeqCst), 0);

    mounted.unmount();
    assert_eq!(cleaned.load(Ordering::SeqCst), 1);
}

// ---- F5 step 8: a worked composition ----------------------------------------------------

/// A titled section laid out as a column — a reactive header over body content handed in as
/// `children`, one child per row. The section keeps its identity while its children swap.
#[crate::component]
fn Section(title: String, children: Children) -> View {
    crate::view! {
        box {
            style: column(),
            text(title.get()),
            { children.clone() },
        }
    }
}

/// A counter with internal reactive state that also reports every new value up to its parent
/// through a callback prop — child-to-parent composition layered on local state.
#[crate::component]
fn ReportingCounter(start: i32, on_change: Callback<i32>) -> View {
    let count = state(start.get());
    crate::view! {
        box {
            focusable: true,
            on_key_press: move |ev| {
                if ev.code == KeyCode::Char('+') {
                    count.update(|c| *c += 1);
                    on_change.call(count.get());
                }
            },
            text(count.get()),
        }
    }
}

#[test]
fn worked_composition_of_section_counters_and_callbacks() {
    // The whole milestone in one tree: a `Section` with a reactive title wraps two
    // `ReportingCounter`s passed as `children`; each reports its new value up through a callback
    // into a shared `latest` signal a sibling text reflects. Exercises reactive props, a children
    // prop, callback props, and per-component internal state together.
    let doc = Document::new().unwrap();
    let title = state("Sum".to_string());
    let latest = state(0i64);

    let view = crate::view! {
        box {
            style: column(),
            Section {
                title: title.get(),
                ReportingCounter { start: 0, on_change: move |n| latest.set(n as i64) },
                ReportingCounter { start: 10, on_change: move |n| latest.set(n as i64) },
            },
            text(latest.get()),
        }
    };
    let mounted = mount(&doc, doc.root(), view).unwrap();
    let outer = mounted.roots()[0];

    let mut rt = HeadlessRuntime::new(doc.clone(), 6, 4);
    rt.render().unwrap();
    // Row 0 the section title, rows 1–2 the two counters' starts, row 3 the reported latest.
    assert_eq!(row0(&rt, 3), "Sum");
    assert_eq!(read_row(&rt, 1, 1), "0");
    assert_eq!(read_row(&rt, 2, 2), "10");
    assert_eq!(read_row(&rt, 3, 1), "0");

    // `Section` adds no wrapper node, so its column box is `outer`'s first child, holding the
    // title, both counters, then the children marker; the second counter's box is child index 2.
    let section = doc.get_children(outer)[0];
    let counter1 = doc.get_children(section)[2];
    doc.focus(counter1).unwrap();
    rt.simulate_key(KeyCode::Char('+'));
    rt.render().unwrap();

    assert_eq!(read_row(&rt, 2, 2), "11"); // the focused counter advanced 10 → 11
    assert_eq!(read_row(&rt, 3, 2), "11"); // and its callback pushed 11 up to `latest`

    // The reactive title prop still tracks its parent signal after all that.
    title.set("Total".to_string());
    rt.render().unwrap();
    assert_eq!(row0(&rt, 5), "Total");
}

// ---- Remounted content nests under its owner scope --------------------------------------

#[derive(Clone)]
struct Theme(&'static str);

/// Paints the nearest provided `Theme`, or "none" when context does not reach it.
#[crate::component]
fn ThemeReader() -> View {
    let theme = crate::use_context::<Theme>().map_or("none", |t| t.0);
    crate::view! { text(theme) }
}

#[crate::component]
fn ThemeProvider(children: Children) -> View {
    crate::provide_context(Theme("dark"));
    crate::view! { box { { children.clone() } } }
}

#[crate::component]
fn ThemedIf(show: bool) -> View {
    crate::provide_context(Theme("dark"));
    crate::view! {
        box {
            if show.get() { ThemeReader {} } else { text("hidden") }
        }
    }
}

#[crate::component]
fn ThemedList(keys: Vec<&'static str>) -> View {
    crate::provide_context(Theme("dark"));
    keyed(move || {
        keys.get()
            .into_iter()
            .map(|k| (k, move || crate::view! { ThemeReader {} }))
            .collect::<Vec<_>>()
    })
}

#[test]
fn context_reaches_children_content() {
    let doc = Document::new().unwrap();
    let _mounted = mount(
        &doc,
        doc.root(),
        crate::view! { ThemeProvider { ThemeReader {} } },
    )
    .unwrap();

    let mut rt = HeadlessRuntime::new(doc, 10, 1);
    rt.render().unwrap();
    assert_eq!(row0(&rt, 4), "dark");
}

#[test]
fn context_reaches_content_an_if_mounts_on_a_rerun() {
    // The reader is absent at first, so it mounts only when the block reruns — from inside a
    // flush, where the thread's scope stack is not the owner's.
    let doc = Document::new().unwrap();
    let show = state(false);
    let _mounted = mount(
        &doc,
        doc.root(),
        crate::view! { ThemedIf { show: show.get() } },
    )
    .unwrap();

    let mut rt = HeadlessRuntime::new(doc, 10, 1);
    rt.render().unwrap();
    assert_eq!(row0(&rt, 6), "hidden");

    show.set(true);
    rt.render().unwrap();
    assert_eq!(row0(&rt, 6), "dark  ");
}

#[test]
fn context_reaches_keyed_items_including_ones_added_later() {
    let doc = Document::new().unwrap();
    let keys = state(vec!["a"]);
    let _mounted = mount(
        &doc,
        doc.root(),
        crate::view! { ThemedList { keys: keys.get() } },
    )
    .unwrap();

    let mut rt = HeadlessRuntime::new(doc, 10, 1);
    rt.render().unwrap();
    assert_eq!(row0(&rt, 4), "dark");

    keys.set(vec!["a", "b"]);
    rt.render().unwrap();
    assert_eq!(row0(&rt, 8), "darkdark");
}

/// A `CleanupComp` that counts its cleanup into `count`.
fn counted_cleanup(count: Arc<AtomicUsize>) -> View {
    crate::view! {
        CleanupComp { on_gone: move |_| { count.fetch_add(1, Ordering::SeqCst); } }
    }
}

/// A keyed list of two `counted_cleanup` items.
fn counted_cleanup_list(count: Arc<AtomicUsize>) -> View {
    keyed(move || {
        ["a", "b"]
            .into_iter()
            .map(|k| {
                let count = count.clone();
                (k, move || counted_cleanup(count.clone()))
            })
            .collect::<Vec<_>>()
    })
}

#[test]
fn disposing_the_owner_disposes_nested_content_once() {
    // One cleanup inside a dynamic block, two inside a keyed list. Unmounting the owner reaches all three
    // through the scope tree, and each runs exactly once.
    let doc = Document::new().unwrap();
    let cleaned = Arc::new(AtomicUsize::new(0));

    let c = cleaned.clone();
    let view = boxed()
        .child(dynamic(move || counted_cleanup(c.clone())))
        .child(counted_cleanup_list(cleaned.clone()));
    let mounted = mount(&doc, doc.root(), view).unwrap();
    assert_eq!(cleaned.load(Ordering::SeqCst), 0);

    mounted.unmount();
    assert_eq!(cleaned.load(Ordering::SeqCst), 3);
    assert_eq!(doc.get_children(doc.root()).len(), 0);
}

#[test]
fn swapping_dynamic_content_does_not_accumulate_child_scopes() {
    let doc = Document::new().unwrap();
    let show = state(false);
    let mounted = mount(
        &doc,
        doc.root(),
        crate::view! { if show.get() { text("a") } else { text("b") } },
    )
    .unwrap();
    assert_eq!(mounted.scope.child_count(), 1);

    for _ in 0..50 {
        show.update(|s| *s = !*s);
    }
    assert_eq!(mounted.scope.child_count(), 1);
}

// ---- Keyed lists: identity-preserving reconciliation ------------------------------------

/// A keyed list of one-glyph text items, keyed by the glyph itself, driven by `order`.
fn text_list(order: Signal<Vec<&'static str>>) -> View {
    keyed(move || {
        order
            .get()
            .into_iter()
            .map(|k| (k, move || text(k)))
            .collect::<Vec<_>>()
    })
}

/// The keyed block's item roots in DOM order — everything under `parent` except the trailing
/// anchor marker, which a keyed list always keeps last.
fn item_roots(doc: &Document, parent: NodeId) -> Vec<NodeId> {
    let mut kids = doc.get_children(parent);
    kids.pop(); // drop the marker
    kids
}

#[test]
fn a_keyed_list_paints_its_items_in_order() {
    let doc = Document::new().unwrap();
    let order = state(vec!["a", "b", "c"]);
    let _m = mount(&doc, doc.root(), text_list(order)).unwrap();

    let mut rt = HeadlessRuntime::new(doc, 4, 1);
    rt.render().unwrap();
    assert_eq!(row0(&rt, 3), "abc");
}

#[test]
fn a_keyed_reorder_preserves_node_identity() {
    let doc = Document::new().unwrap();
    let order = state(vec!["a", "b", "c"]);
    let _m = mount(&doc, doc.root(), text_list(order)).unwrap();

    let before = item_roots(&doc, doc.root());
    assert_eq!(before.len(), 3);

    // Reorder to c, a, b. Every item is the *same node* it was — moved, never rebuilt.
    order.set(vec!["c", "a", "b"]);
    let after = item_roots(&doc, doc.root());
    assert_eq!(after, vec![before[2], before[0], before[1]]);
}

#[test]
fn a_keyed_insert_reuses_survivors_and_mounts_only_the_new_key() {
    let doc = Document::new().unwrap();
    let order = state(vec!["a", "b", "c"]);
    let _m = mount(&doc, doc.root(), text_list(order)).unwrap();

    let before = item_roots(&doc, doc.root());

    // Insert x at the front: a, b, c keep their identity and order; only x is freshly mounted.
    order.set(vec!["x", "a", "b", "c"]);
    let after = item_roots(&doc, doc.root());
    assert_eq!(after.len(), 4);
    assert_eq!(&after[1..], &before[..]);
    assert!(!before.contains(&after[0]));
}

#[test]
fn a_keyed_remove_disposes_only_the_vanished_key() {
    let doc = Document::new().unwrap();
    let order = state(vec!["a", "b", "c"]);
    let _m = mount(&doc, doc.root(), text_list(order)).unwrap();

    let before = item_roots(&doc, doc.root());

    // Drop b: a and c survive with their nodes; b's node is gone from the document.
    order.set(vec!["a", "c"]);
    let after = item_roots(&doc, doc.root());
    assert_eq!(after, vec![before[0], before[2]]);
    assert!(doc.get_node(before[1]).is_none());
}

#[test]
fn an_empty_keyed_list_holds_its_place_between_siblings() {
    let doc = Document::new().unwrap();
    let order = state(vec!["a", "b"]);
    // A sibling on each side: the block empties and refills without disturbing them, because
    // the out-of-flow marker keeps holding the slot.
    let view = fragment([text("<"), text_list(order), text(">")]);
    let _m = mount(&doc, doc.root(), view).unwrap();

    let mut rt = HeadlessRuntime::new(doc, 4, 1);
    rt.render().unwrap();
    assert_eq!(row0(&rt, 4), "<ab>");

    order.set(vec![]);
    rt.render().unwrap();
    assert_eq!(row0(&rt, 2), "<>");

    order.set(vec!["a", "b"]);
    rt.render().unwrap();
    assert_eq!(row0(&rt, 4), "<ab>");
}

#[test]
fn a_duplicate_key_is_dropped_not_duplicated() {
    let doc = Document::new().unwrap();
    let order = state(vec!["a", "a", "b"]);
    let _m = mount(&doc, doc.root(), text_list(order)).unwrap();

    // The second "a" is dropped: two items, not three.
    let roots = item_roots(&doc, doc.root());
    assert_eq!(roots.len(), 2);

    let mut rt = HeadlessRuntime::new(doc, 4, 1);
    rt.render().unwrap();
    assert_eq!(row0(&rt, 2), "ab");
}

/// A focusable box wrapping the key's glyph — an item that carries focus state.
fn focusable_list(order: Signal<Vec<&'static str>>) -> View {
    View::from(boxed().style(column()).child(keyed(move || {
        order
            .get()
            .into_iter()
            .map(|k| {
                (k, move || {
                    View::from(boxed().focusable(true).child(text(k)))
                })
            })
            .collect::<Vec<_>>()
    })))
}

#[test]
fn focus_stays_with_a_keyed_item_across_a_reorder() {
    let doc = Document::new().unwrap();
    let order = state(vec!["a", "b", "c"]);
    let _m = mount(&doc, doc.root(), focusable_list(order)).unwrap();

    let parent = doc.get_children(doc.root())[0];
    let roots = item_roots(&doc, parent);
    doc.focus(roots[1]).unwrap(); // focus b
    assert_eq!(doc.focused(), Some(roots[1]));

    // Reorder so b moves position: focus rides the node, not the slot.
    order.set(vec!["c", "b", "a"]);
    assert_eq!(doc.focused(), Some(roots[1]));
    let after = item_roots(&doc, parent);
    assert_eq!(after, vec![roots[2], roots[1], roots[0]]);
}

/// A one-cell box with a solid background — an item whose paint we can watch a transition move.
fn cell_box(bg: Color) -> Style {
    let mut style = Style::new();
    style.width(Length::Cells(1));
    style.height(Length::Cells(1));
    style.background(bg);
    style
}

#[test]
fn an_in_flight_transition_stays_with_a_keyed_item_across_a_reorder() {
    let doc = Document::new().unwrap();
    let order = state(vec!["a", "b"]);
    let view = boxed().style(column()).child(keyed(move || {
        order
            .get()
            .into_iter()
            .map(|k| {
                (k, move || {
                    View::from(boxed().style(cell_box(Color::white())).child(text(k)))
                })
            })
            .collect::<Vec<_>>()
    }));
    let _m = mount(&doc, doc.root(), view).unwrap();

    let mut rt = HeadlessRuntime::new(doc.clone(), 4, 2);
    rt.render().unwrap();

    let parent = doc.get_children(doc.root())[0];
    let roots = item_roots(&doc, parent); // [a, b]
    let b = roots[1];

    // Start a slow white → black background transition on b (row 1), then run it halfway.
    doc.set_transition(
        b,
        TransitionConfig::new(
            TransitionProperty::Background,
            Duration::from_millis(1000),
            Easing::Linear,
        ),
    )
    .unwrap();
    doc.update_style(b, |s| s.background(Color::black()))
        .unwrap();
    rt.advance_time(Duration::from_millis(500));
    rt.render().unwrap();
    let midway = rt.get_cell(0, 1).unwrap().bg; // b, mid-fade
    let white = rt.get_cell(0, 0).unwrap().bg; // a, never transitioned
    assert_ne!(midway, white); // the transition has visibly moved off white

    // Reorder b to the top. The transition keeps running on the same node — it does not reset.
    order.set(vec!["b", "a"]);
    let after = item_roots(&doc, parent);
    assert_eq!(after, vec![b, roots[0]]);

    rt.advance_time(Duration::from_millis(250));
    rt.render().unwrap();
    let later = rt.get_cell(0, 0).unwrap().bg; // b now on row 0
    let white_after = rt.get_cell(0, 1).unwrap().bg; // a now on row 1, still white
    assert_eq!(white_after, white); // a untouched by any of this
    assert_ne!(later, white); // b did not snap back to its base — the transition survived
    assert_ne!(later, midway); // and it kept progressing past the halfway color
}

/// A fixed-height scroll container with more content than fits — its offset is per-node state.
fn scroller() -> View {
    let mut style = column();
    style.overflow_y(Overflow::Scroll);
    style.height(Length::Cells(2));
    View::from(
        boxed()
            .style(style)
            .child(text("l0"))
            .child(text("l1"))
            .child(text("l2"))
            .child(text("l3")),
    )
}

#[test]
fn a_nested_scroll_offset_survives_a_keyed_reorder() {
    let doc = Document::new().unwrap();
    let order = state(vec!["a", "b", "c"]);
    let view = boxed().style(column()).child(keyed(move || {
        order
            .get()
            .into_iter()
            .map(|k| (k, scroller as fn() -> View))
            .collect::<Vec<_>>()
    }));
    let _m = mount(&doc, doc.root(), view).unwrap();

    let mut rt = HeadlessRuntime::new(doc.clone(), 10, 8);
    rt.render().unwrap(); // compute layout so the scroll range is known

    let parent = doc.get_children(doc.root())[0];
    let roots = item_roots(&doc, parent);
    doc.scroll_to(roots[1], 0, 2).unwrap(); // scroll b's inner container down
    assert_eq!(doc.scroll_offset(roots[1]).y, 2);

    // Reorder: b keeps its node, and thus its scroll offset.
    order.set(vec!["c", "b", "a"]);
    assert_eq!(doc.scroll_offset(roots[1]).y, 2);
    let after = item_roots(&doc, parent);
    assert_eq!(after, vec![roots[2], roots[1], roots[0]]);
}

// ---- The uniform windowed list ----------------------------------------------------------

/// The scroll container's inner content box — its single child, which holds the spacers and the
/// materialized item window.
fn list_content(doc: &Document, container: NodeId) -> NodeId {
    doc.get_children(container)[0]
}

/// A fixed 4×5 uniform list of `count` one-row text items, keyed by index. The width leaves the
/// scroll container's own vertical scrollbar (its last column) clear of the item text.
fn number_list(count: usize) -> View {
    let mut style = Style::new();
    style.width(Length::Cells(4));
    style.height(Length::Cells(5));
    virtual_list(style, 1, 0, move || count, |i| text(i.to_string()))
}

#[test]
fn an_unlaid_out_virtual_list_materializes_nothing() {
    let doc = Document::new().unwrap();
    let mounted = mount(&doc, doc.root(), number_list(100)).unwrap();
    let container = mounted.roots()[0];

    // Before the first layout the viewport is unknown, so the driver skips rather than windowing
    // to a zero viewport: the content box holds only its lead spacer, the keyed marker, and the
    // trail spacer — no items yet.
    assert_eq!(doc.get_children(list_content(&doc, container)).len(), 3);
}

#[test]
fn a_virtual_list_materializes_only_the_visible_window() {
    let doc = Document::new().unwrap();
    let mounted = mount(&doc, doc.root(), number_list(100)).unwrap();
    let container = mounted.roots()[0];

    let mut rt = HeadlessRuntime::new(doc.clone(), 4, 5);
    rt.render().unwrap(); // first frame learns the viewport and schedules the window
    rt.render().unwrap(); // second frame paints the materialized items

    // Viewport 5, stride 1, no overscan: exactly items 0..=4 exist and paint on their rows.
    assert_eq!(read_row(&rt, 0, 1), "0");
    assert_eq!(read_row(&rt, 4, 1), "4");
    // Five item nodes plus the lead spacer, keyed marker, and trail spacer — not 100 items.
    assert_eq!(doc.get_children(list_content(&doc, container)).len(), 8);
}

#[test]
fn spacers_keep_the_scroll_range_honest() {
    let doc = Document::new().unwrap();
    let _m = mount(&doc, doc.root(), number_list(100)).unwrap();

    let mut rt = HeadlessRuntime::new(doc.clone(), 4, 5);
    rt.render().unwrap();
    rt.render().unwrap();

    // Only five items are materialized, but the spacers stand in for the other 95, so the
    // container's scroll range reflects the full collection: 100 cells of content − 5 of viewport.
    let container = doc.get_children(doc.root())[0];
    let layout = doc.get_node(container).unwrap().layout.unwrap();
    assert_eq!(layout.max_scroll_y, 95);
}

#[test]
fn scrolling_a_virtual_list_shifts_the_materialized_window() {
    let doc = Document::new().unwrap();
    let _m = mount(&doc, doc.root(), number_list(100)).unwrap();

    let mut rt = HeadlessRuntime::new(doc.clone(), 4, 5);
    rt.render().unwrap();
    rt.render().unwrap();
    assert_eq!(read_row(&rt, 0, 1), "0"); // start of the list

    // Scroll ten rows down; the window slides to items 10..=14.
    let container = doc.get_children(doc.root())[0];
    doc.scroll_to(container, 0, 10).unwrap();
    rt.render().unwrap();
    rt.render().unwrap();
    assert_eq!(read_row(&rt, 0, 2), "10");
    assert_eq!(read_row(&rt, 4, 2), "14");

    // Still a bounded window — the far items never materialized.
    assert_eq!(doc.get_children(list_content(&doc, container)).len(), 8);
}

// ---- The measured windowed list ---------------------------------------------------------

/// An item that is really two cells tall (a fixed-height box), labelled by index.
fn tall_item(i: usize) -> View {
    let mut style = Style::new();
    style.height(Length::Cells(2));
    View::from(boxed().style(style).child(text(i.to_string())))
}

#[test]
fn a_measured_list_windows_by_real_extent_not_estimate() {
    let doc = Document::new().unwrap();
    let mut style = Style::new();
    style.width(Length::Cells(4));
    style.height(Length::Cells(6));
    // Estimate one cell, but every item is really two — the window must follow the measurements.
    let view = virtual_list_measured(style, 1, 0, || 20, tall_item);
    let _m = mount(&doc, doc.root(), view).unwrap();

    let container = doc.get_children(doc.root())[0];
    let mut rt = HeadlessRuntime::new(doc.clone(), 4, 6);
    for _ in 0..6 {
        rt.render().unwrap();
    }

    // A 6-cell viewport at a real height of 2 holds three items, not the six the estimate implied.
    assert_eq!(doc.get_children(list_content(&doc, container)).len(), 6); // lead+3 items+marker+trail
    assert_eq!(read_row(&rt, 0, 1), "0");
    assert_eq!(read_row(&rt, 2, 1), "1");
    assert_eq!(read_row(&rt, 4, 1), "2");
}

#[test]
fn anchoring_pins_the_view_when_an_item_above_the_fold_grows() {
    let doc = Document::new().unwrap();
    let top_height = state(2u16); // item 0's height; every other item is always two cells
    let mut style = Style::new();
    style.width(Length::Cells(4));
    style.height(Length::Cells(4)); // viewport 4
    let render = move |i: usize| -> View {
        if i == 0 {
            View::from(
                boxed()
                    .dyn_style(move || {
                        let mut s = Style::new();
                        s.height(Length::Cells(top_height.get()));
                        s
                    })
                    .child(text("A")),
            )
        } else {
            let mut s = Style::new();
            s.height(Length::Cells(2));
            View::from(boxed().style(s).child(text(i.to_string())))
        }
    };
    // Estimate 2 matches every item's initial height; overscan 2 keeps items 0 and 1 materialized
    // (and thus measured) while item 2 sits at the top.
    let view = virtual_list_measured(style, 2, 2, || 20, render);
    let _m = mount(&doc, doc.root(), view).unwrap();

    let container = doc.get_children(doc.root())[0];
    let mut rt = HeadlessRuntime::new(doc.clone(), 4, 4);
    for _ in 0..6 {
        rt.render().unwrap();
    }

    // Scroll so items 0 and 1 (two cells each) sit above the fold and item 2 is at the top.
    doc.scroll_to(container, 0, 4).unwrap();
    for _ in 0..6 {
        rt.render().unwrap();
    }
    assert_eq!(read_row(&rt, 0, 1), "2");
    assert_eq!(doc.scroll_offset(container).y, 4);

    // Grow item 0 (above the fold) by three cells. Anchoring absorbs the growth into the offset, so
    // item 2 stays put rather than sliding down out from under the eye.
    top_height.set(5);
    for _ in 0..6 {
        rt.render().unwrap();
    }
    assert_eq!(read_row(&rt, 0, 1), "2");
    assert_eq!(doc.scroll_offset(container).y, 7);
}

/// An item whose height depends on the container width: three 2-wide blocks that sit on one row
/// when wide (height 1) and wrap to two rows when narrow (height 2).
fn wrapping_item() -> View {
    let mut item = Style::new();
    item.flex_wrap(FlexWrap::Wrap);
    let block = || {
        let mut s = Style::new();
        s.width(Length::Cells(2));
        s.height(Length::Cells(1));
        s.flex_shrink(0.0);
        boxed().style(s)
    };
    View::from(
        boxed()
            .style(item)
            .child(block())
            .child(block())
            .child(block()),
    )
}

#[test]
fn a_cross_axis_resize_re_measures_off_window_items() {
    let doc = Document::new().unwrap();
    let mut style = Style::new();
    style.width(Length::Percent(100.0)); // follow the screen width
    style.height(Length::Cells(2)); // viewport 2 rows
    // Estimate 2 == the narrow height, so once the wide measurements are dropped every item reads
    // as two cells again.
    let view = virtual_list_measured(style, 2, 3, || 8, |_| wrapping_item());
    let _m = mount(&doc, doc.root(), view).unwrap();

    let container = doc.get_children(doc.root())[0];
    // Wide: three blocks fit on one row, so each item is one cell tall. Visit top and bottom so
    // items across the whole list get measured at that wide height.
    let mut rt = HeadlessRuntime::new(doc.clone(), 6, 2);
    for _ in 0..6 {
        rt.render().unwrap();
    }
    doc.scroll_to(container, 0, 1000).unwrap();
    for _ in 0..6 {
        rt.render().unwrap();
    }
    doc.scroll_to(container, 0, 0).unwrap();
    for _ in 0..6 {
        rt.render().unwrap();
    }

    // Narrow: the blocks wrap, so every item is two cells tall. The resize must invalidate the
    // wide-width measurements — including the off-window ones — or the scroll range would still
    // reflect the stale one-cell heights.
    rt.resize(4, 2);
    for _ in 0..8 {
        rt.render().unwrap();
    }
    let layout = doc.get_node(container).unwrap().layout.unwrap();
    assert_eq!(layout.max_scroll_y, 8 * 2 - 2); // every item two cells, minus the 2-row viewport
}

// ---- Worked examples: the whole milestone at once ---------------------------------------

/// An item carrying all three kinds of `NodeId`-keyed runtime state at once: it is focusable, it
/// holds a nested scroll container with its own offset, and its own background can run a
/// transition. Its root is three cells tall over a two-cell nested scroller, so the item's
/// bottom row is exposed background whose colour a transition can be watched through.
fn stateful_item() -> View {
    let mut root = column();
    root.width(Length::Cells(6));
    root.height(Length::Cells(3));
    root.background(Color::white());

    let mut inner = column();
    inner.overflow_y(Overflow::Scroll);
    inner.height(Length::Cells(2));

    View::from(
        boxed().focusable(true).style(root).child(
            boxed()
                .style(inner)
                .child(text("l0"))
                .child(text("l1"))
                .child(text("l2"))
                .child(text("l3")),
        ),
    )
}

fn stateful_list(order: Signal<Vec<&'static str>>) -> View {
    View::from(boxed().style(column()).child(keyed(move || {
        order
            .get()
            .into_iter()
            .map(|k| (k, stateful_item as fn() -> View))
            .collect::<Vec<_>>()
    })))
}

#[test]
fn a_keyed_reorder_carries_focus_scroll_and_a_transition_together() {
    let doc = Document::new().unwrap();
    let order = state(vec!["a", "b", "c"]);
    let _m = mount(&doc, doc.root(), stateful_list(order)).unwrap();

    let mut rt = HeadlessRuntime::new(doc.clone(), 6, 12);
    rt.render().unwrap(); // compute layout so scroll ranges are known

    let parent = doc.get_children(doc.root())[0];
    let roots = item_roots(&doc, parent); // [a, b, c], rows 0..3, 3..6, 6..9
    let b = roots[1];
    let b_scroller = doc.get_children(b)[0];

    // Load b with all three kinds of state: focus it, scroll its nested container, and start a
    // slow background transition on it — then run the transition halfway.
    doc.focus(b).unwrap();
    doc.scroll_to(b_scroller, 0, 2).unwrap();
    doc.set_transition(
        b,
        TransitionConfig::new(
            TransitionProperty::Background,
            Duration::from_millis(1000),
            Easing::Linear,
        ),
    )
    .unwrap();
    doc.update_style(b, |s| s.background(Color::black()))
        .unwrap();
    rt.advance_time(Duration::from_millis(500));
    rt.render().unwrap();

    assert_eq!(doc.focused(), Some(b));
    assert_eq!(doc.scroll_offset(b_scroller).y, 2);
    let midway = rt.get_cell(0, 5).unwrap().bg; // b's exposed bg row (index 1, row 3+2), mid-fade
    let white = rt.get_cell(0, 2).unwrap().bg; // a's exposed bg row (index 0), never transitioned
    assert_ne!(midway, white); // the transition has visibly moved off white

    // Reorder b to the front. Nothing about b was rebuilt: it is the same node, so focus, the
    // nested scroll offset, and the in-flight transition all ride along.
    order.set(vec!["b", "a", "c"]);
    assert_eq!(item_roots(&doc, parent), vec![roots[1], roots[0], roots[2]]);
    assert_eq!(doc.focused(), Some(b)); // focus stayed with the node, not the slot
    assert_eq!(doc.scroll_offset(b_scroller).y, 2); // nested offset survived

    rt.advance_time(Duration::from_millis(250));
    rt.render().unwrap();
    let later = rt.get_cell(0, 2).unwrap().bg; // b now at index 0, exposed bg on row 2
    let a_now = rt.get_cell(0, 5).unwrap().bg; // a now at index 1, exposed bg on row 5
    assert_eq!(a_now, white); // a was untouched by any of this
    assert_ne!(later, white); // b did not snap back to base — the transition survived the move
    assert_ne!(later, midway); // and it kept progressing past the halfway colour
}

#[test]
fn a_windowed_list_scrolls_through_a_large_collection() {
    let doc = Document::new().unwrap();
    // A thousand rows, five in view, one item of overscan — only a handful ever exist at once.
    let mut style = Style::new();
    style.width(Length::Cells(6));
    style.height(Length::Cells(5));
    let view = virtual_list(style, 1, 1, || 1000, |i| text(i.to_string()));
    let container = mount(&doc, doc.root(), view).unwrap().roots()[0];

    let mut rt = HeadlessRuntime::new(doc.clone(), 6, 5);
    rt.render().unwrap();
    rt.render().unwrap();
    assert_eq!(read_row(&rt, 0, 1), "0");

    // The spacers stand in for the 995 unmaterialized rows, so the scrollbar reflects all thousand.
    let full = doc.get_node(container).unwrap().layout.unwrap();
    assert_eq!(full.max_scroll_y, 1000 - 5);

    // Jump deep into the collection; the window follows and paints the rows at that offset.
    doc.scroll_to(container, 0, 500).unwrap();
    rt.render().unwrap();
    rt.render().unwrap();
    assert_eq!(read_row(&rt, 0, 3), "500");
    assert_eq!(read_row(&rt, 4, 3), "504");

    // Jump to the end; the last visible row is the last item and the window is still bounded.
    doc.scroll_to(container, 0, 1000).unwrap();
    rt.render().unwrap();
    rt.render().unwrap();
    assert_eq!(read_row(&rt, 4, 3), "999");
    let n = doc.get_children(list_content(&doc, container)).len();
    assert!(n <= 10, "window stayed bounded, got {n} children"); // never a thousand nodes
}

// ---- Element events ---------------------------------------------------------------------

#[test]
fn every_element_event_is_accepted_and_freed_on_unmount() {
    // Each handler holds a clone of `probe`, so the strong count shows how many the engine
    // still holds. This `view!` also pins the macro's event list to the builder's methods: a
    // name the macro accepts but `Element` lacks fails to compile here.
    let probe = Arc::new(());
    let [p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11] = std::array::from_fn(|_| probe.clone());
    let view = crate::view! {
        box {
            on_click: move || { let _ = &p0; },
            on_key_press: move || { let _ = &p1; },
            on_mouse_down: move || { let _ = &p2; },
            on_mouse_up: move || { let _ = &p3; },
            on_focus: move || { let _ = &p4; },
            on_blur: move || { let _ = &p5; },
            on_wheel: move || { let _ = &p6; },
            on_scroll: move || { let _ = &p7; },
            on_input: move || { let _ = &p8; },
            on_transition_end: move || { let _ = &p9; },
            on_animation_end: move || { let _ = &p10; },
            on_animation_iteration: move || { let _ = &p11; },
        }
    };
    let doc = Document::new().unwrap();
    let mounted = mount(&doc, doc.root(), view).unwrap();
    assert_eq!(Arc::strong_count(&probe), 13);

    mounted.unmount();
    assert_eq!(Arc::strong_count(&probe), 1);
}

#[test]
fn a_click_fires_mouse_down_then_up_then_click() {
    let log = state(Vec::<&'static str>::new());
    let mut h = Harness::new(4, 1, move || {
        crate::view! {
            box {
                style: sized_box_style(),
                on_mouse_down: move || log.update(|l| l.push("down")),
                on_mouse_up: move || log.update(|l| l.push("up")),
                on_click: move || log.update(|l| l.push("click")),
            }
        }
    });

    h.click(0, 0);
    assert_eq!(log.get(), ["down", "up", "click"]);
}

#[test]
fn focus_and_blur_fire_as_focus_moves_and_bubble() {
    let log = state(Vec::<&'static str>::new());
    let mut h = Harness::new(4, 2, move || {
        crate::view! {
            box {
                style: column(),
                on_focus: move || log.update(|l| l.push("parent focus")),
                box {
                    focusable: true,
                    on_focus: move || log.update(|l| l.push("a focus")),
                    on_blur: move || log.update(|l| l.push("a blur")),
                    text("a"),
                },
                box {
                    focusable: true,
                    on_focus: move || log.update(|l| l.push("b focus")),
                    text("b"),
                },
            }
        }
    });

    h.press(KeyCode::Tab);
    h.press(KeyCode::Tab);
    assert_eq!(
        log.get(),
        [
            "a focus",
            "parent focus",
            "a blur",
            "b focus",
            "parent focus"
        ]
    );
}

#[test]
fn a_wheel_fires_wheel_then_scroll_with_the_new_offset() {
    let wheels = state(0);
    let offset = state(None);
    let mut h = Harness::new(4, 2, move || {
        let mut style = column();
        style.width(Length::Cells(4));
        style.height(Length::Cells(2));
        style.overflow_y(Overflow::Scroll);
        crate::view! {
            box {
                style: style.clone(),
                on_wheel: move || wheels.update(|w| *w += 1),
                on_scroll: move |ev| offset.set(Some(ev.y)),
                text("0"), text("1"), text("2"), text("3"), text("4"),
            }
        }
    });

    h.scroll(0, 0, -1);
    assert_eq!(wheels.get(), 1);
    assert!(
        offset.get().is_some_and(|y| y > 0),
        "got {:?}",
        offset.get()
    );
}

#[test]
fn on_input_on_a_container_sees_typing_in_an_input_inside_it() {
    let values = state(Vec::<String>::new());
    let mut h = Harness::new(6, 1, move || {
        crate::view! {
            box {
                on_input: move |ev| values.update(|v| v.push(ev.value.clone())),
                input { style: sized_box_style() },
            }
        }
    });

    h.press(KeyCode::Tab);
    h.type_text("hi");
    assert_eq!(values.get(), ["h", "hi"]);
}

#[test]
fn transition_end_fires_once_the_transition_finishes() {
    let dark = state(false);
    let ended = state(0);
    let mut h = Harness::new(4, 1, move || {
        crate::view! {
            box {
                style: {
                    let mut style = sized_box_style();
                    style.background(if dark.get() { Color::black() } else { Color::white() });
                    style
                },
                transitions: background_over(100),
                on_transition_end: move || ended.update(|n| *n += 1),
            }
        }
    });

    dark.set(true);
    h.advance(Duration::from_millis(50));
    assert_eq!(ended.get(), 0);
    h.advance(Duration::from_millis(50));
    assert_eq!(ended.get(), 1);
}

#[test]
fn animation_iteration_and_end_fire_at_their_boundaries() {
    let (iterations, ended) = (state(0), state(0));
    let mut h = Harness::new(4, 1, move || {
        crate::view! {
            box {
                test_id: "pulse",
                style: sized_box_style(),
                on_animation_iteration: move || iterations.update(|n| *n += 1),
                on_animation_end: move || ended.update(|n| *n += 1),
            }
        }
    });
    let pulse = h.find_attr("test_id", "pulse").unwrap();
    let animation = KeyframeAnimation::from_to(
        Duration::from_millis(100),
        [AnimatableProperty::Background(Color::white())],
        [AnimatableProperty::Background(Color::black())],
    )
    .iterations(2);
    h.document().animate(pulse, animation).unwrap();

    h.advance(Duration::from_millis(100));
    assert_eq!((iterations.get(), ended.get()), (1, 0));
    h.advance(Duration::from_millis(100));
    // The final boundary is reported as the end, not as another iteration.
    assert_eq!((iterations.get(), ended.get()), (1, 1));
}

// ---- Disabled state and pseudo-state styles ---------------------------------------------

fn white_box() -> Style {
    let mut style = sized_box_style();
    style.background(Color::white());
    style
}

fn black_bg() -> Style {
    let mut style = Style::new();
    style.background(Color::black());
    style
}

fn white() -> Option<ScreenColor> {
    Some(ScreenColor::from_rgb(255, 255, 255))
}

fn black() -> Option<ScreenColor> {
    Some(ScreenColor::from_rgb(0, 0, 0))
}

#[test]
fn a_disabled_element_ignores_clicks_and_tab_skips_it() {
    let clicks = state(0);
    let mut h = Harness::new(4, 2, move || {
        crate::view! {
            box {
                style: column(),
                box {
                    style: sized_box_style(),
                    focusable: true,
                    disabled: true,
                    on_click: move || clicks.update(|c| *c += 1),
                    text("off"),
                },
                box { test_id: "on", focusable: true, text("on") },
            }
        }
    });

    h.click(0, 0);
    assert_eq!(clicks.get(), 0);
    h.press(KeyCode::Tab);
    assert_eq!(h.document().focused(), h.find_attr("test_id", "on"));
}

#[test]
fn a_reactive_disabled_toggles_both_ways() {
    let (off, clicks) = (state(true), state(0));
    let mut h = Harness::new(4, 1, move || {
        crate::view! {
            box {
                test_id: "button",
                style: sized_box_style(),
                focusable: true,
                disabled: off.get(),
                on_click: move || clicks.update(|c| *c += 1),
            }
        }
    });
    let button = h.find_attr("test_id", "button");

    h.click(0, 0);
    h.press(KeyCode::Tab);
    assert_eq!((clicks.get(), h.document().focused()), (0, None));

    off.set(false);
    h.click(0, 0);
    h.press(KeyCode::Tab);
    assert_eq!((clicks.get(), h.document().focused()), (1, button));

    // Disabling the focused element blurs it.
    off.set(true);
    h.click(0, 0);
    assert_eq!((clicks.get(), h.document().focused()), (1, None));
}

#[test]
fn disabling_a_container_disables_what_is_inside_it() {
    let clicks = state(0);
    let mut h = Harness::new(4, 1, move || {
        crate::view! {
            box {
                disabled: true,
                box { style: sized_box_style(), on_click: move || clicks.update(|c| *c += 1) },
            }
        }
    });

    h.click(0, 0);
    assert_eq!(clicks.get(), 0);
}

#[test]
fn a_focus_style_paints_only_while_focused() {
    let mut h = Harness::new(4, 2, || {
        crate::view! {
            box {
                style: column(),
                box { style: white_box(), focusable: true, focus_style: black_bg() },
                box { focusable: true, text("b") },
            }
        }
    });
    assert_eq!(h.cell(0, 0).unwrap().bg, white());

    h.press(KeyCode::Tab);
    assert_eq!(h.cell(0, 0).unwrap().bg, black());

    h.press(KeyCode::Tab);
    assert_eq!(h.cell(0, 0).unwrap().bg, white());
}

#[test]
fn a_disabled_style_paints_while_disabled() {
    let off = state(false);
    let mut h = Harness::new(4, 1, move || {
        crate::view! {
            box { style: white_box(), disabled: off.get(), disabled_style: black_bg() }
        }
    });
    assert_eq!(h.cell(0, 0).unwrap().bg, white());

    off.set(true);
    assert_eq!(h.cell(0, 0).unwrap().bg, black());
}

#[test]
fn an_active_style_paints_between_mouse_down_and_up() {
    let mut h = Harness::new(4, 1, || {
        crate::view! {
            box { style: white_box(), focusable: true, active_style: black_bg() }
        }
    });

    // `click` sends down and up together, so press through the raw runtime.
    h.runtime().simulate_mouse_down(0, 0, MouseButton::Left);
    assert_eq!(h.cell(0, 0).unwrap().bg, black());
    h.runtime().simulate_mouse_up(0, 0, MouseButton::Left);
    assert_eq!(h.cell(0, 0).unwrap().bg, white());
}

// ---- Input value, multiline, mask -------------------------------------------------------

fn field_style(height: u16) -> Style {
    let mut style = Style::new();
    style.width(Length::Cells(8));
    style.height(Length::Cells(height));
    style
}

/// The harness's input cursor, as a byte offset into its value.
fn field_cursor(h: &mut Harness) -> usize {
    let field = h.find_attr("test_id", "field").unwrap();
    h.document().input_cursor(field).unwrap()
}

#[test]
fn typing_updates_a_bound_signal() {
    let name = state(String::new());
    let mut h = Harness::new(8, 1, move || {
        crate::view! { input { test_id: "field", style: field_style(1), bind_value: name } }
    });

    h.press(KeyCode::Tab);
    h.type_text("hey");
    assert_eq!(name.get(), "hey");
    assert_eq!(h.row(0), "hey");
}

#[test]
fn an_outside_write_replaces_the_value_and_clamps_the_cursor() {
    let name = state("hi".to_string());
    let mut h = Harness::new(8, 1, move || {
        crate::view! { input { test_id: "field", style: field_style(1), bind_value: name } }
    });
    assert_eq!(h.row(0), "hi");

    h.press(KeyCode::Tab);
    h.press(KeyCode::End);
    assert_eq!(field_cursor(&mut h), 2);

    // A longer value keeps the cursor's offset; clearing moves it to the start.
    name.set("hello".to_string());
    assert_eq!((h.row(0), field_cursor(&mut h)), ("hello".to_string(), 2));
    name.set(String::new());
    assert_eq!((h.row(0), field_cursor(&mut h)), (String::new(), 0));
}

#[test]
fn typing_mid_string_keeps_the_cursor_in_place() {
    let name = state(String::new());
    let mut h = Harness::new(8, 1, move || {
        crate::view! { input { test_id: "field", style: field_style(1), bind_value: name } }
    });

    h.press(KeyCode::Tab);
    h.type_text("ac");
    h.press(KeyCode::Left);
    h.type_text("b");
    assert_eq!(name.get(), "abc");
    assert_eq!(field_cursor(&mut h), 2);
}

#[test]
fn an_edit_the_handler_does_not_record_stays_until_the_next_write() {
    let text = state(String::new());
    let mut h = Harness::new(8, 1, move || {
        crate::view! {
            input { style: field_style(1), value: text.get(), on_input: move || {} }
        }
    });

    h.press(KeyCode::Tab);
    h.type_text("z");
    assert_eq!((text.get(), h.row(0)), (String::new(), "z".to_string()));

    text.set("q".to_string());
    assert_eq!(h.row(0), "q");
}

#[test]
fn a_handler_can_rewrite_the_value() {
    let text = state(String::new());
    let mut h = Harness::new(8, 1, move || {
        crate::view! {
            input {
                test_id: "field",
                style: field_style(1),
                value: text.get(),
                on_input: move |ev| text.set(ev.value.to_uppercase()),
            }
        }
    });

    h.press(KeyCode::Tab);
    h.type_text("ab");
    assert_eq!((text.get(), h.row(0)), ("AB".to_string(), "AB".to_string()));
    assert_eq!(field_cursor(&mut h), 2);
}

#[test]
fn a_mask_paints_in_place_of_the_value() {
    let mut h = Harness::new(8, 1, || {
        crate::view! { input { style: field_style(1), value: "abc", mask: Some('*') } }
    });
    assert_eq!(h.row(0), "***");
}

#[test]
fn a_multiline_input_takes_enter_as_a_newline() {
    let text = state(String::new());
    let mut h = Harness::new(8, 2, move || {
        crate::view! { input { style: field_style(2), multiline: true, bind_value: text } }
    });

    h.press(KeyCode::Tab);
    h.type_text("a");
    h.press(KeyCode::Enter);
    h.type_text("b");
    assert_eq!(text.get(), "a\nb");
    assert_eq!(h.screen(), "a\nb");
}

#[test]
fn input_only_properties_on_a_box_are_ignored() {
    let doc = Document::new().unwrap();
    let view = boxed()
        .style(sized_box_style())
        .value("x")
        .multiline(true)
        .mask(Some('*'));
    let mounted = mount(&doc, doc.root(), view).unwrap();

    let mut rt = HeadlessRuntime::new(doc, 4, 1);
    rt.render().unwrap();
    assert_eq!(row0(&rt, 4).trim_end(), "");
    assert_eq!(mounted.roots().len(), 1);
}

// ---- Declared transitions ---------------------------------------------------------------

fn background_over(millis: u64) -> [TransitionConfig; 1] {
    [TransitionConfig::new(
        TransitionProperty::Background,
        Duration::from_millis(millis),
        Easing::Linear,
    )]
}

#[test]
fn a_declared_transition_does_not_animate_the_initial_style() {
    // Opacity, because it defaults to a concrete 1.0: a transition installed before the first
    // style would fade the box out from fully opaque. An unset background has nothing to
    // interpolate from and snaps either way, so it could not tell the two orders apart.
    let mut h = Harness::new(4, 1, || {
        crate::view! {
            box {
                style: { let mut s = white_box(); s.opacity(0.0); s },
                transitions: [TransitionConfig::new(
                    TransitionProperty::Opacity,
                    Duration::from_millis(1000),
                    Easing::Linear,
                )],
            }
        }
    });
    assert_eq!(h.cell(0, 0).unwrap().bg, None);
}

#[test]
fn a_declared_transition_animates_a_later_style_change() {
    let dark = state(false);
    let mut h = Harness::new(4, 1, move || {
        crate::view! {
            box {
                style: {
                    let mut style = sized_box_style();
                    style.background(if dark.get() { Color::black() } else { Color::white() });
                    style
                },
                transitions: background_over(1000),
            }
        }
    });

    dark.set(true);
    h.advance(Duration::from_millis(500));
    let midway = h.cell(0, 0).unwrap().bg;
    assert_ne!(midway, white());
    assert_ne!(midway, black());
    h.advance(Duration::from_millis(500));
    assert_eq!(h.cell(0, 0).unwrap().bg, black());
}

#[test]
fn a_declared_transition_animates_a_focus_style() {
    let mut h = Harness::new(4, 1, || {
        crate::view! {
            box {
                style: white_box(),
                focusable: true,
                focus_style: black_bg(),
                transitions: background_over(1000),
            }
        }
    });

    h.press(KeyCode::Tab);
    h.advance(Duration::from_millis(500));
    let midway = h.cell(0, 0).unwrap().bg;
    assert_ne!(midway, white());
    assert_ne!(midway, black());
    h.advance(Duration::from_millis(500));
    assert_eq!(h.cell(0, 0).unwrap().bg, black());
}

// ---- Node refs and the document handle ---------------------------------------------------

#[test]
fn a_ref_holds_its_node_only_while_mounted() {
    let doc = Document::new().unwrap();
    let r = node_ref();
    assert_eq!(r.node(), None);

    let mounted = mount(&doc, doc.root(), boxed().node_ref(r)).unwrap();
    assert_eq!(r.node(), Some(mounted.roots()[0]));
    assert!(r.document().is_some());

    mounted.unmount();
    assert_eq!(r.node(), None);
    assert!(r.document().is_none());
}

#[test]
fn swapping_branches_that_share_a_ref_leaves_it_on_the_live_node() {
    let (first, r) = (state(true), node_ref());
    let builds = Arc::new(AtomicUsize::new(0));
    let counted = builds.clone();
    let mut h = Harness::new(4, 1, move || {
        crate::view! {
            box {
                if { counted.fetch_add(1, Ordering::SeqCst); first.get() } {
                    box { ref: r, test_id: "a" }
                } else {
                    box { ref: r, test_id: "b" }
                }
            }
        }
    });
    let a = h.find_attr("test_id", "a");
    assert_eq!(r.node(), a);

    first.set(false);
    let b = h.find_attr("test_id", "b");
    assert_eq!(r.node(), b);
    first.set(true);
    let a = h.find_attr("test_id", "a");
    assert_eq!(r.node(), a);
    // Filling and emptying the ref must not rerun the block that mounts it.
    assert_eq!(builds.load(Ordering::SeqCst), 3);
}

#[test]
fn unmounting_an_element_leaves_a_ref_a_later_element_took_over() {
    let (shown, r) = (state(true), node_ref());
    let mut h = Harness::new(4, 2, move || {
        crate::view! {
            box {
                if shown.get() { box { ref: r, test_id: "first" } },
                box { ref: r, test_id: "last" },
            }
        }
    });
    let last = h.find_attr("test_id", "last");
    assert_eq!(r.node(), last);

    shown.set(false);
    h.screen();
    assert_eq!(r.node(), last);
}

#[test]
fn an_effect_reading_a_ref_reruns_when_it_fills_and_empties() {
    let (shown, r) = (state(false), node_ref());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    let mut h = Harness::new(4, 1, move || {
        crate::effect(move || log.lock().unwrap().push(r.node().is_some()));
        crate::view! { box { if shown.get() { box { ref: r } } } }
    });

    shown.set(true);
    h.screen();
    shown.set(false);
    h.screen();
    assert_eq!(*seen.lock().unwrap(), [false, true, false]);
}

/// A component exposing its inner node through a ref the caller passes in.
#[crate::component]
fn RefField(node_ref: crate::NodeRef) -> View {
    crate::view! { box { box { ref: node_ref, test_id: "inner" } } }
}

/// A component binding a ref the caller may omit, reporting whether it filled.
#[crate::component]
fn OptionalRef(#[prop(default)] node_ref: crate::NodeRef, filled: Callback<bool>) -> View {
    crate::on_mount(move || filled.call(node_ref.node().is_some()));
    crate::view! { box { ref: node_ref } }
}

#[test]
fn a_component_fills_a_ref_its_caller_passes_in() {
    let r = node_ref();
    let mut h = Harness::new(4, 1, move || crate::view! { RefField { node_ref: r } });
    let inner = h.find_attr("test_id", "inner");
    assert!(inner.is_some());
    assert_eq!(r.node(), inner);
}

#[test]
fn an_omitted_ref_prop_defaults_to_a_fresh_ref() {
    let filled = state(false);
    let _h = Harness::new(4, 1, move || {
        crate::view! { OptionalRef { filled: move |f| filled.set(f) } }
    });
    assert!(filled.get());
}

#[test]
fn a_callback_held_in_a_variable_passes_as_a_prop() {
    let received = state(0i64);
    let mut h = Harness::new(4, 1, move || {
        let on_fire = Callback::new(move |n: i32| received.set(n as i64));
        crate::view! { Emitter { on_fire: on_fire } }
    });
    h.press(KeyCode::Tab);
    h.press(KeyCode::Char('x'));
    assert_eq!(received.get(), 7);
}

/// A component that records the root of whatever document `use_document` finds for it.
fn document_probe(found: &Arc<Mutex<Vec<Option<NodeId>>>>) -> View {
    let found = found.clone();
    super::component(move || {
        let root = use_document().map(|doc| doc.root());
        found.lock().unwrap().push(root);
        fragment([])
    })
}

#[test]
fn use_document_reaches_bodies_under_mount_app_and_dynamic_content() {
    let found = Arc::new(Mutex::new(Vec::new()));
    let doc = Document::new().unwrap();
    let _plain = mount(&doc, doc.root(), document_probe(&found)).unwrap();

    let shown = state(false);
    let probes = found.clone();
    let mut h = Harness::new(4, 1, move || {
        let nested = probes.clone();
        fragment([
            document_probe(&probes),
            super::component(move || document_probe(&nested)),
            dynamic(move || {
                if shown.get() {
                    document_probe(&probes)
                } else {
                    fragment([])
                }
            }),
        ])
    });
    shown.set(true);
    h.screen();

    let app_root = Some(h.document().root());
    assert_eq!(
        *found.lock().unwrap(),
        [Some(doc.root()), app_root, app_root, app_root]
    );
}

#[test]
fn use_document_is_none_outside_any_mount() {
    assert!(use_document().is_none());
}

// ---- Node ref methods --------------------------------------------------------------------

/// A focusable box tagged `id`, bound to `r`.
fn focus_target(id: &'static str, r: crate::NodeRef) -> View {
    crate::view! { box { ref: r, test_id: id, focusable: true, text(id) } }
}

#[test]
fn focus_from_on_mount_focuses_the_bound_input() {
    let mut h = Harness::new(8, 1, || {
        let field = node_ref();
        crate::on_mount(move || field.focus());
        crate::view! { input { ref: field, test_id: "field" } }
    });
    let field = h.find_attr("test_id", "field");
    assert!(field.is_some());
    assert_eq!(h.document().focused(), field);
}

#[test]
fn blur_clears_focus_only_from_its_own_node() {
    let (a, b) = (node_ref(), node_ref());
    let mut h = Harness::new(4, 2, move || {
        crate::view! { box { style: column(), { focus_target("a", a) }, { focus_target("b", b) } } }
    });
    h.screen();

    a.focus();
    assert!(a.is_focused() && !b.is_focused());
    b.blur();
    assert!(a.is_focused());
    a.blur();
    assert_eq!(h.document().focused(), None);
}

#[test]
fn scroll_methods_move_a_container_and_read_its_offset() {
    let list = node_ref();
    let mut h = Harness::new(4, 2, move || {
        let mut style = column();
        style.width(Length::Cells(4));
        style.height(Length::Cells(2));
        style.overflow_y(Overflow::Scroll);
        crate::view! {
            box {
                ref: list,
                style: style.clone(),
                text("0"), text("1"), text("2"), text("3"), text("4"),
            }
        }
    });
    h.screen();

    list.scroll_to(0, 2);
    assert_eq!(list.scroll_offset().map(|o| o.y), Some(2));
    assert!(h.row(0).starts_with('2'), "{}", h.screen());
    list.scroll_by(0, -1);
    assert_eq!(list.scroll_offset().map(|o| o.y), Some(1));
    assert!(h.row(0).starts_with('1'), "{}", h.screen());
}

#[test]
fn animate_runs_a_keyframe_animation_on_the_node() {
    let (pulse, ended) = (node_ref(), state(0));
    let mut h = Harness::new(4, 1, move || {
        crate::view! {
            box {
                ref: pulse,
                style: sized_box_style(),
                on_animation_end: move || ended.update(|n| *n += 1),
            }
        }
    });
    h.screen();

    let animation = KeyframeAnimation::from_to(
        Duration::from_millis(100),
        [AnimatableProperty::Background(Color::white())],
        [AnimatableProperty::Background(Color::black())],
    );
    assert!(pulse.animate(animation).is_some());
    h.advance(Duration::from_millis(100));
    assert_eq!(ended.get(), 1);
}

#[test]
fn layout_is_empty_in_on_mount_and_set_after_a_frame() {
    let (sized, at_mount) = (node_ref(), state(Some(false)));
    let mut h = Harness::new(4, 2, move || {
        crate::on_mount(move || at_mount.set(Some(sized.layout().is_some())));
        crate::view! { box { ref: sized, style: sized_box_style() } }
    });
    h.screen();

    assert_eq!(at_mount.get(), Some(false));
    let rect = sized.layout().map(|layout| layout.rect);
    assert_eq!(rect.map(|r| (r.width, r.height)), Some((4, 1)));
}

#[test]
fn cursor_and_selection_round_trip_on_an_input_and_do_nothing_on_a_box() {
    let (field, plain) = (node_ref(), node_ref());
    let mut h = Harness::new(8, 2, move || {
        crate::view! {
            box {
                style: column(),
                input { ref: field, value: "hello" },
                box { ref: plain },
            }
        }
    });
    h.screen();

    field.set_cursor(2);
    assert_eq!(field.cursor(), Some(2));
    field.set_selection(1..3);
    assert_eq!(field.selection(), Some(1..3));
    field.clear_selection();
    assert_eq!(field.selection(), None);

    plain.set_cursor(1);
    plain.set_selection(0..1);
    plain.clear_selection();
    assert_eq!((plain.cursor(), plain.selection()), (None, None));
}

#[test]
fn every_method_is_inert_on_an_empty_ref() {
    let empty = node_ref();
    empty.focus();
    empty.blur();
    empty.scroll_to(0, 1);
    empty.scroll_by(0, 1);
    empty.set_cursor(0);
    empty.set_selection(0..1);
    empty.clear_selection();
    let animation = KeyframeAnimation::from_to(
        Duration::from_millis(100),
        [AnimatableProperty::Opacity(0.0)],
        [AnimatableProperty::Opacity(1.0)],
    );
    assert!(empty.animate(animation).is_none());
    assert!(!empty.is_focused());
    assert_eq!(empty.scroll_offset(), None);
    assert!(empty.layout().is_none());
    assert_eq!((empty.cursor(), empty.selection()), (None, None));
}

#[test]
fn a_command_inside_an_effect_does_not_subscribe_it() {
    let (shown, target) = (state(false), node_ref());
    let runs = Arc::new(AtomicUsize::new(0));
    let counted = runs.clone();
    let mut h = Harness::new(4, 1, move || {
        crate::effect(move || {
            counted.fetch_add(1, Ordering::SeqCst);
            target.focus();
        });
        crate::view! { box { if shown.get() { { focus_target("t", target) } } } }
    });

    shown.set(true);
    h.screen();
    shown.set(false);
    h.screen();
    assert_eq!(runs.load(Ordering::SeqCst), 1);
}

// ---- Focus traps -------------------------------------------------------------------------

/// A column that is a stacking context, so it can trap focus.
fn modal_style() -> Style {
    let mut style = column();
    style.stacking_context(true);
    style
}

/// A focusable box tagged and labelled `id`.
fn focusable(id: &'static str) -> View {
    crate::view! { box { test_id: id, focusable: true, text(id) } }
}

/// An "open" button above a modal holding "ok" and "cancel", shown while `shown` is true.
fn opener_and_modal(shown: Signal<bool>, opened: Signal<i32>) -> View {
    crate::view! {
        box {
            style: column(),
            box { test_id: "open", focusable: true, on_click: move || opened.update(|n| *n += 1), text("open") },
            if shown.get() {
                box { style: modal_style(), trap_focus: true, { focusable("ok") }, { focusable("cancel") } }
            },
        }
    }
}

#[test]
fn a_trap_moves_focus_inside_and_tab_stays_within_it() {
    let shown = state(false);
    let mut h = Harness::new(8, 3, move || opener_and_modal(shown, state(0)));
    h.press(KeyCode::Tab);

    shown.set(true);
    let (ok, cancel) = (
        h.find_attr("test_id", "ok"),
        h.find_attr("test_id", "cancel"),
    );
    assert_eq!(h.document().focused(), ok);
    h.press(KeyCode::Tab);
    assert_eq!(h.document().focused(), cancel);
    h.press(KeyCode::Tab);
    assert_eq!(h.document().focused(), cancel);
    h.press(KeyCode::BackTab);
    h.press(KeyCode::BackTab);
    assert_eq!(h.document().focused(), ok);
}

#[test]
fn nodes_outside_a_trap_are_inert() {
    let (shown, opened) = (state(true), state(0));
    let mut h = Harness::new(8, 3, move || opener_and_modal(shown, opened));

    let open = h.find_attr("test_id", "open").unwrap();
    assert!(h.document().is_inert(open).unwrap());
    h.click_text("open");
    assert_eq!(opened.get(), 0);
}

#[test]
fn unmounting_a_trap_gives_focus_back_to_the_opener() {
    let shown = state(false);
    let mut h = Harness::new(8, 3, move || opener_and_modal(shown, state(0)));
    h.press(KeyCode::Tab);
    let open = h.find_attr("test_id", "open");

    shown.set(true);
    shown.set(false);
    h.screen();
    assert_eq!(h.document().focused(), open);
    assert_eq!(h.document().focus_context_depth(), 1);
}

/// A field that focuses itself once mounted.
#[crate::component]
fn AutofocusField() -> View {
    let field = node_ref();
    crate::on_mount(move || field.focus());
    crate::view! { input { ref: field, test_id: "search" } }
}

#[test]
fn an_autofocus_inside_a_trap_keeps_focus_and_the_opener_is_still_restored() {
    let shown = state(false);
    let mut h = Harness::new(8, 3, move || {
        crate::view! {
            box {
                style: column(),
                { focusable("open") },
                if shown.get() {
                    box { style: modal_style(), trap_focus: true, { focusable("ok") }, AutofocusField {} }
                },
            }
        }
    });
    h.press(KeyCode::Tab);
    let open = h.find_attr("test_id", "open");

    shown.set(true);
    let search = h.find_attr("test_id", "search");
    assert!(search.is_some());
    assert_eq!(h.document().focused(), search);

    shown.set(false);
    h.screen();
    assert_eq!(h.document().focused(), open);
}

#[test]
fn nested_traps_close_innermost_first() {
    let (outer, inner) = (state(false), state(false));
    let mut h = Harness::new(8, 5, move || {
        crate::view! {
            box {
                style: column(),
                { focusable("open") },
                if outer.get() {
                    box {
                        style: modal_style(),
                        trap_focus: true,
                        { focusable("outer") },
                        if inner.get() {
                            box { style: modal_style(), trap_focus: true, { focusable("inner") } }
                        },
                    }
                },
            }
        }
    });
    h.press(KeyCode::Tab);
    let open = h.find_attr("test_id", "open");

    outer.set(true);
    let outer_item = h.find_attr("test_id", "outer");
    inner.set(true);
    let inner_item = h.find_attr("test_id", "inner");
    assert_eq!(h.document().focused(), inner_item);
    assert_eq!(h.document().focus_context_depth(), 3);

    inner.set(false);
    h.screen();
    assert_eq!(h.document().focused(), outer_item);

    // Closing the outer trap while the inner one is open closes both.
    inner.set(true);
    outer.set(false);
    h.screen();
    assert_eq!(h.document().focused(), open);
    assert_eq!(h.document().focus_context_depth(), 1);
}

#[test]
fn a_trap_on_an_element_that_is_not_a_stacking_context_is_not_opened() {
    let mut h = Harness::new(8, 2, || {
        crate::view! {
            box {
                style: column(),
                { focusable("outside") },
                box { style: column(), trap_focus: true, { focusable("inside") } },
            }
        }
    });

    assert_eq!(h.document().focus_context_depth(), 1);
    assert_eq!(h.document().focused(), None);
    let outside = h.find_attr("test_id", "outside");
    h.press(KeyCode::Tab);
    assert_eq!(h.document().focused(), outside);
}
