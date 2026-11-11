use crate::style::ResolvedColor;

const ESC: u8 = 0x1b;
const BEL: u8 = 0x07;

/// The image id in the kitty graphics query. Any id works; the reply echoes it.
const KITTY_QUERY_ID: &str = "31";

/// A terminal reply the probe understands.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Reply {
    /// Primary device attributes, the reply every terminal sends and so the end of a batch.
    DeviceAttributes {
        sixel: bool,
    },
    /// The XTVERSION string, such as `kitty(0.35.2)` or `tmux 3.4`.
    Version(String),
    Foreground(ResolvedColor),
    Background(ResolvedColor),
    KittyGraphics(bool),
    /// An XTGETTCAP answer for `RGB` or `Tc`, either of which means 24-bit color.
    TrueColor(bool),
}

/// The queries the probe sends, ending with the DA1 request whose reply marks the end.
pub(super) fn query_batch() -> Vec<u8> {
    let mut batch = Vec::new();
    batch.extend_from_slice(b"\x1b[>0q");
    batch.extend_from_slice(b"\x1b]10;?\x1b\\");
    batch.extend_from_slice(b"\x1b]11;?\x1b\\");
    batch.extend_from_slice(b"\x1b_Gi=");
    batch.extend_from_slice(KITTY_QUERY_ID.as_bytes());
    batch.extend_from_slice(b",s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\");
    for capability in ["RGB", "Tc"] {
        batch.extend_from_slice(b"\x1bP+q");
        batch.extend_from_slice(hex_encode(capability).as_bytes());
        batch.extend_from_slice(b"\x1b\\");
    }
    batch.extend_from_slice(b"\x1b[c");
    batch
}

/// Parse the replies in `buf`, returning them and how many bytes were used.
///
/// A sequence cut off at the end of `buf` is left unused, so the caller keeps those bytes
/// and parses them again once the rest arrives. Everything else, recognized or not, is used.
pub(super) fn parse(buf: &[u8]) -> (Vec<Reply>, usize) {
    let mut replies = Vec::new();
    let mut at = 0;
    while let Some(offset) = buf[at..].iter().position(|&byte| byte == ESC) {
        let start = at + offset;
        let Some(sequence) = split_sequence(&buf[start..]) else {
            return (replies, start);
        };
        let (len, reply) = match sequence {
            Sequence::Csi {
                params,
                final_byte,
                len,
            } => (len, parse_csi(params, final_byte)),
            Sequence::Dcs { body, len } => (len, parse_dcs(body)),
            Sequence::Osc { body, len } => (len, parse_osc(body)),
            Sequence::Apc { body, len } => (len, parse_apc(body)),
            Sequence::Other => (1, None),
        };
        replies.extend(reply);
        at = start + len;
    }
    (replies, buf.len())
}

enum Sequence<'a> {
    Csi {
        params: &'a [u8],
        final_byte: u8,
        len: usize,
    },
    Dcs {
        body: &'a [u8],
        len: usize,
    },
    Osc {
        body: &'a [u8],
        len: usize,
    },
    Apc {
        body: &'a [u8],
        len: usize,
    },
    /// An ESC that starts nothing the probe asked for, such as alt plus a typed key.
    Other,
}

/// Split one escape sequence off the front of `buf`, which starts with ESC. `None` means it is
/// not complete yet.
fn split_sequence(buf: &[u8]) -> Option<Sequence<'_>> {
    let introducer = *buf.get(1)?;
    if introducer == b'[' {
        let final_at = 2 + buf[2..]
            .iter()
            .position(|byte| (0x40..=0x7e).contains(byte))?;
        return Some(Sequence::Csi {
            params: &buf[2..final_at],
            final_byte: buf[final_at],
            len: final_at + 1,
        });
    }
    if !matches!(introducer, b'P' | b']' | b'_') {
        return Some(Sequence::Other);
    }
    let (body_end, len) = find_string_end(buf, 2)?;
    let body = &buf[2..body_end];
    Some(match introducer {
        b'P' => Sequence::Dcs { body, len },
        b']' => Sequence::Osc { body, len },
        _ => Sequence::Apc { body, len },
    })
}

