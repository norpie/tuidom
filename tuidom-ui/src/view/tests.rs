//! View-layer tests. Everything is asserted through the headless screen — painted cells,
//! not internals — because that is the only ground truth for "the signal reached the node".

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use tuidom::Document;
use tuidom::event::KeyCode;
use tuidom::headless::HeadlessRuntime;
use tuidom::style::{FlexDirection, FlexGap, Length, Style};

use super::{Children, View, boxed, dyn_text, dynamic, fragment, mount, text};
use crate::{Callback, state};

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
fn view_macro_each_renders_a_list_and_rebuilds() {
    // `_, item` drops the index. The list renders adjacently (default row layout); changing the
    // collection rebuilds it whole — the naive F4 semantics, keyed reorder is F6.
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

    items.set(vec![
        String::from("x"),
        String::from("y"),
        String::from("z"),
    ]);
    rt.render().unwrap();
    assert_eq!(row0(&rt, 3), "xyz");
}

#[test]
fn view_macro_each_binds_the_index_and_accepts_a_key() {
    // `i` binds the enumerate index (a `usize`, stringified by the text hole). `key = …` parses
    // and is accepted — a documented no-op until F6 — proving the grammar is stable now.
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

/// Marks its body and its mount through a `mark` callback, then instantiates `Inner` directly
/// (not through `children`, so the parent scope chain is preserved and context reaches it). It
/// provides the callback as context — proving a context can carry behavior, not just data — so
/// `Inner` injects the same sink without a prop threaded through.
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
