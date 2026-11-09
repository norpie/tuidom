//! Every component in `tuidom-components`, a section per tab, under a switchable theme.
//!
//! Tab / Shift-Tab   — move focus (hovering focuses too)
//! Left / Right      — switch sections while the tab strip is focused
//! Enter / Space     — press a button, toggle a checkbox, open or pick in a select or menu
//! Arrows            — move a radio selection, a slider, a list's highlight, or a split
//! PageUp / PageDown, Home / End — move through a list, tree, or table
//! Left / Right      — collapse or expand a tree node while the tree is focused
//! Mouse drag        — move the split's divider
//! Click a toast     — dismiss it
//! t                 — switch between the dark and light themes (outside a text field)
//! q                 — quit (outside a text field)

use std::collections::HashSet;
use tuidom::NodeKindView;
use tuidom::event::KeyCode;
use tuidom::style::{AlignItems, EdgeInsets, FlexDirection, FlexGap, Length, Style};
use tuidom_components::{
    Button, ButtonProps, ButtonVariant, Checkbox, CheckboxProps, Column, Density, Divider,
    DividerProps, KeyHint, KeyHintProps, Label, LabelProps, LabelTone, List, ListProps, Menu,
    MenuItem, MenuProps, Modal, ModalProps, Orientation, Panel, PanelProps, ProgressBar,
    ProgressBarProps, RadioGroup, RadioGroupProps, Scrollable, ScrollableProps, Select,
    SelectProps, Shape, Side, Slider, SliderProps, Spinner, SpinnerProps, Split, SplitProps,
    StatusBar, StatusBarProps, Surface, Table, TableProps, Tabs, TabsProps, TextInput,
    TextInputProps, Theme, ThemeScope, ThemeScopeProps, Toast, Toaster, ToasterProps, Tone,
    Tooltip, TooltipProps, Tree, TreeNode, TreeProps, install_theme, use_toaster,
};

use tuidom_ui::{App, Signal, View, component, fragment, state, text, use_document, view};

fn screen() -> Style {
    let mut style = Style::new();
    style.width(Length::Percent(100.0));
    style.height(Length::Percent(100.0));
    style.flex_direction(FlexDirection::Column);
    style
}

fn page() -> Style {
    let mut style = Style::new();
    style.flex_direction(FlexDirection::Column);
    style.flex_grow(1.0);
    style.padding(EdgeInsets::symmetric(2, 1));
    style.gap(FlexGap::new(1, 0));
    style
}

fn section() -> Style {
    let mut style = Style::new();
    style.flex_direction(FlexDirection::Column);
    style.padding(EdgeInsets::new(1, 0, 0, 0));
    style.gap(FlexGap::new(1, 0));
    style.flex_grow(1.0);
    style
}

fn row() -> Style {
    let mut style = Style::new();
    style.gap(FlexGap::new(0, 2));
    style
}

fn inputs_row() -> Style {
    let mut style = row();
    style.align_items(AlignItems::FlexStart);
    style
}

fn column(width: u16) -> Style {
    let mut style = Style::new();
    style.flex_direction(FlexDirection::Column);
    style.width(Length::Cells(width));
    style
}

fn sized(width: u16) -> Style {
    let mut style = Style::new();
    style.width(Length::Cells(width));
    style
}

fn tall(height: u16) -> Style {
    let mut style = Style::new();
    style.height(Length::Cells(height));
    style
}

fn grow() -> Style {
    let mut style = Style::new();
    style.flex_grow(1.0);
    style
}

fn heading() -> Style {
    let mut style = Style::new();
    style.bold(true);
    style
}

fn sections() -> Vec<String> {
    [
        "Buttons",
        "Inputs",
        "Containers",
        "Feedback",
        "Navigation",
        "Data",
        "Labels",
    ]
    .map(String::from)
    .to_vec()
}

fn plans() -> Vec<String> {
    ["Free", "Pro", "Team"].map(String::from).to_vec()
}

fn sizes() -> Vec<String> {
    ["Small", "Medium", "Large", "Extra large"]
        .map(String::from)
        .to_vec()
}

