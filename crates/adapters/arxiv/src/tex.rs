//! The figures a paper's TeX files draw, in the order they are drawn: what `\includegraphics` (and `\includesvg`,
//! `\epsfig`, `\psfig`) name, looked for where `\graphicspath` says. The files are read as TeX reads them: from the main
//! file, each part where `\input`, `\include` or `\subfile` brings it in.

/// A TeX file of the source.
pub struct Document {
    /// Its path in the source, with `/` between names.
    pub path: String,
    pub text: String,
}

impl Document {
    /// Whether it is a paper's main file rather than a part it inputs.
    fn is_main(&self) -> bool {
        self.text.contains("\\documentclass") || self.text.contains("\\documentstyle")
    }

    /// Its folder in the source; empty at the top.
    fn folder(&self) -> String {
        normalize(self.path.rsplit_once('/').map_or("", |(f, _)| f))
    }
}

/// What a file brings in, where: a figure, or another file.
enum Use {
    Figure(String),
    Part(String),
}

/// The files the documents draw, in the order they are drawn (from each main file, the parts it brings in read in their
/// places, then any file no main file brings in): for each, the paths in the source it may be, with `/` between names
/// and with or without an extension, as TeX looks for them.
pub fn references(documents: &[Document]) -> Vec<Vec<String>> {
    let texts: Vec<String> = documents.iter().map(|d| without_comments(&d.text)).collect();
    let mut folders = vec![String::new()];
    for text in &texts {
        for (_, arg) in arguments(text, "\\graphicspath") {
            folders.extend(groups(&arg).into_iter().map(|g| normalize(&g)));
        }
    }
    let mut starts: Vec<usize> = (0..documents.len()).collect();
    starts.sort_by_key(|&k| !documents[k].is_main());
    let mut read = vec![false; documents.len()];
    let mut out = Vec::new();
    for k in starts {
        visit(k, documents, &texts, &folders, &mut read, &mut out);
    }
    out
}

/// Add the figures document `k` draws to `out`, and those of the parts it brings in, in their places; a document is read
/// once.
fn visit(
    k: usize,
    documents: &[Document],
    texts: &[String],
    folders: &[String],
    read: &mut [bool],
    out: &mut Vec<Vec<String>>,
) {
    if read[k] {
        return;
    }
    read[k] = true;
    let (document, text) = (&documents[k], &texts[k]);
    let folder = document.folder();
    for (_, used) in uses(text) {
        match used {
            Use::Figure(name) => {
                let name = name.trim().trim_matches('"').trim();
                let mut places = Vec::new();
                for base in [String::new(), folder.clone()] {
                    for f in folders {
                        let place = normalize(&format!("{base}/{f}/{name}"));
                        if !place.is_empty() && !places.contains(&place) {
                            places.push(place);
                        }
                    }
                }
                out.push(places);
            }
            Use::Part(name) => {
                // as TeX looks for it: from the top of the source, then beside this file, with or without `.tex`
                let name = name.trim();
                let wanted: Vec<String> = [String::new(), folder.clone()]
                    .iter()
                    .flat_map(|base| [normalize(&format!("{base}/{name}")), normalize(&format!("{base}/{name}.tex"))])
                    .collect();
                if let Some(part) = documents.iter().position(|d| wanted.contains(&normalize(&d.path))) {
                    visit(part, documents, texts, folders, read, out);
                }
            }
        }
    }
}

/// The figures and parts `text` uses, in the order it uses them.
fn uses(text: &str) -> Vec<(usize, Use)> {
    let mut found = Vec::new();
    for command in ["\\includegraphics", "\\includesvg"] {
        found.extend(arguments(text, command).into_iter().map(|(at, name)| (at, Use::Figure(name))));
    }
    for command in ["\\epsfig", "\\psfig"] {
        for (at, arg) in arguments(text, command) {
            let file = arg.split(',').find_map(|kv| {
                let (key, value) = kv.split_once('=')?;
                matches!(key.trim(), "file" | "figure").then(|| value.trim().to_string())
            });
            found.extend(file.map(|f| (at, Use::Figure(f))));
        }
    }
    for command in ["\\input", "\\include", "\\subfile"] {
        found.extend(arguments(text, command).into_iter().map(|(at, name)| (at, Use::Part(name))));
    }
    found.sort_by_key(|(at, _)| *at);
    found
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

    fn doc(path: &str, text: &str) -> Document {
        Document { path: path.into(), text: text.into() }
    }

    #[test]
    fn the_parts_a_main_file_brings_in_are_read_in_their_places() {
        // as a tar may hold them: the parts before the main file, in another order
        let late = doc("sections/late.tex", r"\includegraphics{c}");
        let early = doc("sections/early.tex", r"\includegraphics{b}");
        let unused = doc("notes.tex", r"\includegraphics{z}");
        let main = doc(
            "main.tex",
            r"\documentclass{article}
            \includegraphics{a}
            \subfile{sections/early}
            \include{sections/late.tex}
            \input{sections/late}
            \includegraphics{d}",
        );
        let refs = references(&[late, unused, early, main]);
        let first: Vec<&str> = refs.iter().map(|places| places[0].as_str()).collect();
        assert_eq!(first, ["a", "b", "c", "d", "z"], "a part is read once; a file no main file brings in comes last");
    }

    #[test]
    fn the_figures_are_found_in_the_order_they_are_drawn_main_file_first() {
        let main = doc(
            "paper.tex",
            r"\documentclass{article}
            \graphicspath{{figs/}{./img/}}
            \begin{figure}\includegraphics[width=0.5\linewidth, trim={1 2 3 4}]{loss}\end{figure}
            % \includegraphics{commented}
            50\% of \includegraphics*{ ./plots/a.pdf }
            \epsfig{file=old.eps, width=3in}
            \includegraphicsx{not-this}
            \input{sections/results}",
        );
        let part = doc("sections/results.tex", r"\includesvg{diagram}");
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
