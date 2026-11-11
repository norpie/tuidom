use super::reply::Reply;
use super::tmux::TmuxClient;
use super::{
    ColorDepth, MultiplexerKind, TerminalInfo, is_truecolor_terminal, speaks_iterm2, speaks_kitty,
};

/// Combine what the environment said with what tmux and the terminal answered.
///
/// Inside a multiplexer, the multiplexer is what answered the queries, not the terminal outside it.
pub(super) fn merge(
    mut info: TerminalInfo,
    replies: &[Reply],
    tmux: Option<&TmuxClient>,
) -> TerminalInfo {
    info.probed = answered(replies);
    if info.probed && info.multiplexer.is_none() {
        info.graphics.kitty = replies.contains(&Reply::KittyGraphics(true));
    }

    for reply in replies {
        match reply {
            Reply::DeviceAttributes { sixel } => info.graphics.sixel |= sixel,
            Reply::Version(text) => apply_direct_version(&mut info, text),
            Reply::Foreground(color) => info.foreground = Some(*color),
            Reply::Background(color) => info.background = Some(*color),
            Reply::TrueColor(true) => info.color_depth = ColorDepth::TrueColor,
            Reply::TrueColor(false) | Reply::KittyGraphics(_) => {}
        }
    }

    if let (Some(multiplexer), Some(client)) = (info.multiplexer.as_mut(), tmux) {
        multiplexer.passthrough = client.passthrough;
        if let Some(terminal) = &client.terminal {
            let (name, version) = split_version(terminal);
            info.name = Some(name);
            info.version = version;
        }
    }

    // Inside a multiplexer the outer terminal's colors are not what the application gets;
    // the multiplexer's own settings decide that.
    if info.multiplexer.is_none() && info.name.as_deref().is_some_and(is_truecolor_terminal) {
        info.color_depth = ColorDepth::TrueColor;
    }
    // An image protocol is only usable through a multiplexer that passes it on. Inside one,
    // kitty graphics go by name too: a query forwarded through tmux is answered to tmux, which
    // keeps the reply.
    let reaches_terminal = info
        .multiplexer
        .as_ref()
        .is_none_or(|multiplexer| multiplexer.passthrough);
    let name = info.name.as_deref();
    if info.multiplexer.is_some() {
        info.graphics.kitty = reaches_terminal && name.is_some_and(speaks_kitty);
    }
    info.graphics.iterm2 = reaches_terminal && name.is_some_and(speaks_iterm2);
    info
}

fn answered(replies: &[Reply]) -> bool {
    replies
        .iter()
        .any(|reply| matches!(reply, Reply::DeviceAttributes { .. }))
}

/// Inside a multiplexer, the version reply could come from the multiplexer or from a terminal
/// it forwarded the query to, so it is kept only when it names the multiplexer.
fn apply_direct_version(info: &mut TerminalInfo, text: &str) {
    let (name, version) = split_version(text);
    match info.multiplexer.as_mut() {
        None => {
            info.name = Some(name);
            info.version = version;
        }
        Some(multiplexer) => {
            let own_name = match multiplexer.kind {
                MultiplexerKind::Tmux => "tmux",
                MultiplexerKind::Screen => "screen",
                MultiplexerKind::Zellij => "zellij",
            };
            if name.eq_ignore_ascii_case(own_name) {
                multiplexer.version = version;
            }
        }
    }
}

