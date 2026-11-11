//! A small mail client routed with `tuidom-router`: one shell around every screen, a settings
//! layout nested inside it, an inbox retained while you read, and a message screen that stays
//! mounted as you move between messages. The messages are generated at start.
//!
//! Tab / Shift-Tab, Up / Down — move focus (hovering focuses too)
//! Enter / click — open the focused message or sidebar entry
//! 1 / 2 / 3     — inbox, profile settings, key settings
//! n / p         — next / previous message while reading one
//! Escape        — back
//! q             — quit

use tuidom::event::KeyCode;
use tuidom::style;
use tuidom::style::{Color, Style};
use tuidom_router::{Chain, routed};
use tuidom_ui::{
    App, Children, Signal, View, component, provide_context, state, use_context, use_document, view,
};

const MESSAGES: usize = 30;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Route {
    #[default]
    Inbox,
    Message(usize),
    Settings(Tab),
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Tab {
    #[default]
    Profile,
    Keys,
}

/// The route and the routes that led to it. Back is a pop: the router keeps no history.
#[derive(Clone, Copy)]
struct Nav {
    route: Signal<Route>,
    history: Signal<Vec<Route>>,
}

impl Nav {
    fn new() -> Nav {
        Nav {
            route: state(Route::Inbox),
            history: state(Vec::new()),
        }
    }

    fn go(self, to: Route) {
        let from = self.route.get();
        if from != to {
            self.history.update(|history| history.push(from));
            self.route.set(to);
        }
    }

    fn back(self) {
        let mut previous = None;
        self.history.update(|history| previous = history.pop());
        if let Some(previous) = previous {
            self.route.set(previous);
        }
    }
}

fn nav() -> Nav {
    use_context::<Nav>().expect("the app provides navigation")
}

fn subject(id: usize) -> String {
    format!("Message {id}")
}

fn focus_style() -> Style {
    let mut style = Style::new();
    style.background(Color::white());
    style.color(Color::black());
    style
}

fn link(label: &'static str, test_id: &'static str, to: Route) -> View {
    let nav = nav();
    view! {
        box {
            test_id: test_id,
            focusable: true,
            focus_style: focus_style(),
            on_click: move || nav.go(to),
            on_key_press: move |ev| {
                if ev.code == KeyCode::Enter {
                    nav.go(to);
                }
            },
            text(label),
        }
    }
}

/// The frame around every screen: a sidebar, and the screen beside it.
#[component]
fn Shell(children: Children) -> View {
    view! {
        box {
            style: style! { width: 100%, height: 100%, flex_direction: row },
            box {
                test_id: "sidebar",
                style: style! { width: 12, flex_direction: column, padding: 0 1 },
                text("Mail"),
                { link("Inbox", "to-inbox", Route::Inbox) },
                { link("Settings", "to-settings", Route::Settings(Tab::Profile)) },
            },
            { children },
        }
    }
}

/// Every message, scrollable. Retained, so returning from a message finds it as it was left.
#[component]
fn Inbox() -> View {
    view! {
        box {
            test_id: "inbox",
            style: style! { flex_grow: 1, flex_direction: column, overflow_y: scroll },
            each _, id in 0..MESSAGES {
                { link_to_message(id) }
            }
        }
    }
}

fn link_to_message(id: usize) -> View {
    let nav = nav();
    view! {
        box {
            test_id: format!("message-{id}"),
            focusable: true,
            focus_style: focus_style(),
            on_click: move || nav.go(Route::Message(id)),
            on_key_press: move |ev| {
                if ev.code == KeyCode::Enter {
                    nav.go(Route::Message(id));
                }
            },
            text(subject(id)),
        }
    }
}

/// One message. Moving to another message keeps this component and assigns its `id`.
#[component]
fn Message(id: usize) -> View {
    view! {
        box {
            test_id: "message",
            style: style! { flex_direction: column },
            text(subject(id.get())),
            text("n / p next or previous, Esc back"),
        }
    }
}

/// The settings frame, nested in the shell: a tab row above the tab's screen.
#[component]
fn Settings(tab: Tab, children: Children) -> View {
    let label = move |name: &str, this: Tab| {
        if tab.get() == this {
            format!("[{name}]")
        } else {
            format!(" {name} ")
        }
    };
    view! {
        box {
            test_id: "settings",
            style: style! { flex_grow: 1, flex_direction: column },
            box {
                style: style! { flex_direction: row },
                text(label("Profile", Tab::Profile)),
                text(label("Keys", Tab::Keys)),
            },
            { children },
        }
    }
}

#[component]
fn Profile() -> View {
    let name = state("Ana".to_string());
    view! {
        box {
            style: style! { flex_direction: column },
            text("Name:"),
            input { test_id: "name", bind_value: name },
        }
    }
}

#[component]
fn Keys() -> View {
    view! { text("1 inbox  2 profile  3 keys  Esc back  q quit") }
}

fn chain(route: Route) -> Chain {
    let shell = || ShellProps::builder().build();
    match route {
        Route::Inbox => Chain::layout(shell)
            .leaf(|| InboxProps::builder().build())
            .retain(),
        Route::Message(id) => {
            Chain::layout(shell).leaf(move || MessageProps::builder().id(state(id)).build())
        }
        Route::Settings(tab) => {
            let settings = Chain::layout(shell)
                .layout(move || SettingsProps::builder().tab(state(tab)).build());
            match tab {
                Tab::Profile => settings.leaf(|| ProfileProps::builder().build()),
                Tab::Keys => settings.leaf(|| KeysProps::builder().build()),
            }
        }
    }
}

fn mail(nav: Nav) -> View {
    provide_context(nav);
    let doc = use_document().expect("a component body has a document");
    let keys = doc.clone();
    doc.on_key_press(doc.root(), move |key| {
        if key.edits_input() {
            return;
        }
        let reading = match nav.route.get() {
            Route::Message(id) => Some(id),
            _ => None,
        };
        match (key.code, reading) {
            (KeyCode::Char('1'), _) => nav.go(Route::Inbox),
            (KeyCode::Char('2'), _) => nav.go(Route::Settings(Tab::Profile)),
            (KeyCode::Char('3'), _) => nav.go(Route::Settings(Tab::Keys)),
            (KeyCode::Char('n'), Some(id)) => nav.go(Route::Message((id + 1) % MESSAGES)),
            (KeyCode::Char('p'), Some(id)) => {
                nav.go(Route::Message((id + MESSAGES - 1) % MESSAGES))
            }
            (KeyCode::Esc, _) => nav.back(),
            (KeyCode::Char('q'), _) => keys.quit(),
            _ => {}
        }
    })
    .expect("registering the key handler");
    routed(move || chain(nav.route.get()))
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    App::new(|| mail(Nav::new())).run().await
}

#[cfg(test)]
mod tests {
    use tuidom_ui::testing::Harness;

    use super::*;

    fn harness() -> (Harness, Nav) {
        let nav = Nav::new();
        (Harness::new(60, 10, move || mail(nav)), nav)
    }

    #[test]
    fn the_shell_stays_mounted_across_every_screen() {
        let (mut h, nav) = harness();
        let sidebar = h.find_attr("test_id", "sidebar").unwrap();
        for route in [
            Route::Message(3),
            Route::Settings(Tab::Keys),
            Route::Settings(Tab::Profile),
            Route::Inbox,
        ] {
            nav.go(route);
            assert_eq!(h.find_attr("test_id", "sidebar"), Some(sidebar));
        }
    }

    #[test]
    fn the_inbox_comes_back_scrolled_as_it_was_left() {
        let (mut h, _) = harness();
        let inbox = h.find_attr("test_id", "inbox").unwrap();
        h.document().scroll_to(inbox, 0, 10).unwrap();
        let tenth = h.find_attr("test_id", "message-10").unwrap();
        h.focus(tenth);
        h.press(KeyCode::Enter);
        assert!(h.find_text("Esc back").is_some());

        h.press(KeyCode::Esc);
        assert_eq!(h.find_attr("test_id", "inbox"), Some(inbox));
        assert_eq!(h.document().scroll_offset(inbox).y, 10);
        assert!(h.row(0).contains("Message 10"));
    }

    #[test]
    fn reading_on_keeps_the_message_screen() {
        let (mut h, nav) = harness();
        nav.go(Route::Message(1));
        let message = h.find_attr("test_id", "message").unwrap();
        h.press(KeyCode::Char('n'));
        assert!(h.find_text("Message 2").is_some());
        assert_eq!(h.find_attr("test_id", "message"), Some(message));
    }

    #[test]
    fn escape_walks_back_through_the_routes_visited() {
        let (mut h, nav) = harness();
        h.press(KeyCode::Char('3'));
        nav.go(Route::Message(4));
        h.press(KeyCode::Esc);
        assert!(h.find_text("[Keys]").is_some());
        h.press(KeyCode::Esc);
        assert!(h.find_attr("test_id", "inbox").is_some());
        assert!(h.find_attr("test_id", "settings").is_none());
    }

    #[test]
    fn the_settings_layout_stays_while_its_tabs_change() {
        let (mut h, _) = harness();
        h.press(KeyCode::Char('2'));
        let settings = h.find_attr("test_id", "settings").unwrap();
        assert!(h.find_text("[Profile]").is_some());

        h.press(KeyCode::Char('3'));
        assert!(h.find_text("[Keys]").is_some());
        assert!(h.find_attr("test_id", "name").is_none());
        assert_eq!(h.find_attr("test_id", "settings"), Some(settings));
    }
}
