//! View-layer tests. Everything is asserted through the headless screen — painted cells,
//! not internals — because that is the only ground truth for "the signal reached the node".

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use tuidom::Document;
use tuidom::event::KeyCode;
use tuidom::headless::HeadlessRuntime;
use tuidom::style::{FlexDirection, FlexGap, Length, Style};

use super::{Children, View, boxed, dyn_text, dynamic, fragment, mount, text};
use crate::state;

/// A column container with a one-row gap between its children.
fn gapped_column() -> Style {
    let mut style = Style::new();
    style.flex_direction(FlexDirection::Column);
    style.gap(FlexGap::new(1, 0));
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
    (0..n as i32)
        .filter_map(|x| rt.get_cell(x, 0).map(|c| c.text))
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
