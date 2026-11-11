//! Tests for `ComponentProps` and `WithChildren`: props built by hand, mounted, and assigned newer
//! values while the component stays mounted.

use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Mutex};

use tuidom::Document;
use tuidom::headless::HeadlessRuntime;

use super::{Children, View, boxed, dyn_text, dynamic, mount, text};
use crate::{Callback, ComponentProps, Scope, Signal, WithChildren, effect, state};

fn row0(rt: &HeadlessRuntime) -> String {
    let row: String = (0..40)
        .filter_map(|x| rt.get_cell(x, 0).map(|c| c.text))
        .collect();
    row.trim_end().to_string()
}

/// Props built in a scope of their own and assigned from there, as a caller replacing a mounted
/// component's props would, so the newer props' signals are freed afterwards.
fn assign_built<P: ComponentProps>(mounted: &P, build: impl FnOnce() -> P) {
    let scope = Scope::new();
    let next = scope.run(build);
    mounted.assign(next);
    scope.dispose();
}

static LABEL_BODIES: AtomicI32 = AtomicI32::new(0);
static LABEL_BLOCKS: AtomicI32 = AtomicI32::new(0);

fn count_block(count: Signal<i32>) -> View {
    LABEL_BLOCKS.fetch_add(1, Ordering::SeqCst);
    text(count.get().to_string()).into()
}

#[crate::component]
fn Label(name: String, count: i32) -> View {
    LABEL_BODIES.fetch_add(1, Ordering::SeqCst);
    crate::view! {
        box {
            text(name.get()),
            text(" "),
            { count_block(count) },
        }
    }
}

#[test]
fn assigned_props_update_a_mounted_component_in_place() {
    let doc = Document::new().unwrap();
    let props = LabelProps::builder()
        .name(state("a".to_string()))
        .count(state(1))
        .build();
    let mounted = mount(&doc, doc.root(), props.clone().into_view()).unwrap();
    let roots = mounted.roots();
    let mut rt = HeadlessRuntime::new(doc, 40, 1);
    rt.render().unwrap();
    assert_eq!(row0(&rt), "a 1");
    let bodies = LABEL_BODIES.load(Ordering::SeqCst);
    let blocks = LABEL_BLOCKS.load(Ordering::SeqCst);

    assign_built(&props, || {
        LabelProps::builder()
            .name(state("b".to_string()))
            .count(state(1))
            .build()
    });
    rt.render().unwrap();
    assert_eq!(row0(&rt), "b 1");
    assert_eq!(
        LABEL_BLOCKS.load(Ordering::SeqCst),
        blocks,
        "an equal value reran a reader"
    );

    assign_built(&props, || {
        LabelProps::builder()
            .name(state("b".to_string()))
            .count(state(2))
            .build()
    });
    rt.render().unwrap();
    assert_eq!(row0(&rt), "b 2");
    assert_eq!(LABEL_BLOCKS.load(Ordering::SeqCst), blocks + 1);
    assert_eq!(LABEL_BODIES.load(Ordering::SeqCst), bodies);
    assert_eq!(mounted.roots(), roots);
}

#[derive(Default)]
struct Opaque(i32);

#[crate::component]
fn ShowOpaque(value: Opaque) -> View {
    dyn_text(move || value.with(|v| v.0.to_string())).into()
}

#[test]
fn a_prop_without_partial_eq_is_assigned_too() {
    let doc = Document::new().unwrap();
    let props = ShowOpaqueProps::builder().value(state(Opaque(1))).build();
    let _mounted = mount(&doc, doc.root(), props.clone().into_view()).unwrap();
    let mut rt = HeadlessRuntime::new(doc, 40, 1);
    rt.render().unwrap();
    assert_eq!(row0(&rt), "1");

    assign_built(&props, || {
        ShowOpaqueProps::builder().value(state(Opaque(7))).build()
    });
    rt.render().unwrap();
    assert_eq!(row0(&rt), "7");
}

#[crate::component]
fn Reporter(trigger: i32, on_change: Callback<i32>) -> View {
    effect(move || on_change.call(trigger.get()));
    text("").into()
}

#[test]
fn a_callback_prop_keeps_the_closure_it_mounted_with() {
    let doc = Document::new().unwrap();
    let first = Arc::new(Mutex::new(Vec::new()));
    let second = Arc::new(Mutex::new(Vec::new()));
    let seen = first.clone();
    let props = ReporterProps::builder()
        .trigger(state(1))
        .on_change(Callback::new(move |n| seen.lock().unwrap().push(n)))
        .build();
    let _mounted = mount(&doc, doc.root(), props.clone().into_view()).unwrap();

    let seen = second.clone();
    assign_built(&props, move || {
        ReporterProps::builder()
            .trigger(state(2))
            .on_change(Callback::new(move |n| seen.lock().unwrap().push(n)))
            .build()
    });
    assert_eq!(*first.lock().unwrap(), vec![1, 2]);
    assert!(second.lock().unwrap().is_empty());
}

#[crate::component]
fn Editor(value: Signal<i32>) -> View {
    dyn_text(move || value.get().to_string()).into()
}

#[test]
fn a_bound_signal_prop_stays_bound_to_the_caller_it_mounted_with() {
    let doc = Document::new().unwrap();
    let original = state(1);
    let props = EditorProps::builder()
        .value(crate::__private::Bound(original))
        .build();
    let _mounted = mount(&doc, doc.root(), props.clone().into_view()).unwrap();
    let mut rt = HeadlessRuntime::new(doc, 40, 1);

    let other = state(9);
    assign_built(&props, || {
        EditorProps::builder()
            .value(crate::__private::Bound(other))
            .build()
    });
    rt.render().unwrap();
    assert_eq!(row0(&rt), "1");
    assert_eq!(original.get(), 1);
    assert_eq!(other.get(), 9);

    original.set(3);
    rt.render().unwrap();
    assert_eq!(row0(&rt), "3");
}

#[crate::component]
fn Frame(title: String, children: Children) -> View {
    crate::view! {
        box {
            text(title.get()),
            { children },
        }
    }
}

#[test]
fn children_set_on_props_mount_inside_the_component() {
    let doc = Document::new().unwrap();
    let mut props = FrameProps::builder().title(state("t:".to_string())).build();
    props.set_children(Children::new(|| text("inside").into()));
    let _mounted = mount(&doc, doc.root(), props.into_view()).unwrap();
    let mut rt = HeadlessRuntime::new(doc, 40, 1);
    rt.render().unwrap();
    assert_eq!(row0(&rt), "t:inside");
}

#[test]
fn props_mount_without_a_view_template() {
    let doc = Document::new().unwrap();
    let props = ShowOpaqueProps::builder().value(state(Opaque(5))).build();
    let view = boxed()
        .child(props.into_view())
        .child(dynamic(|| text("!").into()));
    let _mounted = mount(&doc, doc.root(), view).unwrap();
    let mut rt = HeadlessRuntime::new(doc, 40, 1);
    rt.render().unwrap();
    assert_eq!(row0(&rt), "5!");
}
