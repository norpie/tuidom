//! Tests for `#[component(reactive)]` bodies. Most of what they check is that a body compiles:
//! a name rewritten when it should not be, or left alone when it should be read, is a type
//! error. The screen then confirms the values read.

use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};

use tuidom::Document;
use tuidom::headless::HeadlessRuntime;

use super::{NodeRef, View, mount};
use crate::{Callback, Derived, Signal, derived, state};

fn row0(rt: &HeadlessRuntime) -> String {
    let row: String = (0..40)
        .filter_map(|x| rt.get_cell(x, 0).map(|c| c.text))
        .collect();
    row.trim_end().to_string()
}

fn same(handle: Signal<i32>) -> Signal<i32> {
    handle
}

#[derive(Clone, Default, PartialEq, Debug)]
struct Pair {
    n: i32,
}

#[crate::component(reactive)]
fn EveryName(n: i32) -> View {
    let base = state(10);
    let doubled = derived(move || n * 2);
    let tripled = crate::derived!(n.get() * 3);
    let annotated: Signal<i32> = same(crate::signal!(n));
    let total: Derived<(i32, i32, i32, i32, i32, Pair)> =
        derived(move || (n, base, doubled, tripled, annotated, Pair { n }));
    crate::view! { text(format!("{:?}", total.get())) }
}

#[test]
fn each_kind_of_signal_name_reads_when_its_closure_runs() {
    let doc = Document::new().unwrap();
    let source = state(1i32);
    let _mounted = mount(&doc, doc.root(), crate::view! { EveryName { n: source } }).unwrap();

    let mut rt = HeadlessRuntime::new(doc, 40, 1);
    rt.render().unwrap();
    assert_eq!(row0(&rt), "(1, 10, 2, 3, 1, Pair { n: 1 })");

    source.set(2);
    rt.render().unwrap();
    assert_eq!(row0(&rt), "(2, 10, 4, 6, 2, Pair { n: 2 })");
}

#[crate::component(reactive)]
fn Shadowed(n: i32) -> View {
    let add_one = |n: i32| n + 1;
    let by_closure = add_one(n);
    let by_match = match n {
        n if n > 0 => n,
        _ => 0,
    };
    let by_if_let = if let Some(n) = Some(n) { n * 2 } else { 0 };
    let by_chain = if let Some(m) = Some(n)
        && let Some(n) = Some(m + 1)
        && n > 0
    {
        n
    } else {
        0
    };
    let mut by_for = 0;
    for n in 0..n {
        by_for += n;
    }
    let mut by_while = 0;
    let mut next = Some(n);
    while let Some(n) = next {
        by_while += n;
        next = None;
    }
    fn helper(n: i32) -> i32 {
        n * 10
    }
    let by_fn = helper(n);
    let after_block = {
        let n = 5;
        n + 1
    } + n;
    let shown = (
        by_closure,
        by_match,
        by_if_let,
        by_chain,
        by_for,
        by_while,
        by_fn,
        after_block,
    );
    let n = n * 100;
    crate::view! { text(format!("{shown:?} {n}")) }
}

#[test]
fn a_binding_of_the_same_name_hides_a_signal_for_its_scope() {
    let doc = Document::new().unwrap();
    let _mounted = mount(&doc, doc.root(), crate::view! { Shadowed { n: 3 } }).unwrap();

    let mut rt = HeadlessRuntime::new(doc, 40, 1);
    rt.render().unwrap();
    assert_eq!(row0(&rt), "(4, 3, 6, 4, 3, 3, 30, 9) 300");
}

#[crate::component(reactive)]
fn FormA(n: i32) -> View {
    let mut base = state(0);
    base.set(n.get());
    base.update(|b| *b += 1);
    base += 1;
    let read = (
        base.get(),
        n.with(|v| *v),
        base.try_get(),
        base.try_with(|b| *b * 2),
    );
    crate::view! { text(format!("{read:?}")) }
}

#[test]
fn form_a_lines_are_left_alone() {
    let doc = Document::new().unwrap();
    let _mounted = mount(&doc, doc.root(), crate::view! { FormA { n: 5 } }).unwrap();

    let mut rt = HeadlessRuntime::new(doc, 40, 1);
    rt.render().unwrap();
    assert_eq!(row0(&rt), "(7, 5, Some(7), Some(14))");
}

#[crate::component(reactive)]
fn RawProps(n: i32, on_seen: Callback<i32>, #[prop(default)] node_ref: NodeRef) -> View {
    let fire = on_seen;
    let _kept: NodeRef = node_ref;
    fire.call(n);
    crate::view! { text("") }
}

#[test]
fn callback_and_node_ref_props_are_not_signal_names() {
    let seen = Arc::new(AtomicI32::new(0));
    let record = Arc::clone(&seen);
    let on_seen = Callback::new(move |n| record.store(n, Ordering::SeqCst));
    let doc = Document::new().unwrap();
    let _mounted = mount(
        &doc,
        doc.root(),
        crate::view! { RawProps { n: 4, on_seen: on_seen } },
    )
    .unwrap();

    assert_eq!(seen.load(Ordering::SeqCst), 4);
}
