//! Every component in `tuidom-components`, one at a time, under a switchable theme.
//!
//! Tab / Shift-Tab   — move focus (hovering focuses too)
//! Enter / Space     — press a button, toggle a checkbox, open or pick in a select
//! Arrows            — move a radio selection, a slider, or a select's highlight
//! t                 — switch between the dark and light themes (outside a text field)
//! q                 — quit (outside a text field)

use tuidom::NodeKindView;
use tuidom::event::KeyCode;
use tuidom::style::{AlignItems, EdgeInsets, FlexDirection, FlexGap, Length, Style};
use tuidom_components::{
    Button, ButtonProps, ButtonVariant, Checkbox, CheckboxProps, Divider, DividerProps, Label,
    LabelProps, LabelTone, Orientation, RadioGroup, RadioGroupProps, Select, SelectProps, Slider,
    SliderProps, TextInput, TextInputProps, Theme, install_theme,
};
use tuidom_ui::{App, state, use_document, view};

fn page() -> Style {
    let mut style = Style::new();
    style.width(Length::Percent(100.0));
    style.height(Length::Percent(100.0));
    style.flex_direction(FlexDirection::Column);
    style.padding(EdgeInsets::symmetric(2, 1));
    style.gap(FlexGap::new(1, 0));
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

fn plans() -> Vec<String> {
    ["Free", "Pro", "Team"].map(String::from).to_vec()
}

fn sizes() -> Vec<String> {
    ["Small", "Medium", "Large", "Extra large"]
        .map(String::from)
        .to_vec()
}

fn heading() -> Style {
    let mut style = Style::new();
    style.bold(true);
    style
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    App::new(|| {
        let theme = install_theme(Theme::dark());
        let dark = state(true);
        let last = state(String::from("nothing yet"));
        let name = state(String::new());
        let password = state(String::new());
        let subscribe = state(true);
        let plan = state(0_usize);
        let size = state(1_usize);
        let volume = state(40.0);

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
            box {
                style: page(),
                Label { text: "tuidom-components".to_string(), style: heading() },
                Label {
                    text: "Tab moves focus · Enter presses · t switches theme · q quits".to_string(),
                    tone: LabelTone::Muted,
                },
                Divider {},

                Label { text: "Buttons".to_string(), style: heading() },
                box {
                    style: row(),
                    Button {
                        label: "Save".to_string(),
                        on_press: move |_| last.set("Save".to_string()),
                    },
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
                Divider {},

                Label { text: "Inputs".to_string(), style: heading() },
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
                        TextInput { value: state(String::new()), placeholder: "Disabled".to_string(), disabled: true },
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
                Divider {},

                Label { text: "Labels and dividers".to_string(), style: heading() },
                box {
                    style: row(),
                    Label { text: "Default tone".to_string() },
                    Divider { orientation: Orientation::Vertical },
                    Label { text: "Muted tone".to_string(), tone: LabelTone::Muted },
                },
            }
        }
    })
    .run()
    .await
}
