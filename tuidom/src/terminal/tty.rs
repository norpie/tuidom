use std::fs::File;
use std::io::{self, Stdout, Write};
use std::os::fd::{AsFd, BorrowedFd};
use std::time::Instant;

use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::io::Errno;

use super::probe::ProbeIo;

/// The terminals the standard streams are attached to, by device path.
pub(super) fn own_ttys() -> Vec<String> {
    [
        rustix::stdio::stdin(),
        rustix::stdio::stdout(),
        rustix::stdio::stderr(),
    ]
    .into_iter()
    .filter_map(|fd| rustix::termios::ttyname(fd, Vec::new()).ok())
    .filter_map(|name| name.into_string().ok())
    .collect()
}

/// The terminal crossterm talks to: replies are read from stdin when it is a terminal and from
/// `/dev/tty` otherwise, the same choice crossterm makes, and queries go to stdout.
pub(super) struct TtyIo {
    input: Input,
    output: Stdout,
}

enum Input {
    Stdin(BorrowedFd<'static>),
    DevTty(File),
}

impl TtyIo {
    pub fn open() -> io::Result<Self> {
        let stdin = rustix::stdio::stdin();
        let input = if rustix::termios::isatty(stdin) {
            Input::Stdin(stdin)
        } else {
            Input::DevTty(File::options().read(true).write(true).open("/dev/tty")?)
        };
        Ok(Self {
            input,
            output: io::stdout(),
        })
    }

    fn input(&self) -> BorrowedFd<'_> {
        match &self.input {
            Input::Stdin(fd) => *fd,
            Input::DevTty(file) => file.as_fd(),
        }
    }
}

impl ProbeIo for TtyIo {
    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.output.write_all(bytes)?;
        self.output.flush()
    }

    fn read_until(&mut self, buf: &mut [u8], deadline: Instant) -> io::Result<usize> {
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(0);
            }
            let timeout = Timespec::try_from(remaining).map_err(io::Error::other)?;
            let input = self.input();
            let mut fds = [PollFd::new(&input, PollFlags::IN)];
            match poll(&mut fds, Some(&timeout)) {
                Ok(0) => return Ok(0),
                Ok(_) => match rustix::io::read(input, &mut *buf) {
                    Err(Errno::INTR | Errno::AGAIN) => continue,
                    result => return Ok(result?),
                },
                Err(Errno::INTR) => continue,
                Err(err) => return Err(err.into()),
            }
        }
    }
}
