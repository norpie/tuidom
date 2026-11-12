use super::{
    ColorDepth, Graphics, Multiplexer, MultiplexerKind, TerminalInfo, is_truecolor_terminal,
    speaks_iterm2, speaks_kitty, speaks_placeholders,
};

pub(super) fn detect(var: impl Fn(&str) -> Option<String>) -> TerminalInfo {
    let var = |name: &str| var(name).filter(|value| !value.is_empty());
    let term = var("TERM");
    let term_program = var("TERM_PROGRAM");
    let multiplexer = detect_multiplexer(var);

    let (name, version) = if multiplexer.is_some() {
        (None, None)
    } else if let Some(program) = term_program {
        (Some(program), var("TERM_PROGRAM_VERSION"))
    } else if var("KITTY_WINDOW_ID").is_some() || term.as_deref() == Some("xterm-kitty") {
        (Some("kitty".to_string()), None)
    } else {
        (None, None)
    };

    TerminalInfo {
        color_depth: color_depth(
            var("COLORTERM").as_deref(),
            term.as_deref(),
            name.as_deref(),
        ),
        graphics: Graphics {
            kitty: name.as_deref().is_some_and(speaks_kitty),
            placeholders: name.as_deref().is_some_and(speaks_placeholders),
            sixel: false,
            iterm2: name.as_deref().is_some_and(speaks_iterm2),
        },
        name,
        version,
        multiplexer,
        no_color: var("NO_COLOR").is_some(),
        remote: ["SSH_TTY", "SSH_CONNECTION", "SSH_CLIENT"]
            .into_iter()
            .any(|name| var(name).is_some()),
        ..TerminalInfo::default()
    }
}

fn detect_multiplexer(var: impl Fn(&str) -> Option<String>) -> Option<Multiplexer> {
    let kind = if var("TMUX").is_some() {
        MultiplexerKind::Tmux
    } else if var("ZELLIJ").is_some() {
        MultiplexerKind::Zellij
    } else if var("STY").is_some() {
        MultiplexerKind::Screen
    } else {
        return None;
    };
    let version = match kind {
        MultiplexerKind::Tmux if var("TERM_PROGRAM").as_deref() == Some("tmux") => {
            var("TERM_PROGRAM_VERSION")
        }
        _ => None,
    };
    Some(Multiplexer {
        kind,
        version,
        passthrough: false,
    })
}

fn color_depth(colorterm: Option<&str>, term: Option<&str>, name: Option<&str>) -> ColorDepth {
    let truecolor_term =
        term.is_some_and(|term| term.ends_with("-direct") || is_truecolor_terminal(term));
    if matches!(colorterm, Some("truecolor" | "24bit"))
        || truecolor_term
        || name.is_some_and(is_truecolor_terminal)
    {
        ColorDepth::TrueColor
    } else if term.is_some_and(|term| term.contains("256color")) {
        ColorDepth::Ansi256
    } else {
        ColorDepth::Ansi16
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn detect_from(vars: &[(&str, &str)]) -> TerminalInfo {
        let vars: HashMap<String, String> = vars
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect();
        detect(|name| vars.get(name).cloned())
    }

    #[test]
    fn an_empty_environment_says_nothing_beyond_16_colors() {
        assert_eq!(detect_from(&[]), TerminalInfo::default());
    }

    #[test]
    fn colorterm_reports_truecolor() {
        for value in ["truecolor", "24bit"] {
            let info = detect_from(&[("TERM", "xterm"), ("COLORTERM", value)]);
            assert_eq!(info.color_depth, ColorDepth::TrueColor);
        }
    }

    #[test]
    fn a_256color_term_reports_256_colors() {
        let info = detect_from(&[("TERM", "xterm-256color")]);
        assert_eq!(info.color_depth, ColorDepth::Ansi256);
    }

    #[test]
    fn a_direct_color_term_reports_truecolor() {
        let info = detect_from(&[("TERM", "xterm-direct")]);
        assert_eq!(info.color_depth, ColorDepth::TrueColor);
    }

    #[test]
    fn term_program_names_the_terminal() {
        let info = detect_from(&[
            ("TERM", "xterm-256color"),
            ("TERM_PROGRAM", "WezTerm"),
            ("TERM_PROGRAM_VERSION", "20240203"),
        ]);
        assert_eq!(info.name.as_deref(), Some("WezTerm"));
        assert_eq!(info.version.as_deref(), Some("20240203"));
        assert_eq!(info.color_depth, ColorDepth::TrueColor);
        assert!(info.graphics.iterm2);
        assert!(info.multiplexer.is_none());
    }

    #[test]
    fn kitty_is_known_by_its_window_id_or_term() {
        for vars in [
            &[("KITTY_WINDOW_ID", "1")][..],
            &[("TERM", "xterm-kitty")][..],
        ] {
            let info = detect_from(vars);
            assert_eq!(info.name.as_deref(), Some("kitty"));
            assert!(info.graphics.kitty);
            assert!(info.graphics.placeholders);
            assert_eq!(info.color_depth, ColorDepth::TrueColor);
        }
    }

    #[test]
    fn tmux_is_detected_with_its_version() {
        let info = detect_from(&[
            ("TMUX", "/tmp/tmux-1000/default,1234,0"),
            ("TERM", "tmux-256color"),
            ("TERM_PROGRAM", "tmux"),
            ("TERM_PROGRAM_VERSION", "3.4"),
        ]);
        assert_eq!(
            info.multiplexer,
            Some(Multiplexer {
                kind: MultiplexerKind::Tmux,
                version: Some("3.4".to_string()),
                passthrough: false,
            })
        );
        assert_eq!(info.name, None);
        assert_eq!(info.color_depth, ColorDepth::Ansi256);
    }

    #[test]
    fn inside_a_multiplexer_the_inherited_terminal_is_not_trusted() {
        let info = detect_from(&[
            ("STY", "1234.pts-0.host"),
            ("TERM", "screen-256color"),
            ("TERM_PROGRAM", "WezTerm"),
            ("TERM_PROGRAM_VERSION", "20240203"),
            ("KITTY_WINDOW_ID", "1"),
        ]);
        assert_eq!(
            info.multiplexer.map(|multiplexer| multiplexer.kind),
            Some(MultiplexerKind::Screen)
        );
        assert_eq!(info.name, None);
        assert_eq!(info.version, None);
        assert_eq!(info.graphics, Graphics::default());
    }

    #[test]
    fn zellij_is_detected() {
        let info = detect_from(&[("ZELLIJ", "0")]);
        assert_eq!(
            info.multiplexer.map(|multiplexer| multiplexer.kind),
            Some(MultiplexerKind::Zellij)
        );
    }

    #[test]
    fn ssh_marks_the_session_remote() {
        for name in ["SSH_TTY", "SSH_CONNECTION", "SSH_CLIENT"] {
            assert!(detect_from(&[(name, "x")]).remote, "{name}");
        }
    }

    #[test]
    fn no_color_counts_only_when_non_empty() {
        assert!(detect_from(&[("NO_COLOR", "1")]).no_color);
        assert!(!detect_from(&[("NO_COLOR", "")]).no_color);
    }
}