fn buttons(last: Signal<String>) -> View {
    view! {
        box {
            style: section(),
            box {
                style: row(),
                Button { label: "Save".to_string(), on_press: move |_| last.set("Save".to_string()) },
                Button {
                    label: "Cancel".to_string(),
                    variant: ButtonVariant::Secondary,
                    on_press: move |_| last.set("Cancel".to_string()),
                },
                Button {
                    label: "Delete".to_string(),
                    variant: ButtonVariant::Destructive,
                    on_press: move |_| last.set("Delete".to_string()),
                },
                Button {
                    label: "Outline".to_string(),
                    variant: ButtonVariant::Outline,
                    on_press: move |_| last.set("Outline".to_string()),
                },
                Button {
                    label: "Ghost".to_string(),
                    variant: ButtonVariant::Ghost,
                    on_press: move |_| last.set("Ghost".to_string()),
                },
                Button { label: "Disabled".to_string(), disabled: true },
            },
            Label { text: format!("Last pressed: {}", last.get()), tone: LabelTone::Muted },
        }
    }
}

/// The values the inputs are bound to, kept outside the tab so switching away keeps them.
#[derive(Clone, Copy)]
struct Fields {
    name: Signal<String>,
    password: Signal<String>,
    subscribe: Signal<bool>,
    plan: Signal<usize>,
    size: Signal<usize>,
    volume: Signal<f64>,
}

fn inputs(fields: Fields) -> View {
    let Fields {
        name,
        password,
        subscribe,
        plan,
        size,
        volume,
    } = fields;
    view! {
        box {
            style: section(),
            box {
                style: inputs_row(),
                box {
                    style: column(28),
                    TextInput { value: name, placeholder: "Your name".to_string() },
                    TextInput {
                        value: password,
                        placeholder: "Password".to_string(),
                        mask: Some('•'),
                    },
                    TextInput {
                        value: state(String::new()),
                        placeholder: "Disabled".to_string(),
                        disabled: true,
                    },
                },
                box {
                    style: column(16),
                    Checkbox { checked: subscribe, label: "Subscribe".to_string() },
                    RadioGroup { options: plans(), selected: plan },
                },
                Select { options: sizes(), selected: size },
            },
            Slider { value: volume, step: 5.0, style: sized(28) },
            Label {
                text: format!(
                    "name {:?} · password {} chars · subscribe {} · plan {} · size {} · volume {}",
                    name.get(),
                    password.get().chars().count(),
                    subscribe.get(),
                    plans()[plan.get()],
                    sizes()[size.get()],
                    volume.get(),
                ),
                tone: LabelTone::Muted,
            },
        }
    }
}

fn containers(ratio: Signal<f64>, modal: Signal<bool>) -> View {
    view! {
        box {
            style: section(),
            Split {
                ratio: ratio,
                style: tall(14),
                first: view! {
                    Panel {
                        title: "Files".to_string(),
                        style: grow(),
                        Scrollable {
                            style: grow(),
                            each _, n in 0..40 {
                                text(format!("src/file_{n:02}.rs"))
                            },
                        },
                    }
                },
                second: view! {
                    Panel {
                        title: "Details".to_string(),
                        style: grow(),
                        Label {
                            text: format!("Split at {:.0}%", ratio.get() * 100.0),
                            tone: LabelTone::Muted,
                        },
                        Button {
                            label: "Open modal".to_string(),
                            variant: ButtonVariant::Secondary,
                            on_press: move |_| modal.set(true),
                        },
                    }
                },
            },
            Modal {
                open: modal,
                title: "A modal".to_string(),
                Label { text: "Escape or a click outside closes it.".to_string() },
                Button { label: "Close".to_string(), on_press: move |_| modal.set(false) },
            },
        }
    }
}

/// Shows a toast in `tone` from a button.
fn toast_button(label: &'static str, tone: Tone) -> View {
    let toaster = use_toaster();
    let title = format!("{label} toast");
    view! {
        Button {
            label: label.to_string(),
            variant: ButtonVariant::Secondary,
            on_press: move |_| {
                if let Some(toaster) = toaster {
                    toaster.show(Toast {
                        title: title.clone(),
                        description: "Click it, or wait four seconds".to_string(),
                        tone,
                    });
                }
            },
        }
    }
}

