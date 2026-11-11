use std::io;
use std::time::{Duration, Instant};

use super::merge::merge;
use super::reply::{self, Reply};
use super::tmux::{self, TmuxClient};
use super::{MultiplexerKind, PixelSize, TerminalInfo};

/// How long one batch may take to answer. A terminal answers DA1 well within it, so it only
/// runs out on a terminal that ignores DA1 or a link too slow to wait for.
const BATCH_DEADLINE: Duration = Duration::from_millis(300);

/// The terminal's input and output, as the probe needs them.
pub(super) trait ProbeIo {
    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()>;
    /// Read what has arrived, waiting no later than `deadline`. `Ok(0)` means nothing came.
    fn read_until(&mut self, buf: &mut [u8], deadline: Instant) -> io::Result<usize>;
}

/// Find out what the terminal is. Needs raw mode on and nothing else reading the terminal.
///
/// Never fails: anything that goes wrong leaves the fields it would have filled to the
/// environment.
pub(crate) async fn probe() -> TerminalInfo {
    match tokio::task::spawn_blocking(probe_blocking).await {
        Ok(info) => info,
        Err(err) => {
            tracing::warn!(%err, "terminal probe did not finish");
            TerminalInfo::from_env()
        }
    }
}

fn probe_blocking() -> TerminalInfo {
    let env = TerminalInfo::from_env();
    let tmux = env
        .multiplexer
        .as_ref()
        .filter(|multiplexer| multiplexer.kind == MultiplexerKind::Tmux)
        .and_then(|_| tmux::query_client());
    let mut info = match ask_terminal(env.clone(), tmux.as_ref()) {
        Ok(info) => info,
        Err(err) => {
            tracing::warn!(%err, "terminal did not take queries");
            merge(env, &[], tmux.as_ref(), None)
        }
    };
    info.pixel_size = pixel_size();
    tracing::debug!(?info, "terminal probed");
    info
}

#[cfg(unix)]
fn ask_terminal(env: TerminalInfo, tmux: Option<&TmuxClient>) -> io::Result<TerminalInfo> {
    let mut io = super::tty::TtyIo::open()?;
    probe_with(&mut io, env, tmux)
}

#[cfg(not(unix))]
fn ask_terminal(env: TerminalInfo, tmux: Option<&TmuxClient>) -> io::Result<TerminalInfo> {
    Ok(merge(env, &[], tmux, None))
}

pub(crate) fn pixel_size() -> Option<PixelSize> {
    let size = crossterm::terminal::window_size().ok()?;
    if size.width == 0 || size.height == 0 || size.columns == 0 || size.rows == 0 {
        return None;
    }
    Some(PixelSize {
        width: size.width,
        height: size.height,
        cell_width: size.width / size.columns,
        cell_height: size.height / size.rows,
    })
}

fn probe_with(
    io: &mut impl ProbeIo,
    env: TerminalInfo,
    tmux: Option<&TmuxClient>,
) -> io::Result<TerminalInfo> {
    let direct = run_batch(io, &reply::query_batch())?;
    let direct_answered = direct
        .iter()
        .any(|reply| matches!(reply, Reply::DeviceAttributes { .. }));
    // Sent only after the direct batch is answered, since replies carry nothing that says
    // which batch asked.
    let wrapped = match tmux {
        Some(client) if client.passthrough && direct_answered => Some(run_batch(
            io,
            &reply::tmux_passthrough(&reply::outer_batch()),
        )?),
        _ => None,
    };
    Ok(merge(env, &direct, tmux, wrapped.as_deref()))
}

