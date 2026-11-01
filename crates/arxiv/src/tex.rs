//! The figures a paper's TeX files draw, in the order they are drawn: what `\includegraphics` (and `\includesvg`,
//! `\epsfig`, `\psfig`) name, looked for where `\graphicspath` says.

/// A TeX file of the source.
pub struct Document {
    /// Its folder in the source, with `/` between names; empty at the top.
    pub folder: String,
    pub text: String,
}

impl Document {
    /// Whether it is a paper's main file rather than a part it inputs.
    fn is_main(&self) -> bool {
        self.text.contains("\\documentclass") || self.text.contains("\\documentstyle")
    }
}

/// The files the documents draw, in order (main files first): for each, the paths in the source it may be, with `/`
/// between names and with or without an extension, as TeX looks for them.
pub fn references(documents: &[Document]) -> Vec<Vec<String>> {
    let mut order: Vec<&Document> = documents.iter().collect();
    order.sort_by_key(|d| !d.is_main());
    let texts: Vec<String> = order.iter().map(|d| without_comments(&d.text)).collect();
    let mut folders = vec![String::new()];
    for text in &texts {
        for (_, arg) in arguments(text, "\\graphicspath") {
            folders.extend(groups(&arg).into_iter().map(|g| normalize(&g)));
        }
    }
    let mut out = Vec::new();
    for (document, text) in order.iter().zip(&texts) {
        let mut names = Vec::new();
        for command in ["\\includegraphics", "\\includesvg"] {
            names.extend(arguments(text, command));
        }
        for command in ["\\epsfig", "\\psfig"] {
            for (at, arg) in arguments(text, command) {
                let file = arg.split(',').find_map(|kv| {
                    let (key, value) = kv.split_once('=')?;
                    matches!(key.trim(), "file" | "figure").then(|| value.trim().to_string())
                });
                names.extend(file.map(|f| (at, f)));
            }
        }
        names.sort_by_key(|(at, _)| *at);
        for (_, name) in names {
            let name = name.trim().trim_matches('"').trim();
            let mut places = Vec::new();
            for base in [String::new(), document.folder.clone()] {
                for folder in &folders {
                    let place = normalize(&format!("{base}/{folder}/{name}"));
                    if !place.is_empty() && !places.contains(&place) {
                        places.push(place);
                    }
                }
            }
            out.push(places);
        }
    }
    out
}

/// `text` with every comment (from an unescaped `%` to the end of its line) taken out.
fn without_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let mut escaped = false;
        for c in line.chars() {
            if c == '%' && !escaped {
                break;
            }
            escaped = c == '\\' && !escaped;
            out.push(c);
        }
        out.push('\n');
    }
    out
}

/// Where each use of `command` is, and its first argument in braces, after a `*` and an optional argument in brackets.
fn arguments(text: &str, command: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(k) = text[from..].find(command) {
        let at = from + k;
        from = at + command.len();
        let rest = &text[from..];
        // a longer command that starts the same (\includegraphicsx) is another command
        if rest.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
            continue;
        }
        let mut rest = rest.trim_start().strip_prefix('*').unwrap_or(rest.trim_start()).trim_start();
        if rest.starts_with('[') {
            let Some(end) = closing(rest, '[', ']') else { continue };
            rest = rest[end + 1..].trim_start();
        }
        if rest.starts_with('{')
            && let Some(end) = closing(rest, '{', '}')
        {
            out.push((at, rest[1..end].to_string()));
        }
    }
    out
}

/// The position of the bracket that closes the one `s` starts with.
fn closing(s: &str, open: char, close: char) -> Option<usize> {
    let mut depth = 0usize;
    for (k, c) in s.char_indices() {
        if c == open {
            depth += 1;
        } else if c == close {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(k);
            }
        }
    }
    None
}

/// What each `{...}` group of `s` holds.
fn groups(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = s.trim_start();
    while rest.starts_with('{') {
        let Some(end) = closing(rest, '{', '}') else { break };
        out.push(rest[1..end].trim().to_string());
        rest = rest[end + 1..].trim_start();
    }
    out
}

/// A path in the source with `/` between names, `.` and `..` resolved, no slash at either end.
pub fn normalize(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split(['/', '\\']) {
        match part.trim() {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(folder: &str, text: &str) -> Document {
        Document { folder: folder.into(), text: text.into() }
    }

    #[test]
    fn the_figures_are_found_in_the_order_they_are_drawn_main_file_first() {
        let main = doc(
            "",
            r"\documentclass{article}
            \graphicspath{{figs/}{./img/}}
            \begin{figure}\includegraphics[width=0.5\linewidth, trim={1 2 3 4}]{loss}\end{figure}
            % \includegraphics{commented}
            50\% of \includegraphics*{ ./plots/a.pdf }
            \epsfig{file=old.eps, width=3in}
            \includegraphicsx{not-this}
            \input{sections/results}",
        );
        let part = doc("sections", r"\includesvg{diagram}");
        let refs = references(&[part, main]);
        let first: Vec<&str> = refs.iter().map(|places| places[0].as_str()).collect();
        assert_eq!(first, ["loss", "plots/a.pdf", "old.eps", "diagram"]);
        assert_eq!(refs[0], ["loss", "figs/loss", "img/loss"]);
        assert!(refs[3].contains(&"sections/figs/diagram".to_string()), "{:?}", refs[3]);
    }

    #[test]
    fn paths_are_made_plain() {
        assert_eq!(normalize("./a//b/../c/"), "a/c");
        assert_eq!(normalize("../../x"), "x");
        assert_eq!(normalize("figs\\one.png"), "figs/one.png");
    }
}
