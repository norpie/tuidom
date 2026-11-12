use std::process::{Command, Stdio};

const FORMAT: &str = "#{pane_tty}\t#{client_termname}\t#{client_termtype}\t#{allow-passthrough}";

/// What tmux knows about the terminal its client is attached from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TmuxClient {
    /// The outer terminal's version string, or its `TERM` when tmux has no version for it.
    pub terminal: Option<String>,
    pub passthrough: bool,
}

/// Whether the application runs in the tmux pane `$TMUX_PANE` names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum TmuxPane {
    Inside(TmuxClient),
    /// `$TMUX` was inherited, as by a terminal started from a shell inside tmux.
    Elsewhere,
    /// tmux could not be asked or answered with something unexpected.
    Unknown,
}

/// Ask the tmux server about `$TMUX_PANE`, comparing its tty with `own_ttys`, the ttys the
/// application's standard streams are attached to.
pub(super) fn query_pane(own_ttys: &[String]) -> TmuxPane {
    let Some(pane) = std::env::var("TMUX_PANE")
        .ok()
        .filter(|pane| !pane.is_empty())
    else {
        return TmuxPane::Unknown;
    };
    let Ok(output) = Command::new("tmux")
        .args(["display-message", "-p", "-t", &pane, FORMAT])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    else {
        return TmuxPane::Unknown;
    };
    // The server named by `$TMUX` is gone or no longer has the pane.
    if !output.status.success() {
        return TmuxPane::Elsewhere;
    }
    pane_from(&String::from_utf8_lossy(&output.stdout), own_ttys)
}

fn pane_from(output: &str, own_ttys: &[String]) -> TmuxPane {
    let Some((tty, client)) = parse_pane(output) else {
        return TmuxPane::Unknown;
    };
    // With no tty of its own to compare, the application is taken to be where `$TMUX` says.
    if own_ttys.is_empty() || own_ttys.contains(&tty) {
        TmuxPane::Inside(client)
    } else {
        TmuxPane::Elsewhere
    }
}

fn parse_pane(output: &str) -> Option<(String, TmuxClient)> {
    let mut fields = output.trim_end_matches('\n').split('\t');
    let (tty, termname, termtype, passthrough) = (
        fields.next()?,
        fields.next()?,
        fields.next()?,
        fields.next()?,
    );
    if fields.next().is_some() || tty.trim().is_empty() {
        return None;
    }
    let terminal = [termtype, termname]
        .into_iter()
        .map(str::trim)
        .find(|field| !field.is_empty())
        .map(str::to_string);
    let client = TmuxClient {
        terminal,
        passthrough: matches!(passthrough.trim(), "on" | "all"),
    };
    Some((tty.trim().to_string(), client))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client(output: &str) -> Option<TmuxClient> {
        parse_pane(output).map(|(_, client)| client)
    }

    #[test]
    fn the_terminal_type_is_preferred_over_the_term_name() {
        assert_eq!(
            parse_pane("/dev/pts/3\talacritty\talacritty 0.13.2\ton\n"),
            Some((
                "/dev/pts/3".to_string(),
                TmuxClient {
                    terminal: Some("alacritty 0.13.2".to_string()),
                    passthrough: true,
                }
            ))
        );
    }

    #[test]
    fn the_term_name_stands_in_when_there_is_no_type() {
        assert_eq!(
            client("/dev/pts/3\txterm-256color\t\toff\n"),
            Some(TmuxClient {
                terminal: Some("xterm-256color".to_string()),
                passthrough: false,
            })
        );
    }

    #[test]
    fn passthrough_all_counts_as_on() {
        assert!(
            client("/dev/pts/3\tkitty\tkitty(0.35.2)\tall")
                .unwrap()
                .passthrough
        );
    }

    #[test]
    fn unexpected_output_is_rejected() {
        assert_eq!(parse_pane(""), None);
        assert_eq!(parse_pane("/dev/pts/3\talacritty\ton"), None);
        assert_eq!(parse_pane("\talacritty\t\ton"), None);
        assert_eq!(parse_pane("a\tb\tc\td\te"), None);
        assert_eq!(
            pane_from("", &["/dev/pts/3".to_string()]),
            TmuxPane::Unknown
        );
    }

    #[test]
    fn the_pane_is_ours_when_its_tty_is_one_of_ours() {
        let output = "/dev/pts/3\tkitty\tkitty(0.35.2)\ton\n";
        let ours = ["/dev/null".to_string(), "/dev/pts/3".to_string()];
        assert_eq!(
            pane_from(output, &ours),
            TmuxPane::Inside(client(output).unwrap())
        );
    }

    #[test]
    fn a_pane_on_another_tty_means_tmux_was_inherited() {
        let output = "/dev/pts/3\talacritty\t\ton\n";
        assert_eq!(
            pane_from(output, &["/dev/pts/9".to_string()]),
            TmuxPane::Elsewhere
        );
    }

    #[test]
    fn without_a_tty_to_compare_the_pane_is_trusted() {
        let output = "/dev/pts/3\talacritty\t\ton\n";
        assert_eq!(
            pane_from(output, &[]),
            TmuxPane::Inside(client(output).unwrap())
        );
    }
}
