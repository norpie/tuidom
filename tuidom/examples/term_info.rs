//! Shows what tuidom found out about the terminal it runs in, for checking detection by hand.
//!
//! Each field says where its value came from: "probe" when the terminal's answers changed the
//! environment's guess, "env" when they left it as it was. Below that are the environment
//! variables the guess reads. Resizing re-reads the pixel size.
//!
//! q — quit

use tuidom::event::KeyCode;
use tuidom::style::{Color, ResolvedColor};
use tuidom::terminal::{MultiplexerKind, TerminalInfo};
use tuidom::{Document, NodeId, Result, style};

const ENV_VARS: &[&str] = &[
    "TERM",
    "TERM_PROGRAM",
    "TERM_PROGRAM_VERSION",
    "COLORTERM",
    "NO_COLOR",
    "TMUX",
    "STY",
    "ZELLIJ",
    "SSH_TTY",
    "SSH_CONNECTION",
    "SSH_CLIENT",
    "KITTY_WINDOW_ID",
];

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let doc = Document::new()?;

    let list = doc.create_box()?;
    doc.set_style(list, &style! { flex_direction: column, padding: 1 2 })?;
    doc.append_child(doc.root(), list)?;
    let waiting = doc.create_text("waiting for the probe…")?;
    doc.append_child(list, waiting)?;

    let env = TerminalInfo::from_env();
    let d = doc.clone();
    doc.on_terminal_info(move |event| {
        let _ = show(&d, list, &env, &event.info);
    });

    let d = doc.clone();
    doc.on_key_press(doc.root(), move |key| {
        if key.code == KeyCode::Char('q') {
            d.quit();
        }
    })?;

    doc.run().await?;
    Ok(())
}

fn show(doc: &Document, list: NodeId, env: &TerminalInfo, info: &TerminalInfo) -> Result<()> {
    for child in doc.get_children(list) {
        doc.remove_child(list, child)?;
    }

    let probed = if info.probed {
        "yes"
    } else {
        "no, everything is from env"
    };
    row(doc, list, "terminal answered", probed, None, "")?;

    row(
        doc,
        list,
        "name",
        &optional(info.name.as_deref()),
        None,
        source(&env.name, &info.name),
    )?;
    row(
        doc,
        list,
        "version",
        &optional(info.version.as_deref()),
        None,
        source(&env.version, &info.version),
    )?;

    let multiplexer = info.multiplexer.as_ref().map_or("none".to_string(), |m| {
        let kind = match m.kind {
            MultiplexerKind::Tmux => "tmux",
            MultiplexerKind::Screen => "screen",
            MultiplexerKind::Zellij => "zellij",
        };
        let version = m.version.as_deref().unwrap_or("?");
        let passthrough = if m.passthrough { "on" } else { "off" };
        format!("{kind} {version}, passthrough {passthrough}")
    });
    row(
        doc,
        list,
        "multiplexer",
        &multiplexer,
        None,
        source(&env.multiplexer, &info.multiplexer),
    )?;

    row(
        doc,
        list,
        "color depth",
        &format!("{:?}", info.color_depth),
        None,
        source(&env.color_depth, &info.color_depth),
    )?;
    row(doc, list, "NO_COLOR", yes_no(info.no_color), None, "env")?;
    row(
        doc,
        list,
        "kitty graphics",
        yes_no(info.graphics.kitty),
        None,
        source(&env.graphics.kitty, &info.graphics.kitty),
    )?;
    row(
        doc,
        list,
        "sixel",
        yes_no(info.graphics.sixel),
        None,
        source(&env.graphics.sixel, &info.graphics.sixel),
    )?;
    row(
        doc,
        list,
        "iTerm2 images",
        yes_no(info.graphics.iterm2),
        None,
        source(&env.graphics.iterm2, &info.graphics.iterm2),
    )?;
    color_row(doc, list, "foreground", info.foreground)?;
    color_row(doc, list, "background", info.background)?;

    let pixels = info.pixel_size.map_or("unknown".to_string(), |size| {
        format!(
            "{}x{} px, cell {}x{} px",
            size.width, size.height, size.cell_width, size.cell_height
        )
    });
    row(doc, list, "pixel size", &pixels, None, "window size")?;
    row(doc, list, "over SSH", yes_no(info.remote), None, "env")?;

    let heading = doc.create_text("environment")?;
    doc.set_style(heading, &style! { bold: true, margin: 1 0 0 0 })?;
    doc.append_child(list, heading)?;
    for name in ENV_VARS {
        let value = std::env::var(name).unwrap_or_else(|_| "unset".to_string());
        row(doc, list, name, &value, None, "")?;
    }
    Ok(())
}

fn color_row(
    doc: &Document,
    list: NodeId,
    label: &str,
    color: Option<ResolvedColor>,
) -> Result<()> {
    match color {
        Some(color) => row(
            doc,
            list,
            label,
            &format!("{color:?}"),
            Some(color),
            "probe",
        ),
        None => row(doc, list, label, "not reported", None, ""),
    }
}

fn row(
    doc: &Document,
    list: NodeId,
    label: &str,
    value: &str,
    swatch: Option<ResolvedColor>,
    source: &str,
) -> Result<()> {
    let row = doc.create_box()?;
    doc.set_style(row, &style! { flex_direction: row, gap: 0 2 })?;
    doc.append_child(list, row)?;

    let label = doc.create_text(label)?;
    doc.set_style(label, &style! { width: 22, flex_shrink: 0.0 })?;
    doc.append_child(row, label)?;

    let source = doc.create_text(source)?;
    doc.set_style(
        source,
        &style! { width: 12, flex_shrink: 0.0, italic: true },
    )?;
    doc.append_child(row, source)?;

    if let Some(color) = swatch {
        let block = doc.create_box()?;
        doc.set_style(
            block,
            &style! { width: 4, height: 1, flex_shrink: 0.0, background: (Color::from(color)) },
        )?;
        doc.append_child(row, block)?;
    }

    let value = doc.create_text(value)?;
    doc.append_child(row, value)?;
    Ok(())
}

fn source<T: PartialEq>(env: &T, probed: &T) -> &'static str {
    if env == probed { "env" } else { "probe" }
}

fn optional(value: Option<&str>) -> String {
    value.unwrap_or("unknown").to_string()
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}