#[component]
fn Feedback(progress: Signal<f64>) -> View {
    view! {
        box {
            style: section(),
            Spinner { label: "Working".to_string() },
            Slider { value: progress, step: 5.0, style: sized(40) },
            ProgressBar { value: progress.get() / 100.0, style: sized(40) },
            ProgressBar { value: progress.get() / 100.0, tone: Tone::Success, style: sized(40) },
            ProgressBar { value: progress.get() / 100.0, tone: Tone::Warning, style: sized(40) },
            ProgressBar { value: progress.get() / 100.0, tone: Tone::Destructive, style: sized(40) },
            box {
                style: row(),
                { toast_button("Default", Tone::Default) },
                { toast_button("Success", Tone::Success) },
                { toast_button("Warning", Tone::Warning) },
                { toast_button("Destructive", Tone::Destructive) },
                Tooltip {
                    text: "Shown while focused or hovered".to_string(),
                    side: Side::Below,
                    Button { label: "Tooltip".to_string(), variant: ButtonVariant::Ghost },
                },
            },
        }
    }
}

fn file_items() -> Vec<MenuItem> {
    vec![
        MenuItem::new("New").hint("n"),
        MenuItem::new("Open").hint("o"),
        MenuItem::new("Save").hint("s"),
        MenuItem::new("Save as"),
        MenuItem::new("Close").separated(),
    ]
}

fn edit_items() -> Vec<MenuItem> {
    vec![
        MenuItem::new("Undo").hint("u"),
        MenuItem::new("Redo").hint("r").disabled(),
        MenuItem::new("Cut").hint("x").separated(),
        MenuItem::new("Copy").hint("c"),
        MenuItem::new("Paste").hint("v"),
    ]
}

/// The outer theme with flat, compact buttons, so menu triggers sit in one row like a menu bar.
fn menu_bar(theme: &Theme) -> Theme {
    Theme {
        shape: Shape {
            button: Surface::Flat,
            density: Density::Compact,
            ..theme.shape.clone()
        },
        ..theme.clone()
    }
}

fn navigation(theme: Signal<Theme>, chosen: Signal<String>) -> View {
    view! {
        box {
            style: section(),
            ThemeScope {
                theme: menu_bar(&theme.get()),
                style: row(),
                Menu {
                    label: "File".to_string(),
                    items: file_items(),
                    on_select: move |i: usize| chosen.set(format!("File › {}", file_items()[i].label)),
                },
                Menu {
                    label: "Edit".to_string(),
                    items: edit_items(),
                    on_select: move |i: usize| chosen.set(format!("Edit › {}", edit_items()[i].label)),
                },
                Menu {
                    label: "Secondary".to_string(),
                    items: file_items(),
                    variant: ButtonVariant::Secondary,
                    on_select: move |i: usize| chosen.set(format!("Secondary › {}", file_items()[i].label)),
                },
            },
            Label { text: format!("Last chosen: {}", chosen.get()), tone: LabelTone::Muted },
            Label {
                text: "The status bar along the bottom is a StatusBar with KeyHints.".to_string(),
                tone: LabelTone::Muted,
            },
        }
    }
}

fn project() -> Vec<TreeNode> {
    let file = |dir: &str, name: &str| TreeNode::new(format!("{dir}/{name}"), name);
    vec![
        TreeNode::new("src", "src").with_children(vec![
            file("src", "lib.rs"),
            TreeNode::new("src/view", "view").with_children(vec![
                file("src/view", "mod.rs"),
                file("src/view", "tests.rs"),
            ]),
            TreeNode::new("src/runtime", "runtime").with_children(
                (0..30)
                    .map(|n| file("src/runtime", &format!("part_{n:02}.rs")))
                    .collect(),
            ),
        ]),
        TreeNode::new("docs", "docs")
            .with_children(vec![file("docs", "README.md"), file("docs", "GLOSSARY.md")]),
        TreeNode::new("Cargo.toml", "Cargo.toml"),
    ]
}

fn processes() -> Vec<Column> {
    vec![
        Column::new("PID", 6),
        Column::new("Name", 14),
        Column::new("CPU", 5),
        Column::new("Memory", 8),
    ]
}

/// The selections the data views write, kept outside the tab so switching away keeps them.
#[derive(Clone, Copy)]
struct Data {
    item: Signal<usize>,
    expanded: Signal<HashSet<String>>,
    node: Signal<Option<String>>,
    process: Signal<usize>,
    activated: Signal<String>,
}