/// Find the terminator of a string sequence whose body starts at `from`: ST (`ESC \`) or BEL.
/// Returns where the body ends and where the sequence does.
fn find_string_end(buf: &[u8], from: usize) -> Option<(usize, usize)> {
    let mut at = from;
    while at < buf.len() {
        match buf[at] {
            BEL => return Some((at, at + 1)),
            ESC if buf.get(at + 1)? == &b'\\' => return Some((at, at + 2)),
            _ => at += 1,
        }
    }
    None
}

fn parse_csi(params: &[u8], final_byte: u8) -> Option<Reply> {
    let attributes = params.strip_prefix(b"?").filter(|_| final_byte == b'c')?;
    let sixel = attributes
        .split(|&byte| byte == b';')
        .any(|attribute| attribute == b"4");
    Some(Reply::DeviceAttributes { sixel })
}

fn parse_dcs(body: &[u8]) -> Option<Reply> {
    if let Some(version) = body.strip_prefix(b">|") {
        return Some(Reply::Version(
            String::from_utf8_lossy(version).trim().to_string(),
        ));
    }
    let (found, rest) = if let Some(rest) = body.strip_prefix(b"1+r") {
        (true, rest)
    } else {
        (false, body.strip_prefix(b"0+r")?)
    };
    let name = rest.split(|&byte| byte == b'=').next()?;
    let name = hex_decode(name)?;
    matches!(name.as_str(), "RGB" | "Tc").then_some(Reply::TrueColor(found))
}

fn parse_osc(body: &[u8]) -> Option<Reply> {
    let body = std::str::from_utf8(body).ok()?;
    let (code, value) = body.split_once(';')?;
    let color = parse_rgb_spec(value)?;
    match code {
        "10" => Some(Reply::Foreground(color)),
        "11" => Some(Reply::Background(color)),
        _ => None,
    }
}

fn parse_apc(body: &[u8]) -> Option<Reply> {
    let body = std::str::from_utf8(body.strip_prefix(b"G")?).ok()?;
    let (control, message) = body.split_once(';')?;
    let id = control
        .split(',')
        .find_map(|pair| pair.strip_prefix("i="))?;
    (id == KITTY_QUERY_ID).then(|| Reply::KittyGraphics(message == "OK"))
}

/// Parse an X11 color spec, `rgb:R/G/B` with one to four hex digits per channel.
fn parse_rgb_spec(spec: &str) -> Option<ResolvedColor> {
    let mut channels = spec.strip_prefix("rgb:")?.split('/').map(scale_channel);
    let color = ResolvedColor::from_srgb(channels.next()??, channels.next()??, channels.next()??);
    channels.next().is_none().then_some(color)
}

fn scale_channel(hex: &str) -> Option<u8> {
    if hex.is_empty() || hex.len() > 4 {
        return None;
    }
    let value = u32::from_str_radix(hex, 16).ok()?;
    let max = (1u32 << (4 * hex.len())) - 1;
    u8::try_from((value * 255 + max / 2) / max).ok()
}

fn hex_encode(text: &str) -> String {
    text.bytes().map(|byte| format!("{byte:02X}")).collect()
}

