use std::process::{Command, Stdio};

const FORMAT: &str = "#{client_termname}\t#{client_termtype}\t#{allow-passthrough}";

/// What tmux knows about the terminal its client is attached from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TmuxClient {
    /// The outer terminal's version string, or its `TERM` when tmux has no version for it.
    pub terminal: Option<String>,
    pub passthrough: bool,
}

/// Ask the tmux server the application runs under. `None` if tmux cannot be run or answers
/// with something unexpected.
pub(super) fn query_client() -> Option<TmuxClient> {
    let output = Command::new("tmux")
        .args(["display-message", "-p", FORMAT])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_client(&String::from_utf8_lossy(&output.stdout))
}

fn parse_client(output: &str) -> Option<TmuxClient> {
    let mut fields = output.trim_end_matches('\n').split('\t');
    let (termname, termtype, passthrough) = (fields.next()?, fields.next()?, fields.next()?);
    if fields.next().is_some() {
        return None;
    }
    let terminal = [termtype, termname]
        .into_iter()
        .map(str::trim)
        .find(|field| !field.is_empty())
        .map(str::to_string);
    Some(TmuxClient {
        terminal,
        passthrough: matches!(passthrough.trim(), "on" | "all"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_terminal_type_is_preferred_over_the_term_name() {
        assert_eq!(
            parse_client("alacritty\talacritty 0.13.2\ton\n"),
            Some(TmuxClient {
                terminal: Some("alacritty 0.13.2".to_string()),
                passthrough: true,
            })
        );
    }

    #[test]
    fn the_term_name_stands_in_when_there_is_no_type() {
        assert_eq!(
            parse_client("xterm-256color\t\toff\n"),
            Some(TmuxClient {
                terminal: Some("xterm-256color".to_string()),
                passthrough: false,
            })
        );
    }

    #[test]
    fn passthrough_all_counts_as_on() {
        assert!(
            parse_client("kitty\tkitty(0.35.2)\tall")
                .unwrap()
                .passthrough
        );
    }

    #[test]
    fn unexpected_output_is_rejected() {
        assert_eq!(parse_client(""), None);
        assert_eq!(parse_client("alacritty\ton"), None);
        assert_eq!(parse_client("a\tb\tc\td"), None);
    }
}
