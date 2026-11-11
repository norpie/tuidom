//! Tests for `#[component(reactive)]` bodies. Most of what they check is that a body compiles:
//! a name rewritten when it should not be, or left alone when it should be read, is a type
//! error. The screen then confirms the values read.

use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};

use tuidom::Document;
use tuidom::event::KeyCode;
use tuidom::headless::HeadlessRuntime;
use tuidom::style::{FlexDirection, Length, Style};

use super::{NodeRef, View, mount};
use crate::testing::Harness;
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
    let tripled = crate::derived!(n * 3);
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
    let base = state(0);
    base.set(n.get());
    base.update(|b| *b += 1);
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
    assert_eq!(row0(&rt), "(6, 5, Some(6), Some(12))");
}

#[derive(Clone, Default, PartialEq, Debug)]
struct User {
    name: String,
    age: u32,
}

#[crate::component(reactive)]
fn Chains(items: Vec<i32>, index: usize, user: User) -> View {
    let len = derived(move || items.len());
    let picked = derived(move || items.get(index).copied().unwrap_or(-1));
    let doubled_sum = derived(move || items.iter().map(|i| i * 2).sum::<i32>());
    let first = derived(move || items[0]);
    let name = derived(move || user.name.clone());
    let age = derived(move || user.age);
    let summary = derived(move || (len, picked, doubled_sum, first, name, age));
    crate::view! { text(format!("{:?}", summary.get())) }
}

#[test]
fn a_method_chain_reads_through_with_and_tracks_its_arguments() {
    let doc = Document::new().unwrap();
    let items = state(vec![1, 2, 3]);
    let index = state(1usize);
    let _mounted = mount(
        &doc,
        doc.root(),
        crate::view! {
            Chains {
                items: items,
                index: index,
                user: User { name: "ann".to_string(), age: 30 },
            }
        },
    )
    .unwrap();

    let mut rt = HeadlessRuntime::new(doc, 40, 1);
    rt.render().unwrap();
    assert_eq!(row0(&rt), "(3, 2, 12, 1, \"ann\", 30)");

    index.set(2);
    rt.render().unwrap();
    assert_eq!(row0(&rt), "(3, 3, 12, 1, \"ann\", 30)");

    items.set(vec![5]);
    rt.render().unwrap();
    assert_eq!(row0(&rt), "(1, -1, 10, 5, \"ann\", 30)");
}

#[derive(Clone, Default, PartialEq, Debug)]
struct Point {
    x: i32,
    y: i32,
}

#[crate::component(reactive)]
fn Writes(start: i32) -> View {
    let count = state(0);
    let point = state(Point::default());
    let list = state(vec![0, 0]);
    let bump = move || {
        count = start * 10;
        count += start;
        point.x = count;
        point.y += 2;
        list[1] = count + 1;
        list[0] += 3;
    };
    crate::view! {
        box {
            focusable: true,
            on_key_press: move |_| bump(),
            text(format!("{} {:?} {:?}", count.get(), point.get(), list.get())),
        }
    }
}