/// Split an XTVERSION string such as `kitty(0.35.2)` or `tmux 3.4` into name and version.
fn split_version(text: &str) -> (String, Option<String>) {
    let Some(at) = text.find(['(', ' ']) else {
        return (text.to_string(), None);
    };
    let version = text[at + 1..].trim_end_matches(')').trim();
    (
        text[..at].to_string(),
        (!version.is_empty()).then(|| version.to_string()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::ResolvedColor;
    use crate::terminal::{Graphics, Multiplexer};

    const DA1: Reply = Reply::DeviceAttributes { sixel: false };

    fn version(text: &str) -> Reply {
        Reply::Version(text.to_string())
    }

    fn in_tmux() -> TerminalInfo {
        TerminalInfo {
            multiplexer: Some(Multiplexer {
                kind: MultiplexerKind::Tmux,
                version: None,
                passthrough: false,
            }),
            color_depth: ColorDepth::Ansi256,
            ..TerminalInfo::default()
        }
    }

    #[test]
    fn no_answers_leave_the_environment_as_it_was() {
        let env = TerminalInfo {
            name: Some("kitty".to_string()),
            color_depth: ColorDepth::TrueColor,
            graphics: Graphics {
                kitty: true,
                ..Graphics::default()
            },
            ..TerminalInfo::default()
        };
        assert_eq!(merge(env.clone(), &[], None), env);
        assert_eq!(merge(in_tmux(), &[], None), in_tmux());
    }

    #[test]
    fn a_terminal_answering_only_da1_is_probed_and_has_no_kitty_graphics() {
        let env = TerminalInfo {
            name: Some("kitty".to_string()),
            graphics: Graphics {
                kitty: true,
                ..Graphics::default()
            },
            ..TerminalInfo::default()
        };
        let info = merge(env, &[DA1], None);
        assert!(info.probed);
        assert!(!info.graphics.kitty);
        assert_eq!(info.name.as_deref(), Some("kitty"));
    }

    #[test]
    fn xtversion_replaces_the_environment_name() {
        let env = TerminalInfo {
            name: Some("vscode".to_string()),
            version: Some("1.95".to_string()),
            ..TerminalInfo::default()
        };
        let info = merge(env, &[version("WezTerm 20240203-110809"), DA1], None);
        assert_eq!(info.name.as_deref(), Some("WezTerm"));
        assert_eq!(info.version.as_deref(), Some("20240203-110809"));
        assert_eq!(info.color_depth, ColorDepth::TrueColor);
        assert!(info.graphics.iterm2);
    }

    #[test]
    fn version_strings_split_at_a_paren_or_space() {
        assert_eq!(
            split_version("kitty(0.35.2)"),
            ("kitty".to_string(), Some("0.35.2".to_string()))
        );
        assert_eq!(
            split_version("tmux 3.4"),
            ("tmux".to_string(), Some("3.4".to_string()))
        );
        assert_eq!(split_version("st"), ("st".to_string(), None));
        assert_eq!(split_version("foot()"), ("foot".to_string(), None));
    }

    #[test]
    fn replies_fill_colors_sixel_kitty_and_truecolor() {
        let background = ResolvedColor::from_srgb(30, 30, 46);
        let foreground = ResolvedColor::from_srgb(205, 214, 244);
        let info = merge(
            TerminalInfo::default(),
            &[
                Reply::Foreground(foreground),
                Reply::Background(background),
                Reply::KittyGraphics(true),
                Reply::TrueColor(true),
                Reply::DeviceAttributes { sixel: true },
            ],
            None,
        );
        assert_eq!(info.foreground, Some(foreground));
        assert_eq!(info.background, Some(background));
        assert!(info.graphics.kitty);
        assert!(info.graphics.sixel);
        assert_eq!(info.color_depth, ColorDepth::TrueColor);
    }

    #[test]
    fn a_negative_truecolor_answer_does_not_downgrade() {
        let env = TerminalInfo {
            color_depth: ColorDepth::TrueColor,
            ..TerminalInfo::default()
        };
        let info = merge(env, &[Reply::TrueColor(false), DA1], None);
        assert_eq!(info.color_depth, ColorDepth::TrueColor);
    }

    fn client(terminal: &str, passthrough: bool) -> TmuxClient {
        TmuxClient {
            terminal: Some(terminal.to_string()),
            passthrough,
        }
    }

    #[test]
    fn tmux_names_the_outer_terminal() {
        let info = merge(
            in_tmux(),
            &[version("tmux 3.4"), Reply::DeviceAttributes { sixel: true }],
            Some(&client("WezTerm 20240203", true)),
        );
        let multiplexer = info.multiplexer.as_ref().unwrap();
        assert_eq!(multiplexer.version.as_deref(), Some("3.4"));
        assert!(multiplexer.passthrough);
        assert_eq!(info.name.as_deref(), Some("WezTerm"));
        assert_eq!(info.version.as_deref(), Some("20240203"));
        assert!(info.graphics.sixel);
        assert_eq!(info.color_depth, ColorDepth::Ansi256);
    }

    #[test]
    fn inside_tmux_kitty_graphics_go_by_name_and_need_passthrough() {
        for terminal in ["kitty(0.35.2)", "ghostty 1.1.0", "xterm-kitty"] {
            let on = merge(in_tmux(), &[DA1], Some(&client(terminal, true)));
            assert!(on.graphics.kitty, "{terminal}");
            let off = merge(in_tmux(), &[DA1], Some(&client(terminal, false)));
            assert!(!off.graphics.kitty, "{terminal}");
        }
        let alacritty = merge(in_tmux(), &[DA1], Some(&client("alacritty", true)));
        assert!(!alacritty.graphics.kitty);
    }

    #[test]
    fn inside_tmux_a_kitty_reply_is_tmux_speaking_and_ignored() {
        let info = merge(
            in_tmux(),
            &[Reply::KittyGraphics(true), DA1],
            Some(&client("alacritty", true)),
        );
        assert!(!info.graphics.kitty);
    }

    #[test]
    fn a_version_inside_a_multiplexer_is_kept_only_if_it_names_it() {
        let info = merge(in_tmux(), &[version("kitty(0.35.2)"), DA1], None);
        assert_eq!(info.name, None);
        assert_eq!(info.multiplexer.unwrap().version, None);
    }
}
