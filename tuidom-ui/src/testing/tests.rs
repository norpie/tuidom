use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use tuidom::event::KeyCode;
use tuidom::style::{FlexDirection, Length, Style};

use super::Harness;
use crate::{Callback, View, component, dyn_text, on_cleanup, state, text, virtual_list};

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
