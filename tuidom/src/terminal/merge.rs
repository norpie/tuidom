use super::reply::Reply;
use super::{ColorDepth, MultiplexerKind, TerminalInfo, is_truecolor_terminal, speaks_iterm2};

/// Combine what the environment said with the terminal's replies.
///
/// `direct` holds the replies to the batch sent as is, which inside a multiplexer the
/// multiplexer answers. `wrapped` holds the replies to the batch tmux was asked to pass through
/// to the terminal outside it, and is `None` when none was sent.
pub(super) fn merge(
    mut info: TerminalInfo,
    direct: &[Reply],
    wrapped: Option<&[Reply]>,
) -> TerminalInfo {
    info.probed = answered(direct);
    if info.probed && info.multiplexer.is_none() {
        info.graphics.kitty = kitty_ok(direct);
    }

    for reply in direct {
        match reply {
            Reply::DeviceAttributes { sixel } => info.graphics.sixel |= sixel,
            Reply::Version(text) => apply_direct_version(&mut info, text),
            Reply::Foreground(color) => info.foreground = Some(*color),
            Reply::Background(color) => info.background = Some(*color),
            Reply::TrueColor(true) => info.color_depth = ColorDepth::TrueColor,
            Reply::TrueColor(false) | Reply::KittyGraphics(_) => {}
        }
    }

    if let (Some(multiplexer), Some(wrapped)) = (info.multiplexer.as_mut(), wrapped) {
        multiplexer.passthrough = answered(wrapped);
        if multiplexer.passthrough {
            info.graphics.kitty = kitty_ok(wrapped);
        }
        for reply in wrapped {
            if let Reply::Version(text) = reply {
                let (name, version) = split_version(text);
                info.name = Some(name);
                info.version = version;
            }
        }
    }

    // Inside a multiplexer the outer terminal's colors are not what the application gets;
    // the multiplexer's own settings decide that.
    if info.multiplexer.is_none() && info.name.as_deref().is_some_and(is_truecolor_terminal) {
        info.color_depth = ColorDepth::TrueColor;
    }
    info.graphics.iterm2 = info.name.as_deref().is_some_and(speaks_iterm2);
    info
}

fn answered(replies: &[Reply]) -> bool {
    replies
        .iter()
        .any(|reply| matches!(reply, Reply::DeviceAttributes { .. }))
}

fn kitty_ok(replies: &[Reply]) -> bool {
    replies.contains(&Reply::KittyGraphics(true))
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
        assert_eq!(merge(in_tmux(), &[], Some(&[])), in_tmux());
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

    #[test]
    fn tmux_with_passthrough_names_the_outer_terminal() {
        let info = merge(
            in_tmux(),
            &[version("tmux 3.4"), Reply::DeviceAttributes { sixel: true }],
            Some(&[version("kitty(0.35.2)"), Reply::KittyGraphics(true), DA1]),
        );
        let multiplexer = info.multiplexer.as_ref().unwrap();
        assert_eq!(multiplexer.version.as_deref(), Some("3.4"));
        assert!(multiplexer.passthrough);
        assert_eq!(info.name.as_deref(), Some("kitty"));
        assert_eq!(info.version.as_deref(), Some("0.35.2"));
        assert!(info.graphics.kitty);
        assert!(info.graphics.sixel);
        assert_eq!(info.color_depth, ColorDepth::Ansi256);
    }

    #[test]
    fn tmux_with_passthrough_silent_reports_it_off() {
        let info = merge(in_tmux(), &[version("tmux 3.4"), DA1], Some(&[]));
        let multiplexer = info.multiplexer.as_ref().unwrap();
        assert!(!multiplexer.passthrough);
        assert_eq!(multiplexer.version.as_deref(), Some("3.4"));
        assert_eq!(info.name, None);
        assert!(!info.graphics.kitty);
    }

    #[test]
    fn a_version_inside_a_multiplexer_is_kept_only_if_it_names_it() {
        let info = merge(in_tmux(), &[version("kitty(0.35.2)"), DA1], None);
        assert_eq!(info.name, None);
        assert_eq!(info.multiplexer.unwrap().version, None);
    }
}
