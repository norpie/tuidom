use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use tuidom::animation::{Easing, TransitionConfig, TransitionProperty};
use tuidom::event::KeyCode;
use tuidom::headless::ScreenColor;
use tuidom::style::{Color, FlexDirection, Length, Style};

use super::Harness;
use crate::{Callback, Signal, View, component, dyn_text, on_cleanup, state, text, virtual_list};

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

#[crate::component]
fn Button(label: String, on_press: Callback<()>) -> View {
    crate::view! {
        box {
            on_click: move || on_press.call(()),
            text(label.get()),
        }
    }
}

fn column() -> Style {
    let mut style = Style::new();
    style.flex_direction(FlexDirection::Column);
    style
}

#[test]
fn key_presses_drive_a_component_and_repaint() {
    let mut h = Harness::new(10, 1, || crate::view! { Counter { start: 0 } });
    assert_eq!(h.row(0), "0");

    h.press(KeyCode::Tab);
    h.press(KeyCode::Char('+'));
    h.press(KeyCode::Char('+'));
    assert_eq!(h.row(0), "2");
}

#[test]
fn a_signal_passed_in_reads_its_new_value_after_a_click() {
    let clicks = state(0);
    let mut h = Harness::new(10, 1, move || {
        crate::view! {
            Button { label: "press".to_string(), on_press: move |_| clicks.update(|c| *c += 1) }
        }
    });

    h.click(0, 0);
    h.click(2, 0);
    assert_eq!(clicks.get(), 2);
}

#[test]
fn a_write_from_the_test_body_is_visible_on_the_next_read() {
    let label = state("before");
    let mut h = Harness::new(10, 1, move || dyn_text(move || label.get().to_string()));
    assert_eq!(h.row(0), "before");

    // The test body is outside any event-task span, so the write defers like an off-thread one:
    // a raw render still shows the old text, and the harness read settles it.
    label.set("after");
    h.runtime().render().unwrap();
    assert_eq!(h.runtime().get_cell(0, 0).unwrap().text, "b");
    assert_eq!(h.row(0), "after");
}

#[test]
fn screen_joins_every_row_with_trailing_blanks_trimmed() {
    let mut h = Harness::new(6, 3, || {
        crate::view! {
            box { style: column(), text("one"), text("two"), text("three") }
        }
    });
    assert_eq!(h.screen(), "one\ntwo\nthree");
}

#[test]
fn settling_renders_the_extra_frame_a_virtual_list_needs() {
    // The first frame only teaches the list its viewport; the items appear on the second. Read
    // through the raw runtime, so the screen is whatever construction's settle left behind.
    let mut h = Harness::new(4, 5, || {
        let mut style = Style::new();
        style.width(Length::Cells(4));
        style.height(Length::Cells(5));
        virtual_list(style, 1, 0, || 100, |i| text(i.to_string()))
    });
    let rt = h.runtime();
    assert_eq!(rt.get_cell(0, 0).unwrap().text, "0");
    assert_eq!(rt.get_cell(0, 4).unwrap().text, "4");
}

#[test]
fn dropping_the_harness_disposes_the_tree() {
    let cleaned = Arc::new(AtomicUsize::new(0));
    let c = cleaned.clone();
    let h = Harness::new(4, 1, move || {
        component(move || {
            on_cleanup(move || {
                c.fetch_add(1, Ordering::SeqCst);
            });
            text("x")
        })
    });
    let doc = h.document().clone();
    assert_eq!(doc.get_children(doc.root()).len(), 1);

    drop(h);
    assert_eq!(cleaned.load(Ordering::SeqCst), 1);
    assert_eq!(doc.get_children(doc.root()).len(), 0);
}

#[test]
fn click_text_presses_a_button_by_its_label() {
    let (saves, cancels) = (state(0), state(0));
    let mut h = Harness::new(20, 2, move || {
        crate::view! {
            box {
                style: column(),
                Button { label: "Save".to_string(), on_press: move |_| saves.update(|c| *c += 1) },
                Button { label: "Cancel".to_string(), on_press: move |_| cancels.update(|c| *c += 1) },
            }
        }
    });

    h.click_text("Cancel");
    assert_eq!((saves.get(), cancels.get()), (0, 1));
}

