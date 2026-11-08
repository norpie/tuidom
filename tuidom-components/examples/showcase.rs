//! Every component in `tuidom-components`, one at a time, under a switchable theme.
//!
//! Tab / Shift-Tab   — move focus between the buttons (hovering focuses too)
//! Enter / Space     — press the focused button; the status line names it
//! t                 — switch between the dark and light themes
//! q                 — quit

use tuidom::event::KeyCode;
use tuidom::style::{EdgeInsets, FlexDirection, FlexGap, Length, Style};
use tuidom_components::{
    Button, ButtonProps, ButtonVariant, Divider, DividerProps, Label, LabelProps, LabelTone,
    Orientation, Theme, install_theme,
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

        let doc = use_document().expect("an App body has a document");
        let quit = doc.clone();
        doc.on_key_press(doc.root(), move |key| match key.code {
            KeyCode::Char('t') => {
                let next = !dark.get();
                dark.set(next);
                theme.set(if next { Theme::dark() } else { Theme::light() });
            }
            KeyCode::Char('q') => quit.quit(),
            _ => {}
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