fn data(data: Data) -> View {
    let Data {
        item,
        expanded,
        node,
        process,
        activated,
    } = data;
    view! {
        box {
            style: section(),
            box {
                style: row(),
                Panel {
                    title: "List · 100 000 rows".to_string(),
                    style: data_panel(24),
                    List {
                        count: 100_000,
                        selected: item,
                        render: move |i| text(format!("Item {i}")).into(),
                        on_activate: move |i: usize| activated.set(format!("item {i}")),
                        style: grow(),
                    },
                },
                Panel {
                    title: "Tree".to_string(),
                    style: data_panel(24),
                    Tree {
                        nodes: project(),
                        expanded: expanded,
                        selected: node,
                        on_activate: move |id: String| activated.set(id),
                        style: grow(),
                    },
                },
                Panel {
                    title: "Table · 10 000 rows".to_string(),
                    style: data_panel(44),
                    Table {
                        columns: processes(),
                        count: 10_000,
                        cell: move |(row, column): (usize, usize)| match column {
                            0 => format!("{}", 1000 + row),
                            1 => format!("worker-{row}"),
                            2 => format!("{}.{}%", row % 100, row % 10),
                            _ => format!("{} MB", 16 + row % 512),
                        },
                        selected: process,
                        on_activate: move |i: usize| activated.set(format!("process {}", 1000 + i)),
                        style: grow(),
                    },
                },
            },
            Label {
                text: format!("Enter on a row activates it · last activated: {}", activated.get()),
                tone: LabelTone::Muted,
            },
        }
    }
}

fn data_panel(width: u16) -> Style {
    let mut style = Style::new();
    style.width(Length::Cells(width));
    style.height(Length::Cells(16));
    style
}

fn labels() -> View {
    view! {
        box {
            style: section(),
            box {
                style: row(),
                Label { text: "Default tone".to_string() },
                Divider { orientation: Orientation::Vertical },
                Label { text: "Muted tone".to_string(), tone: LabelTone::Muted },
            },
        }
    }
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    App::new(|| {
        let theme = install_theme(Theme::dark());
        let dark = state(true);
        let tab = state(0_usize);
        let last = state(String::from("nothing yet"));
        let fields = Fields {
            name: state(String::new()),
            password: state(String::new()),
            subscribe: state(true),
            plan: state(0),
            size: state(1),
            volume: state(40.0),
        };
        let ratio = state(0.4);
        let progress = state(60.0);
        let modal = state(false);
        let chosen = state(String::from("nothing yet"));
        let data_state = Data {
            item: state(0),
            expanded: state(HashSet::from(["src".to_string()])),
            node: state(Some("src".to_string())),
            process: state(0),
            activated: state(String::from("nothing yet")),
        };

        let doc = use_document().expect("an App body has a document");
        let quit = doc.clone();
        doc.on_key_press(doc.root(), move |key| {
            // Keys typed into a text field bubble here too; they are text, not shortcuts.
            let typing = quit
                .get_node(key.target())
                .is_some_and(|node| matches!(node.kind, NodeKindView::Input { .. }));
            if typing {
                return;
            }
            match key.code {
                KeyCode::Char('t') => {
                    let next = !dark.get();
                    dark.set(next);
                    theme.set(if next { Theme::dark() } else { Theme::light() });
                }
                KeyCode::Char('q') => quit.quit(),
                _ => {}
            }
        })
        .expect("registering the key handler");

        view! {
            Toaster {
                box {
                    style: screen(),
                    box {
                        style: page(),
                        Label { text: "tuidom-components".to_string(), style: heading() },
                        Divider {},
                        Tabs {
                            tabs: sections(),
                            selected: tab,
                            style: grow(),
                            match tab.get() {
                                0 => { buttons(last) },
                                1 => { inputs(fields) },
                                2 => { containers(ratio, modal) },
                                3 => Feedback { progress: progress },
                                4 => { navigation(theme, chosen) },
                                5 => { data(data_state) },
                                _ => { labels() },
                            }
                        },
                    },
                    StatusBar {
                        left: fragment([
                            view! { KeyHint { key: "tab".to_string(), label: "focus".to_string() } },
                            view! { KeyHint { key: "←/→".to_string(), label: "sections".to_string() } },
                            view! { KeyHint { key: "t".to_string(), label: "theme".to_string() } },
                            view! { KeyHint { key: "q".to_string(), label: "quit".to_string() } },
                        ]),
                        right: view! { text(sections()[tab.get()].clone()) },
                    },
                }
            }
        }
    })
    .run()
    .await
}