#[test]
fn find_attr_reaches_a_tagged_node_for_focus() {
    // Tab would land on the counter first; the attribute picks out the second box directly.
    let hits = state(0);
    let mut h = Harness::new(10, 2, move || {
        crate::view! {
            box {
                style: column(),
                Counter { start: 0 },
                box {
                    test_id: "second",
                    focusable: true,
                    on_key_press: move |ev| {
                        if ev.code == KeyCode::Char('+') {
                            hits.update(|c| *c += 1);
                        }
                    },
                    text(hits.get()),
                },
            }
        }
    });

    let second = h.find_attr("test_id", "second").unwrap();
    h.focus(second);
    h.press(KeyCode::Char('+'));
    assert_eq!(h.screen(), "0\n1");
    assert_eq!(h.find_attr("test_id", "missing"), None);
}

#[test]
fn find_text_is_none_for_absent_or_empty_text() {
    let mut h = Harness::new(10, 1, || text("hello"));
    assert_eq!(h.find_text("hello"), Some((0, 0)));
    assert_eq!(h.find_text("bye"), None);
    assert_eq!(h.find_text(""), None);
}

#[test]
fn find_text_counts_wide_glyphs_as_two_columns() {
    // "日本" takes four cells but two `char`s, so "OK" starts at column 5, not 3.
    let mut h = Harness::new(10, 2, || {
        crate::view! { box { style: column(), text("x"), text("日本 OK") } }
    });
    assert_eq!(h.find_text("OK"), Some((5, 1)));
}

#[test]
fn advance_steps_a_transition_partway_then_to_its_end() {
    let dark = state(false);
    let mut h = Harness::new(4, 1, move || {
        crate::view! {
            box {
                test_id: "fade",
                focusable: true,
                style: {
                    let mut style = Style::new();
                    style.width(Length::Cells(2));
                    style.height(Length::Cells(1));
                    style.background(if dark.get() { Color::black() } else { Color::white() });
                    style
                },
                on_key_press: move |_| dark.set(true),
            }
        }
    });

    let fade = h.find_attr("test_id", "fade").unwrap();
    h.document()
        .set_transition(
            fade,
            TransitionConfig::new(
                TransitionProperty::Background,
                Duration::from_millis(1000),
                Easing::Linear,
            ),
        )
        .unwrap();
    let white = h.cell(0, 0).unwrap().bg;

    h.focus(fade);
    h.press(KeyCode::Enter);
    h.advance(Duration::from_millis(500));
    let midway = h.cell(0, 0).unwrap().bg;

    h.advance(Duration::from_millis(500));
    let black = h.cell(0, 0).unwrap().bg;

    assert_eq!(white, Some(ScreenColor::from_rgb(255, 255, 255)));
    assert_eq!(black, Some(ScreenColor::from_rgb(0, 0, 0)));
    assert_ne!(midway, white);
    assert_ne!(midway, black);
}

#[test]
fn an_action_sees_a_test_body_write_made_before_it() {
    let on = state(false);
    let mut h = Harness::new(4, 1, move || {
        crate::view! { box { test_id: "target", focusable: on.get(), text("x") } }
    });

    // Had the write applied only after the key, Tab would have found nothing to focus.
    on.set(true);
    h.press(KeyCode::Tab);
    assert_eq!(h.document().focused(), h.find_attr("test_id", "target"));
}

#[test]
fn advance_applies_a_test_body_write_before_moving_the_clock() {
    let dark = state(false);
    let mut h = Harness::new(4, 1, move || {
        crate::view! {
            box {
                test_id: "fade",
                style: {
                    let mut style = Style::new();
                    style.width(Length::Cells(2));
                    style.height(Length::Cells(1));
                    style.background(if dark.get() { Color::black() } else { Color::white() });
                    style
                },
            }
        }
    });
    let fade = h.find_attr("test_id", "fade").unwrap();
    h.document()
        .set_transition(
            fade,
            TransitionConfig::new(
                TransitionProperty::Background,
                Duration::from_millis(1000),
                Easing::Linear,
            ),
        )
        .unwrap();

    // Had the write applied after the clock moved, the transition would start at 1000ms and
    // still be white here.
    dark.set(true);
    h.advance(Duration::from_millis(1000));
    assert_eq!(
        h.cell(0, 0).unwrap().bg,
        Some(ScreenColor::from_rgb(0, 0, 0))
    );
}

