//! Scanning a `format!`-style string for the names its holes read.

/// What a format string reads, and its text with escapes resolved when it reads nothing.
pub(crate) struct Scanned {
    /// Each name a hole or a `name$` width or precision reads, once, in order of appearance.
    pub names: Vec<String>,
    /// The string with `{{` and `}}` resolved. Meaningful only when `names` is empty.
    pub unescaped: String,
}

/// One `{..}` hole: the argument before the colon and the spec after it.
struct Hole {
    arg: String,
    spec: String,
}

/// Scan `text`, accepting only named holes. The error says what is wrong in plain words.
pub(crate) fn scan(text: &str) -> Result<Scanned, String> {
    let (holes, unescaped) = split(text)?;
    let mut names = Vec::new();
    for hole in &holes {
        push_name(&mut names, hole.arg.trim())?;
        if hole.spec.contains(".*") {
            return Err("a `.*` precision needs a positional argument; use `.name$`".to_string());
        }
        for name in spec_refs(&hole.spec) {
            push_name(&mut names, name)?;
        }
    }
    Ok(Scanned { names, unescaped })
}

/// The names a `format!` string captures from scope, once each, in order of appearance.
/// Positional holes are skipped, and a malformed string captures nothing, leaving `format!` to
/// report it.
pub(crate) fn captures(text: &str) -> Vec<String> {
    let Ok((holes, _)) = split(text) else {
        return Vec::new();
    };
    let mut names: Vec<String> = Vec::new();
    for hole in &holes {
        for name in std::iter::once(hole.arg.trim()).chain(spec_refs(&hole.spec)) {
            if is_name(name) && !names.iter().any(|n| n == name) {
                names.push(name.to_string());
            }
        }
    }
    names
}

fn split(text: &str) -> Result<(Vec<Hole>, String), String> {
    let mut holes = Vec::new();
    let mut unescaped = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                unescaped.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                unescaped.push('}');
            }
            '}' => return Err("unmatched `}`; write `}}` for a literal brace".to_string()),
            '{' => {
                let mut hole = String::new();
                loop {
                    match chars.next() {
                        Some('}') => break,
                        Some(c) => hole.push(c),
                        None => {
                            return Err("unclosed `{`; write `{{` for a literal brace".to_string());
                        }
                    }
                }
                let (arg, spec) = hole.split_once(':').unwrap_or((&hole, ""));
                holes.push(Hole {
                    arg: arg.to_string(),
                    spec: spec.to_string(),
                });
            }
            c => unescaped.push(c),
        }
    }
    Ok((holes, unescaped))
}

fn push_name(names: &mut Vec<String>, name: &str) -> Result<(), String> {
    if !is_name(name) {
        return Err(format!(
            "`{{{name}}}` is not a name; a hole takes a name and an optional format spec, like \
             `{{count}}` or `{{ratio:.2}}`"
        ));
    }
    if !names.iter().any(|n| n == name) {
        names.push(name.to_string());
    }
    Ok(())
}

/// What a spec's `x$` widths and precisions refer to, each a name or a position.
fn spec_refs(spec: &str) -> Vec<&str> {
    spec.match_indices('$')
        .map(|(end, _)| {
            let before = &spec[..end];
            let start = before
                .rfind(|c: char| !(c.is_alphanumeric() || c == '_'))
                .map_or(0, |i| i + 1);
            &before[start..]
        })
        .collect()
}

fn is_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_')
        && name != "_"
}
