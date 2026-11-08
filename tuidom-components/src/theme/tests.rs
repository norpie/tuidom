use std::sync::{Arc, Mutex};

use tuidom::NodeId;
use tuidom::style::{Color, FlexDirection, ResolvedColor, Style};
use tuidom_ui::testing::Harness;
use tuidom_ui::{Signal, View, component, view};

use super::*;

fn literal(color: &Color) -> ResolvedColor {
    match color {
        Color::Literal(resolved) => *resolved,
        other => panic!("palette color is not a literal: {other:?}"),
    }
}

fn filled(var: &str) -> Style {
    let mut style = Style::new();
    style.background(Color::var(var));
    style
}

fn background(h: &Harness, node: NodeId) -> Option<ResolvedColor> {
    h.document().resolved_style(node).unwrap().background
}

#[test]
fn install_declares_the_palette_and_paints_the_root() {
    let h = Harness::new(8, 2, || {
        install_theme(Theme::dark());
        view! { box { text("x") } }
    });
    let doc = h.document();
    let palette = Palette::dark();
    for (name, color) in palette.vars() {
        assert_eq!(doc.color_var(name).as_ref(), Some(color), "{name}");
    }
    assert_eq!(doc.terminal_background(), palette.background);
    assert_eq!(
        background(&h, doc.root()),
        Some(literal(&palette.background))
    );
}

#[test]
fn a_transparent_root_is_unpainted_but_assumes_the_palette_behind_it() {
    let h = Harness::new(8, 2, || {
        install_theme(Theme {
            root: RootBackground::Transparent,
            ..Theme::light()
        });
        view! { box { text("x") } }
    });
    let doc = h.document();
    let root = doc.resolved_style(doc.root()).unwrap();
    assert_eq!(root.background, None);
    assert_eq!(
        root.effective_background,
        literal(&Palette::light().background)
    );
}

#[test]
fn install_keeps_the_rest_of_the_root_style() {
    let h = Harness::new(8, 2, || {
        let doc = tuidom_ui::use_document().unwrap();
        doc.update_style(doc.root(), |s| s.bold(true)).unwrap();
        install_theme(Theme::dark());
        view! { box { text("x") } }
    });
    let doc = h.document();
    assert!(doc.resolved_style(doc.root()).unwrap().bold);
}

#[test]
fn switching_restyles_the_same_nodes_at_once() {
    let slot: Arc<Mutex<Option<Signal<Theme>>>> = Arc::default();
    let handle = Arc::clone(&slot);
    let mut h = Harness::new(8, 2, move || {
        *handle.lock().unwrap() = Some(install_theme(Theme::dark()));
        view! { box { test_id: "button", style: filled("--primary"), text("ok") } }
    });
    let theme = slot.lock().unwrap().unwrap();
    let before = h.find_attr("test_id", "button").unwrap();
    let dark_cell = h.cell(0, 0).unwrap();

    theme.set(Theme::light());
    let after = h.find_attr("test_id", "button").unwrap();
    assert_eq!(after, before);
    // No time has passed, so this also shows a variable change does not start a transition.
    assert_eq!(
        background(&h, after),
        Some(literal(&Palette::light().primary))
    );
    assert_ne!(h.cell(0, 0).unwrap().bg, dark_cell.bg);
}

#[component]
fn ShowDensity() -> View {
    let shape = use_shape();
    view! { box { text(format!("{:?}", shape.get().density)) } }
}

#[test]
fn a_scope_shadows_the_outer_theme() {
    let mut h = Harness::new(24, 2, || {
        install_theme(Theme::dark());
        let inner = Theme {
            shape: Shape {
                density: Density::Compact,
                ..Shape::default()
            },
            ..Theme::light()
        };
        view! {
            box {
                style: column(),
                box { test_id: "outer", style: filled("--card"), ShowDensity {} },
                ThemeScope {
                    theme: inner.clone(),
                    box { test_id: "inner", style: filled("--card"), ShowDensity {} },
                },
            }
        }
    });
    let outer = h.find_attr("test_id", "outer").unwrap();
    let inner = h.find_attr("test_id", "inner").unwrap();
    assert_eq!(background(&h, outer), Some(literal(&Palette::dark().card)));
    assert_eq!(background(&h, inner), Some(literal(&Palette::light().card)));
    assert_eq!(h.row(0).trim_end(), "Comfortable");
    assert_eq!(h.row(1).trim_end(), "Compact");
}

#[test]
fn a_scope_merges_its_style_prop_over_the_declarations() {
    let mut h = Harness::new(8, 2, || {
        install_theme(Theme::dark());
        view! {
            ThemeScope {
                theme: Theme::light(),
                style: filled("--background"),
                box { test_id: "inside", text("x") },
            }
        }
    });
    let inside = h.find_attr("test_id", "inside").unwrap();
    let scope = h.document().get_node(inside).unwrap().parent.unwrap();
    assert_eq!(
        background(&h, scope),
        Some(literal(&Palette::light().background))
    );
}

#[test]
fn without_a_theme_the_shape_is_the_default() {
    let mut h = Harness::new(16, 1, || view! { ShowDensity {} });
    assert_eq!(h.row(0).trim_end(), "Comfortable");
}

fn column() -> Style {
    let mut style = Style::new();
    style.flex_direction(FlexDirection::Column);
    style
}

#[test]
fn plain_text_takes_the_nearest_foreground() {
    let mut h = Harness::new(8, 2, || {
        install_theme(Theme::dark());
        view! {
            box {
                style: column(),
                box { test_id: "outside", text("a") },
                ThemeScope {
                    theme: Theme::light(),
                    box { test_id: "inside", text("b") },
                },
            }
        }
    });
    let text_in = |h: &mut Harness, id: &str| {
        let parent = h.find_attr("test_id", id).unwrap();
        let text = h.document().get_node(parent).unwrap().children[0];
        h.document().resolved_style(text).unwrap().color
    };
    assert_eq!(
        text_in(&mut h, "outside"),
        literal(&Palette::dark().foreground)
    );
    assert_eq!(
        text_in(&mut h, "inside"),
        literal(&Palette::light().foreground)
    );
}
