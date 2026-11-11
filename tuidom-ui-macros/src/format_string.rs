//! Scanning a `format!`-style string for the names its holes read.

/// What a format string reads, and its text with escapes resolved when it reads nothing.
pub(crate) struct Scanned {
    /// Each name a hole or a `name$` width or precision reads, once, in order of appearance.
    pub names: Vec<String>,
    /// The string with `{{` and `}}` resolved. Meaningful only when `names` is empty.
    pub unescaped: String,
}

/// Scan `text`, accepting only named holes. The error says what is wrong in plain words.
pub(crate) fn scan(text: &str) -> Result<Scanned, String> {
    let mut names = Vec::new();
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
                let (name, spec) = hole.split_once(':').unwrap_or((&hole, ""));
                push_name(&mut names, name.trim())?;
                for name in spec_names(spec)? {
                    push_name(&mut names, name)?;
                }
            }
            c => unescaped.push(c),
        }
    }
    Ok(Scanned { names, unescaped })
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

/// The names a spec reads through `name$` widths and precisions. A `.*` precision reads a
/// positional argument, which a hole cannot supply.
fn spec_names(spec: &str) -> Result<Vec<&str>, String> {
    if spec.contains(".*") {
        return Err("a `.*` precision needs a positional argument; use `.name$`".to_string());
    }
    let mut names = Vec::new();
    for (end, _) in spec.match_indices('$') {
        let before = &spec[..end];
        let start = before
            .rfind(|c: char| !(c.is_alphanumeric() || c == '_'))
            .map_or(0, |i| i + 1);
        names.push(&before[start..]);
    }
    Ok(names)
}

fn is_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_')
        && name != "_"
}