fn hex_decode(hex: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(hex).ok()?;
    if !text.len().is_multiple_of(2) {
        return None;
    }
    let bytes = (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&text[at..at + 2], 16).ok())
        .collect::<Option<Vec<u8>>>()?;
    String::from_utf8(bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn replies(buf: &[u8]) -> Vec<Reply> {
        let (replies, used) = parse(buf);
        assert_eq!(used, buf.len(), "a complete buffer is used whole");
        replies
    }

    #[test]
    fn device_attributes_report_sixel_by_attribute_4() {
        assert_eq!(
            replies(b"\x1b[?62;4;22c"),
            [Reply::DeviceAttributes { sixel: true }]
        );
        assert_eq!(
            replies(b"\x1b[?62;22;44c"),
            [Reply::DeviceAttributes { sixel: false }]
        );
    }

    #[test]
    fn xtversion_gives_the_version_string() {
        assert_eq!(
            replies(b"\x1bP>|kitty(0.35.2)\x1b\\"),
            [Reply::Version("kitty(0.35.2)".to_string())]
        );
    }

    #[test]
    fn colors_parse_with_either_terminator() {
        let white = ResolvedColor::from_srgb(255, 255, 255);
        assert_eq!(
            replies(b"\x1b]10;rgb:ffff/ffff/ffff\x1b\\"),
            [Reply::Foreground(white)]
        );
        assert_eq!(
            replies(b"\x1b]11;rgb:ffff/ffff/ffff\x07"),
            [Reply::Background(white)]
        );
    }

    #[test]
    fn color_channels_scale_from_any_width() {
        let expected = ResolvedColor::from_srgb(0x12, 0x80, 0xff);
        for spec in ["rgb:1212/8080/ffff", "rgb:121/808/fff", "rgb:12/80/ff"] {
            let reply = format!("\x1b]11;{spec}\x1b\\");
            assert_eq!(
                replies(reply.as_bytes()),
                [Reply::Background(expected)],
                "{spec}"
            );
        }
        assert_eq!(
            replies(b"\x1b]11;rgb:f/0/8\x1b\\"),
            [Reply::Background(ResolvedColor::from_srgb(255, 0, 0x88))]
        );
    }

    #[test]
    fn a_malformed_color_is_skipped() {
        assert_eq!(replies(b"\x1b]11;rgb:ff/ff\x1b\\"), []);
        assert_eq!(replies(b"\x1b]11;rgb:fffff/0/0\x1b\\"), []);
    }

    #[test]
    fn kitty_graphics_reply_reports_support_only_for_ok() {
        assert_eq!(
            replies(b"\x1b_Gi=31;OK\x1b\\"),
            [Reply::KittyGraphics(true)]
        );
        assert_eq!(
            replies(b"\x1b_Gi=31;EINVAL:unsupported\x1b\\"),
            [Reply::KittyGraphics(false)]
        );
        assert_eq!(replies(b"\x1b_Gi=7;OK\x1b\\"), []);
    }

    #[test]
    fn xtgettcap_reports_truecolor_capabilities() {
        assert_eq!(
            replies(b"\x1bP1+r524742=382F382F38\x1b\\"),
            [Reply::TrueColor(true)]
        );
        assert_eq!(replies(b"\x1bP1+r5463\x1b\\"), [Reply::TrueColor(true)]);
        assert_eq!(replies(b"\x1bP0+r524742\x1b\\"), [Reply::TrueColor(false)]);
        assert_eq!(replies(b"\x1bP1+r636F6C6F7273=323536\x1b\\"), []);
    }

    #[test]
    fn typed_keys_and_unknown_sequences_are_skipped() {
        let buf = b"q\x1b[A\x1bx\x1b[?62;4c\x1b]52;c;aGk=\x07z\x1bP>|foot(1.17)\x1b\\";
        assert_eq!(
            replies(buf),
            [
                Reply::DeviceAttributes { sixel: true },
                Reply::Version("foot(1.17)".to_string()),
            ]
        );
    }

    #[test]
    fn a_reply_split_at_any_byte_parses_once_complete() {
        let full: &[u8] =
            b"\x1b]11;rgb:1212/8080/ffff\x1b\\\x1bP>|WezTerm 20240203\x1b\\\x1b[?62;4c";
        let expected = replies(full);
        assert_eq!(expected.len(), 3);
        for split in 1..full.len() {
            let (mut got, used) = parse(&full[..split]);
            let mut rest = full[used..split].to_vec();
            rest.extend_from_slice(&full[split..]);
            let (more, used) = parse(&rest);
            assert_eq!(used, rest.len(), "split at {split}");
            got.extend(more);
            assert_eq!(got, expected, "split at {split}");
        }
    }

    #[test]
    fn the_batch_ends_with_the_device_attributes_request() {
        let batch = query_batch();
        assert!(batch.ends_with(b"\x1b[c"));
        assert!(
            batch
                .windows(b"\x1bP+q524742\x1b\\".len())
                .any(|window| window == b"\x1bP+q524742\x1b\\")
        );
    }
}
