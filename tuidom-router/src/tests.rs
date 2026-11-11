use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use tuidom::NodeId;
use tuidom::animation::{Easing, TransitionConfig, TransitionProperty};
use tuidom::style;
use tuidom::style::Color;
use tuidom_ui::testing::Harness;
use tuidom_ui::{Callback, Children, Signal, View, component, state, view};

use crate::{Chain, routed};

#[component]
fn Shell(children: Children) -> View {
    view! {
        box {
            test_id: "shell",
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

#[component]
fn Other() -> View {
    view! { box { test_id: "other", text("other") } }
}

fn other() -> OtherProps {
    OtherProps::builder().build()
}

#[test]
fn a_kept_layout_keeps_its_nodes() {
    let route = state(0u8);
    let mut h = harness(route);
    let shell = h.find_attr("test_id", "outside").unwrap();

    route.set(1);
    assert_eq!(h.row(0).trim_end(), "A:S:two");
    assert_eq!(h.find_attr("test_id", "outside"), Some(shell));
}

#[test]
fn a_removed_layout_remounts_the_entries_after_it() {
    let route = state(1u8);
    let mut h = harness(route);
    let shell = h.find_attr("test_id", "outside").unwrap();
    let page = h.find_attr("test_id", "inside").unwrap();

    route.set(0);
    assert_eq!(h.row(0).trim_end(), "A:one");
    assert_eq!(h.find_attr("test_id", "outside"), Some(shell));
    let remounted = h.find_attr("test_id", "inside").unwrap();
    assert_ne!(remounted, page);
    assert!(h.document().get_parent(page).is_none());
}

#[test]
fn kept_entries_take_new_props_through_their_signals() {
    let names = state(("S:", "two"));
    let mut h = Harness::new(20, 3, move || {
        routed(move || {
            let (name, title) = names.get();
            Chain::layout(shell)
                .layout(move || section(name))
                .leaf(move || page(title))
        })
    });
    let page_node = h.find_attr("test_id", "inside").unwrap();
    assert_eq!(h.row(0).trim_end(), "A:S:two");

    names.set(("T:", "three"));
    assert_eq!(h.row(0).trim_end(), "A:T:three");
    assert_eq!(h.find_attr("test_id", "inside"), Some(page_node));
}

static COUNTED_BODIES: AtomicUsize = AtomicUsize::new(0);

#[component]
fn CountedShell(children: Children) -> View {
    COUNTED_BODIES.fetch_add(1, Ordering::SeqCst);
    view! { box { { children } } }
}

#[test]
fn a_layout_body_runs_once_across_leaf_swaps() {
    let route = state(0u8);
    let mut h = Harness::new(20, 3, move || {
        routed(move || {
            let shell = || CountedShellProps::builder().build();
            match route.get() % 3 {
                0 => Chain::layout(shell).leaf(|| page("one")),
                1 => Chain::layout(shell).leaf(other),
                _ => Chain::layout(shell).leaf(|| page("two")),
            }
        })
    });
    for n in 1..=9 {
        route.set(n);
        h.row(0);
    }
    assert_eq!(h.row(0).trim_end(), "one");
    assert_eq!(COUNTED_BODIES.load(Ordering::SeqCst), 1);
}

#[component]
fn Sidebar(children: Children) -> View {
    view! {
        box {
            style: style! { flex_direction: row },
            box {
                test_id: "sidebar",
                style: style! { height: 2, width: 3, flex_direction: column, overflow_y: scroll },
                text("1"), text("2"), text("3"), text("4"), text("5"),
            },
            { children },
        }
    }
}

#[test]
fn a_kept_layout_keeps_its_scroll_offset() {
    let route = state(0u8);
    let mut h = Harness::new(20, 3, move || {
        routed(move || {
            let shell = || SidebarProps::builder().build();
            if route.get() == 0 {
                Chain::layout(shell).leaf(|| page("one"))
            } else {
                Chain::layout(shell).leaf(other)
            }
        })
    });
    let sidebar = h.find_attr("test_id", "sidebar").unwrap();
    h.document().scroll_to(sidebar, 0, 2).unwrap();
    assert!(h.row(0).starts_with('3'));
    assert!(h.row(0).trim_end().ends_with("one"));

    route.set(1);
    assert!(h.row(0).trim_end().ends_with("other"));
    assert!(h.row(0).starts_with('3'));
    assert_eq!(h.document().scroll_offset(sidebar).y, 2);
}

#[component]
fn Fading(dark: bool, on_end: Callback<()>, children: Children) -> View {
    view! {
        box {
            style: {
                let mut style = style! { width: 4, height: 1 };
                style.background(if dark.get() { Color::black() } else { Color::white() });
                style
            },
            transitions: [TransitionConfig::new(
                TransitionProperty::Background,
                Duration::from_millis(1000),
                Easing::Linear,
            )],
            on_transition_end: move || on_end.call(()),
            { children },
        }
    }
}

#[test]
fn a_transition_on_a_kept_layout_runs_to_its_end() {
    let route = state(0u8);
    let dark = state(false);
    let ended = state(0u32);
    let mut h = Harness::new(20, 3, move || {
        routed(move || {
            let shell = move || {
                FadingProps::builder()
                    .dark(dark)
                    .on_end(Callback::new(move |_| ended.update(|n| *n += 1)))
                    .build()
            };
            if route.get() == 0 {
                Chain::layout(shell).leaf(|| page("one"))
            } else {
                Chain::layout(shell).leaf(other)
            }
        })
    });

    dark.set(true);
    h.advance(Duration::from_millis(500));
    route.set(1);
    h.advance(Duration::from_millis(500));
    assert_eq!(ended.get(), 1);
}

#[component]
fn Dialog() -> View {
    view! {
        box {
            style: style! { stacking_context: true },
            trap_focus: true,
            box { test_id: "dialog", focusable: true, text("ok") },
        }
    }
}

/// The entry box a component's root node mounts in.
///
/// The focus context depths below count the document's permanent root context.
fn entry_box(h: &mut Harness, test_id: &str) -> NodeId {
    let root = h.find_attr("test_id", test_id).unwrap();
    h.document().get_parent(root).unwrap()
}

#[test]
fn trapping_entries_open_contexts_in_chain_order() {
    let route = state(0u8);
    let mut h = Harness::new(20, 3, move || {
        routed(move || match route.get() {
            0 => Chain::layout(shell)
                .trap_focus()
                .leaf(|| page("one"))
                .trap_focus(),
            1 => Chain::layout(shell).trap_focus().leaf(other).trap_focus(),
            2 => Chain::layout(shell).trap_focus().leaf(|| page("two")),
            _ => Chain::leaf(other),
        })
    });
    let shell_box = entry_box(&mut h, "shell");
    assert_eq!(h.document().focus_context_depth(), 3);
    assert_eq!(
        h.document().active_focus_context(),
        entry_box(&mut h, "inside")
    );

    route.set(1);
    h.row(0);
    assert_eq!(h.document().focus_context_depth(), 3);
    assert_eq!(
        h.document().active_focus_context(),
        entry_box(&mut h, "other")
    );
    assert_eq!(entry_box(&mut h, "shell"), shell_box);

    route.set(0);
    h.row(0);
    assert_eq!(h.document().focus_context_depth(), 3);
    assert_eq!(
        h.document().active_focus_context(),
        entry_box(&mut h, "inside")
    );

    route.set(2);
    h.row(0);
    assert_eq!(h.document().focus_context_depth(), 2);
    assert_eq!(h.document().active_focus_context(), shell_box);

    route.set(3);
    h.row(0);
    assert_eq!(h.document().focus_context_depth(), 1);
    assert_eq!(h.document().active_focus_context(), h.document().root());
}

#[test]
fn focus_does_not_stay_in_a_disposed_leaf() {
    let route = state(0u8);
    let mut h = Harness::new(20, 3, move || {
        routed(move || {
            if route.get() == 0 {
                Chain::layout(shell).leaf(|| page("one"))
            } else {
                Chain::layout(shell).leaf(other)
            }
        })
    });
    let inside = h.find_attr("test_id", "inside").unwrap();
    h.focus(inside);
    assert_eq!(h.document().focused(), Some(inside));

    route.set(1);
    h.row(0);
    let focused = h.document().focused();
    assert_ne!(focused, Some(inside));
    assert!(focused.is_none_or(|node| h.document().get_parent(node).is_some()));
}

#[test]
fn closing_a_leaf_context_returns_focus_to_the_shell() {
    let route = state(0u8);
    let mut h = Harness::new(20, 3, move || {
        routed(move || {
            if route.get() == 0 {
                Chain::layout(shell).leaf(other)
            } else {
                Chain::layout(shell).leaf(|| page("one")).trap_focus()
            }
        })
    });
    let outside = h.find_attr("test_id", "outside").unwrap();
    h.focus(outside);

    route.set(1);
    h.row(0);
    assert_eq!(h.document().focused(), h.find_attr("test_id", "inside"));

    route.set(0);
    h.row(0);
    assert_eq!(h.document().focus_context_depth(), 1);
    assert_eq!(h.document().focused(), Some(outside));
}

#[test]
fn a_context_opened_inside_a_leaf_closes_with_it() {
    let route = state(0u8);
    let mut h = Harness::new(20, 3, move || {
        routed(move || {
            if route.get() == 0 {
                Chain::layout(shell).leaf(|| DialogProps::builder().build())
            } else {
                Chain::layout(shell).leaf(other)
            }
        })
    });
    assert_eq!(h.document().focus_context_depth(), 2);
    assert_eq!(h.document().focused(), h.find_attr("test_id", "dialog"));
    let outside = h.find_attr("test_id", "outside").unwrap();
    assert!(h.document().is_inert(outside).unwrap());

    route.set(1);
    h.row(0);
    assert_eq!(h.document().focus_context_depth(), 1);
    assert!(!h.document().is_inert(outside).unwrap());
}

#[test]
fn a_kept_entry_takes_its_new_box_style_in_place() {
    let padded = state(false);
    let mut h = Harness::new(20, 3, move || {
        routed(move || {
            let chain = Chain::leaf(|| page("one"));
            if padded.get() {
                chain.style(style! { padding: 1 3 })
            } else {
                chain
            }
        })
    });
    let inside = h.find_attr("test_id", "inside").unwrap();
    assert_eq!(h.row(0).trim_end(), "one");

    padded.set(true);
    assert_eq!(h.row(1).trim_end(), "   one");
    assert_eq!(h.find_attr("test_id", "inside"), Some(inside));
}

#[component]
fn Scroller() -> View {
    view! {
        box {
            test_id: "list",
            style: style! { height: 2, width: 6, flex_direction: column, overflow_y: scroll },
            text("1"), text("2"), text("3"), text("4"), text("5"),
        }
    }
}

/// Route 0 shows `retained` in the shell's slot, any other route shows `Other` there.
fn retaining_harness<P, F>(route: Signal<u8>, retained: F) -> Harness
where
    P: tuidom_ui::ComponentProps,
    F: Fn() -> P + Send + Sync + Clone + 'static,
{
    Harness::new(20, 3, move || {
        let retained = retained.clone();
        routed(move || {
            if route.get() == 0 {
                Chain::layout(shell).leaf(retained.clone()).retain()
            } else {
                Chain::layout(shell).leaf(other)
            }
        })
    })
}

#[test]
fn a_retained_leaf_comes_back_with_its_nodes_and_scroll_offset() {
    let route = state(0u8);
    let mut h = retaining_harness(route, || ScrollerProps::builder().build());
    let list = h.find_attr("test_id", "list").unwrap();
    h.document().scroll_to(list, 0, 2).unwrap();
    assert!(h.row(0).starts_with("A:3"));

    route.set(1);
    assert_eq!(h.row(0).trim_end(), "A:other");
    assert!(h.document().get_parent(list).is_some());

    route.set(0);
    assert!(h.row(0).starts_with("A:3"));
    assert_eq!(h.find_attr("test_id", "list"), Some(list));
    assert_eq!(h.document().scroll_offset(list).y, 2);
}

#[test]
fn a_retained_leaf_takes_the_returning_chain_s_props() {
    let route = state(0u8);
    let title = state("one");
    let mut h = Harness::new(20, 3, move || {
        routed(move || {
            if route.get() == 0 {
                let title = title.get();
                Chain::layout(shell).leaf(move || page(title)).retain()
            } else {
                Chain::layout(shell).leaf(other)
            }
        })
    });
    let inside = h.find_attr("test_id", "inside").unwrap();

    route.set(1);
    h.row(0);
    title.set("two");
    route.set(0);
    assert_eq!(h.row(0).trim_end(), "A:two");
    assert_eq!(h.find_attr("test_id", "inside"), Some(inside));
}

#[test]
fn a_leaf_that_is_not_retained_is_disposed_when_left() {
    let route = state(0u8);
    let mut h = harness(route);
    let inside = h.find_attr("test_id", "inside").unwrap();

    route.set(1);
    h.row(0);
    assert!(h.document().get_parent(inside).is_none());
    route.set(0);
    h.row(0);
    assert_ne!(h.find_attr("test_id", "inside"), Some(inside));
}

#[test]
fn focus_does_not_stay_in_a_hidden_retained_leaf() {
    let route = state(0u8);
    let mut h = retaining_harness(route, || page("one"));
    let inside = h.find_attr("test_id", "inside").unwrap();
    h.focus(inside);

    route.set(1);
    h.row(0);
    assert_ne!(h.document().focused(), Some(inside));
}

#[test]
fn a_retained_leaf_is_disposed_with_its_layout() {
    let route = state(0u8);
    let mut h = Harness::new(20, 3, move || {
        routed(move || match route.get() {
            0 => Chain::layout(shell).leaf(|| page("one")).retain(),
            1 => Chain::layout(shell).leaf(other),
            _ => Chain::leaf(other),
        })
    });
    let inside = h.find_attr("test_id", "inside").unwrap();

    route.set(1);
    h.row(0);
    assert!(h.document().get_parent(inside).is_some());

    route.set(2);
    h.row(0);
    assert!(h.document().get_parent(inside).is_none());

    route.set(0);
    assert_eq!(h.row(0).trim_end(), "A:one");
    assert_ne!(h.find_attr("test_id", "inside"), Some(inside));
}

#[test]
fn a_hidden_retained_trap_leaves_the_document_interactive() {
    let route = state(0u8);
    let mut h = Harness::new(20, 3, move || {
        routed(move || {
            if route.get() == 0 {
                Chain::layout(shell)
                    .leaf(|| page("one"))
                    .trap_focus()
                    .retain()
            } else {
                Chain::layout(shell).leaf(other)
            }
        })
    });
    let inside = h.find_attr("test_id", "inside").unwrap();
    let outside = h.find_attr("test_id", "outside").unwrap();
    assert_eq!(h.document().focused(), Some(inside));

    route.set(1);
    h.row(0);
    assert_eq!(h.document().active_focus_context(), h.document().root());
    assert!(!h.document().is_inert(outside).unwrap());

    route.set(0);
    h.row(0);
    assert_eq!(h.document().focused(), Some(inside));
    assert!(h.document().is_inert(outside).unwrap());
}

#[test]
fn a_dialog_left_open_in_a_retained_leaf_traps_only_while_shown() {
    let route = state(0u8);
    let mut h = retaining_harness(route, || DialogProps::builder().build());
    let dialog = h.find_attr("test_id", "dialog").unwrap();
    let outside = h.find_attr("test_id", "outside").unwrap();
    assert_eq!(h.document().focused(), Some(dialog));

    route.set(1);
    h.row(0);
    assert!(!h.document().is_inert(outside).unwrap());
    h.focus(outside);

    route.set(0);
    h.row(0);
    assert_eq!(h.document().focused(), Some(dialog));
    assert!(h.document().is_inert(outside).unwrap());
}

#[component]
fn Numbers() -> View {
    let mut style = style! { width: 6, height: 2 };
    style.flex_shrink(0.0);
    tuidom_ui::virtual_list(
        style,
        1,
        0,
        || 100,
        |i| view! { box { test_id: format!("n{i}"), text(i.to_string()) } },
    )
    .into()
}

#[test]
fn a_hidden_virtual_list_keeps_its_window() {
    let route = state(0u8);
    let mut h = retaining_harness(route, || NumbersProps::builder().build());
    let first = h.find_attr("test_id", "n0").unwrap();
    let content = h.document().get_parent(first).unwrap();
    let container = h.document().get_parent(content).unwrap();
    h.document().scroll_to(container, 0, 50).unwrap();
    assert!(h.row(0).starts_with("A:50"));
    let fiftieth = h.find_attr("test_id", "n50").unwrap();

    route.set(1);
    h.row(0);
    assert_eq!(h.find_attr("test_id", "n50"), Some(fiftieth));
    assert_eq!(h.find_attr("test_id", "n0"), None);

    route.set(0);
    assert!(h.row(0).starts_with("A:50"));
    assert_eq!(h.find_attr("test_id", "n50"), Some(fiftieth));
}