/// Send `batch` and collect replies until its DA1 reply arrives or the deadline passes.
fn run_batch(io: &mut impl ProbeIo, batch: &[u8]) -> io::Result<Vec<Reply>> {
    let deadline = Instant::now() + BATCH_DEADLINE;
    io.write_all(batch)?;
    let mut pending = Vec::new();
    let mut replies = Vec::new();
    let mut chunk = [0; 1024];
    loop {
        let read = io.read_until(&mut chunk, deadline)?;
        if read == 0 {
            return Ok(replies);
        }
        pending.extend_from_slice(&chunk[..read]);
        let (parsed, used) = reply::parse(&pending);
        pending.drain(..used);
        let done = parsed
            .iter()
            .any(|reply| matches!(reply, Reply::DeviceAttributes { .. }));
        replies.extend(parsed);
        if done {
            return Ok(replies);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;
    use crate::style::ResolvedColor;
    use crate::terminal::{ColorDepth, Multiplexer};

    /// Answers each write with the chunks scripted for it, then reports nothing more, as a
    /// deadline running out would.
    struct Scripted {
        answers: VecDeque<Vec<&'static [u8]>>,
        unread: VecDeque<&'static [u8]>,
        written: Vec<Vec<u8>>,
    }

    impl Scripted {
        fn new(answers: Vec<Vec<&'static [u8]>>) -> Self {
            Self {
                answers: answers.into(),
                unread: VecDeque::new(),
                written: Vec::new(),
            }
        }
    }

    impl ProbeIo for Scripted {
        fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
            self.written.push(bytes.to_vec());
            self.unread
                .extend(self.answers.pop_front().unwrap_or_default());
            Ok(())
        }

        fn read_until(&mut self, buf: &mut [u8], _deadline: Instant) -> io::Result<usize> {
            let Some(chunk) = self.unread.pop_front() else {
                return Ok(0);
            };
            buf[..chunk.len()].copy_from_slice(chunk);
            Ok(chunk.len())
        }
    }

    fn in_tmux() -> TerminalInfo {
        TerminalInfo {
            multiplexer: Some(Multiplexer {
                kind: MultiplexerKind::Tmux,
                version: None,
                passthrough: false,
            }),
            ..TerminalInfo::default()
        }
    }

    fn client(passthrough: bool) -> TmuxClient {
        TmuxClient {
            terminal: Some("kitty(0.35.2)".to_string()),
            passthrough,
        }
    }

    #[test]
    fn replies_split_across_reads_are_collected() {
        let mut io = Scripted::new(vec![vec![
            b"\x1bP>|foot(1.1",
            b"7.2)\x1b\\\x1b]11;rgb:1e1e/1e1e/2e2e\x1b",
            b"\\\x1b[?62;4c",
        ]]);
        let info = probe_with(&mut io, TerminalInfo::default(), None).unwrap();
        assert!(info.probed);
        assert_eq!(info.name.as_deref(), Some("foot"));
        assert_eq!(info.version.as_deref(), Some("1.17.2"));
        assert_eq!(
            info.background,
            Some(ResolvedColor::from_srgb(0x1e, 0x1e, 0x2e))
        );
        assert!(info.graphics.sixel);
        assert_eq!(info.color_depth, ColorDepth::TrueColor);
        assert_eq!(io.written, [reply::query_batch()]);
    }

    #[test]
    fn a_terminal_that_never_sends_da1_leaves_the_environment() {
        let env = TerminalInfo {
            color_depth: ColorDepth::Ansi256,
            ..TerminalInfo::default()
        };
        let mut io = Scripted::new(vec![vec![b"\x1b]11;rgb:0/0/0\x1b\\"]]);
        let info = probe_with(&mut io, env, None).unwrap();
        assert!(!info.probed);
        assert_eq!(info.color_depth, ColorDepth::Ansi256);
    }

    #[test]
    fn reading_stops_at_da1() {
        let mut io = Scripted::new(vec![vec![b"\x1b[?62c", b"typed after"]]);
        probe_with(&mut io, TerminalInfo::default(), None).unwrap();
        assert_eq!(io.unread, [b"typed after"]);
    }

    #[test]
    fn with_passthrough_on_the_outer_batch_follows_the_direct_answer() {
        let mut io = Scripted::new(vec![
            vec![b"\x1bP>|tmux 3.4\x1b\\\x1b[?62c"],
            vec![b"\x1b_Gi=31;OK\x1b\\\x1b[?62;22c"],
        ]);
        let info = probe_with(&mut io, in_tmux(), Some(&client(true))).unwrap();
        assert_eq!(
            io.written,
            [
                reply::query_batch(),
                reply::tmux_passthrough(&reply::outer_batch())
            ]
        );
        assert!(info.graphics.kitty);
        assert_eq!(info.name.as_deref(), Some("kitty"));
        assert_eq!(info.multiplexer.unwrap().version.as_deref(), Some("3.4"));
    }

    #[test]
    fn with_passthrough_off_only_tmux_is_asked() {
        let mut io = Scripted::new(vec![vec![b"\x1b[?62c"]]);
        let info = probe_with(&mut io, in_tmux(), Some(&client(false))).unwrap();
        assert_eq!(io.written, [reply::query_batch()]);
        assert!(!info.graphics.kitty);
    }

    #[test]
    fn an_unanswered_direct_batch_sends_nothing_through_tmux() {
        let mut io = Scripted::new(vec![vec![]]);
        probe_with(&mut io, in_tmux(), Some(&client(true))).unwrap();
        assert_eq!(io.written, [reply::query_batch()]);
    }
}