#[test]
fn writes_to_a_signal_and_its_fields_reach_the_screen() {
    let doc = Document::new().unwrap();
    let mounted = mount(&doc, doc.root(), crate::view! { Writes { start: 4 } }).unwrap();

    let mut rt = HeadlessRuntime::new(doc.clone(), 40, 1);
    rt.render().unwrap();
    assert_eq!(row0(&rt), "0 Point { x: 0, y: 0 } [0, 0]");

    doc.focus(mounted.roots()[0]).unwrap();
    rt.simulate_key(KeyCode::Char('x'));
    rt.render().unwrap();
    assert_eq!(row0(&rt), "44 Point { x: 44, y: 2 } [3, 45]");
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

fn column() -> Style {
    let mut style = Style::new();
    style.flex_direction(FlexDirection::Column);
    style
}

#[crate::component]
fn Stepper(value: Signal<i32>) -> View {
    crate::view! {
        box {
            on_click: move || value.update(|v| *v += 10),
            text(format!("step {}", value.get())),
        }
    }
}

#[crate::component(reactive)]
fn ReactiveBoard(start: i32) -> View {
    let count = state(start);
    let items = state(vec![10, 20]);
    let doubled = crate::derived!(count * 2);
    crate::view! {
        box {
            style: column(),
            focusable: true,
            on_key_press: move |ev| {
                if ev.code == KeyCode::Char('+') {
                    count += 1;
                    let next = count;
                    items.update(|i| i.push(next));
                }
            },
            text(count),
            text(doubled),
            if count > 1 { "big" } else { "small" },
            match count % 2 {
                0 => "even",
                _ => "odd",
            },
            each _, item in items key = item { text(item) },
            Stepper { value: count },
        }
    }
}

#[crate::component]
fn PlainBoard(start: i32) -> View {
    let count = state(start.get());
    let items = state(vec![10, 20]);
    let doubled = derived(move || count.get() * 2);
    crate::view! {
        box {
            style: column(),
            focusable: true,
            on_key_press: move |ev| {
                if ev.code == KeyCode::Char('+') {
                    count.update(|c| *c += 1);
                    let next = count.get();
                    items.update(|i| i.push(next));
                }
            },
            text(count.get()),
            text(doubled.get()),
            if count.get() > 1 { "big" } else { "small" },
            match count.get() % 2 {
                0 => "even",
                _ => "odd",
            },
            each _, item in items.get() key = item { text(item) },
            Stepper { value: count },
        }
    }
}

fn drive(root: impl FnOnce() -> View + Send + Sync + 'static) -> Vec<String> {
    let mut h = Harness::new(20, 8, root);
    let mut screens = vec![h.screen()];
    h.press(KeyCode::Tab);
    h.press(KeyCode::Char('+'));
    screens.push(h.screen());
    h.click_text("step");
    screens.push(h.screen());
    screens
}

#[test]
fn a_reactive_template_behaves_like_its_form_a_twin() {
    let reactive = drive(|| crate::view! { ReactiveBoard { start: 0 } });
    let plain = drive(|| crate::view! { PlainBoard { start: 0 } });

    assert_eq!(reactive, plain);
    assert_eq!(
        reactive,
        [
            "0\n0\nsmall\neven\n10\n20\nstep 0\n",
            "1\n2\nsmall\nodd\n10\n20\n1\nstep 1",
            "11\n22\nbig\nodd\n10\n20\n1\nstep 11",
        ]
    );
}

#[crate::component(reactive)]
fn EchoField() -> View {
    let name = state(String::new());
    let mut field = Style::new();
    field.width(Length::Cells(8));
    field.height(Length::Cells(1));
    crate::view! {
        box {
            style: column(),
            input { style: field.clone(), bind_value: name },
            text(name),
        }
    }
}

#[test]
fn bind_value_takes_the_handle_in_a_reactive_template() {
    let mut h = Harness::new(8, 2, || crate::view! { EchoField {} });

    h.press(KeyCode::Tab);
    h.type_text("hey");
    assert_eq!(h.row(1), "hey");
}

#[crate::component(reactive)]
fn Interpolated(count: i32) -> View {
    let width = 4usize;
    let ratio = crate::derived!(count as f64 / 4.0);
    crate::view! {
        box {
            style: column(),
            text("{count} left"),
            "{ratio:.2}|{count:>width$}|{{x}}",
            "{{static}}",
        }
    }
}

#[test]
fn a_text_literal_with_holes_updates_with_the_signals_it_names() {
    let count = state(3);
    let mut h = Harness::new(
        20,
        3,
        move || crate::view! { Interpolated { count: count } },
    );
    assert_eq!(h.screen(), "3 left\n0.75|   3|{x}\n{static}");

    count.set(6);
    assert_eq!(h.screen(), "6 left\n1.50|   6|{x}\n{static}");
}
