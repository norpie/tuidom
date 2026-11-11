use tuidom::style;
use tuidom_ui::testing::Harness;
use tuidom_ui::{Children, Signal, View, component, state, view};

use crate::{Chain, routed};

#[component]
fn Shell(children: Children) -> View {
    view! {
        box {
            style: style! { flex_direction: row },
            box { test_id: "outside", focusable: true, text("A:") },
            { children },
        }
    }
}

#[component]
fn Section(name: String, children: Children) -> View {
    view! {
        box {
            style: style! { flex_direction: row },
            text(name.get()),
            { children },
        }
    }
}

#[component]
fn Page(title: String) -> View {
    view! { box { test_id: "inside", focusable: true, text(title.get()) } }
}

fn shell() -> ShellProps {
    ShellProps::builder().build()
}

fn section(name: &'static str) -> SectionProps {
    SectionProps::builder()
        .name(state(name.to_string()))
        .build()
}

fn page(title: &'static str) -> PageProps {
    PageProps::builder().title(state(title.to_string())).build()
}

fn chain_for(route: u8) -> Chain {
    match route {
        0 => Chain::layout(shell).leaf(|| page("one")),
        1 => Chain::layout(shell)
            .layout(|| section("S:"))
            .leaf(|| page("two")),
        _ => Chain::leaf(|| page("three")),
    }
}

fn harness(route: Signal<u8>) -> Harness {
    Harness::new(20, 3, move || routed(move || chain_for(route.get())))
}

#[test]
fn each_layout_holds_the_next_entry_in_its_slot() {
    let route = state(1u8);
    let mut h = harness(route);
    assert_eq!(h.row(0).trim_end(), "A:S:two");
}

#[test]
fn a_route_change_shows_the_new_chain() {
    let route = state(0u8);
    let mut h = harness(route);
    assert_eq!(h.row(0).trim_end(), "A:one");

    route.set(1);
    assert_eq!(h.row(0).trim_end(), "A:S:two");

    route.set(2);
    assert_eq!(h.row(0).trim_end(), "three");

    route.set(0);
    assert_eq!(h.row(0).trim_end(), "A:one");
}

#[test]
fn an_entry_style_replaces_its_box_style() {
    let mut h = Harness::new(20, 3, || {
        routed(|| Chain::leaf(|| page("one")).style(style! { padding: 1 3 }))
    });
    assert_eq!(h.row(0).trim_end(), "");
    assert_eq!(h.row(1).trim_end(), "   one");
}

#[test]
fn an_entry_that_traps_focus_holds_it_inside_its_box() {
    let mut h = Harness::new(20, 3, || {
        routed(|| Chain::layout(shell).leaf(|| page("one")).trap_focus())
    });
    let inside = h.find_attr("test_id", "inside");
    let outside = h.find_attr("test_id", "outside").unwrap();
    assert!(inside.is_some());
    assert_eq!(h.document().focused(), inside);
    assert!(h.document().is_inert(outside).unwrap());

    h.press(tuidom::event::KeyCode::Tab);
    assert_eq!(h.document().focused(), inside);
}