// ---- Worked example: a task picker, tested only through the harness ---------------------

/// One task row: focusable, picked with Enter, highlighted while it is the picked one.
#[crate::component]
fn TaskItem(
    name: &'static str,
    picked: Option<&'static str>,
    on_pick: Callback<&'static str>,
) -> View {
    crate::view! {
        box {
            test_id: name.get().to_string(),
            focusable: true,
            style: {
                let mut style = Style::new();
                style.height(Length::Cells(1));
                let highlighted = picked.get() == Some(name.get());
                style.background(if highlighted { Color::black() } else { Color::white() });
                style
            },
            on_key_press: move |ev| {
                if ev.code == KeyCode::Enter {
                    on_pick.call(name.get());
                }
            },
            text(name.get()),
        }
    }
}

/// A keyed list of tasks, a Done button, and a status line reporting the pick.
fn task_picker(tasks: Signal<Vec<&'static str>>, picked: Signal<Option<&'static str>>) -> View {
    let done = state(false);
    crate::view! {
        box {
            style: column(),
            each _, name in tasks.get() key = name {
                TaskItem { name: name, picked: picked.get(), on_pick: move |n| picked.set(Some(n)) }
            },
            Button { label: "Done".to_string(), on_press: move |_| done.set(true) },
            text(match (picked.get(), done.get()) {
                (None, _) => "nothing picked".to_string(),
                (Some(name), false) => format!("picked {name}"),
                (Some(name), true) => format!("done {name}"),
            }),
        }
    }
}

#[test]
fn worked_example_task_picker() {
    let tasks = state(vec!["write", "test", "ship"]);
    let picked = state(None);
    let mut h = Harness::new(16, 5, move || task_picker(tasks, picked));
    assert_eq!(h.screen(), "write\ntest\nship\nDone\nnothing picked");

    // The engine animates background changes once a transition is set, which a test reaches
    // through the tagged nodes.
    for name in ["write", "test", "ship"] {
        let node = h.find_attr("test_id", name).unwrap();
        h.document()
            .set_transition(
                node,
                TransitionConfig::new(
                    TransitionProperty::Background,
                    Duration::from_millis(200),
                    Easing::Linear,
                ),
            )
            .unwrap();
    }

    // Tab twice lands on "test"; Enter picks it, and the highlight fades in.
    h.press(KeyCode::Tab);
    h.press(KeyCode::Tab);
    h.press(KeyCode::Enter);
    assert_eq!(picked.get(), Some("test"));
    assert_eq!(h.row(4), "picked test");

    let white = Some(ScreenColor::from_rgb(255, 255, 255));
    let black = Some(ScreenColor::from_rgb(0, 0, 0));
    h.advance(Duration::from_millis(100));
    let midway = h.cell(0, 1).unwrap().bg;
    assert_ne!(midway, white);
    assert_ne!(midway, black);
    h.advance(Duration::from_millis(100));
    assert_eq!(h.cell(0, 1).unwrap().bg, black);

    // Reversing the list moves the keyed rows. Focus and the highlight go with "test", which
    // stays in the middle, while "ship" moves to the top.
    tasks.set(vec!["ship", "test", "write"]);
    assert_eq!(h.screen(), "ship\ntest\nwrite\nDone\npicked test");
    assert_eq!(h.cell(0, 0).unwrap().bg, white);
    assert_eq!(h.cell(0, 1).unwrap().bg, black);
    let test_row = h.find_attr("test_id", "test").unwrap();
    assert_eq!(h.document().focused(), Some(test_row));

    h.click_text("Done");
    assert_eq!(h.row(4), "done test");
}
