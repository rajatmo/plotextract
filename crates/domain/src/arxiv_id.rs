//! The number of an arXiv paper, as people write it: `2301.01234`, `2301.01234v2`, `arXiv:2301.01234`, an old-style
//! `hep-th/9901001`, or a link to any of its pages.

use std::fmt;

/// The number of an arXiv paper, with its version if one was given. Only numbers of the two forms arXiv uses are
/// accepted, so one is safe to put in a URL.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ArxivId(String);

impl ArxivId {
    /// The paper `text` names, if it names one.
    pub fn parse(text: &str) -> Option<Self> {
        let mut s = text.trim();
        // a link to one of the paper's pages: what follows the page's kind
        for page in ["/abs/", "/pdf/", "/src/", "/e-print/", "/format/", "/html/"] {
            if let Some(k) = s.find(page) {
                s = &s[k + page.len()..];
                break;
            }
        }
        s = s.split(['?', '#']).next().unwrap_or_default().trim_end_matches('/');
        s = s.strip_suffix(".pdf").unwrap_or(s);
        if s.len() >= 6 && s[..6].eq_ignore_ascii_case("arxiv:") {
            s = &s[6..];
        }
        let s = s.trim();
        (new_style(s) || old_style(s)).then(|| Self(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Where arXiv serves the paper's source.
    pub fn source_url(&self) -> String {
        format!("https://arxiv.org/src/{}", self.0)
    }

    /// A folder name for the paper: the number, with the slash of an old-style one made an underscore.
    pub fn folder_name(&self) -> String {
        self.0.replace('/', "_")
    }
}

impl fmt::Display for ArxivId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// `s` without a version suffix (`v` and digits) at its end.
fn without_version(s: &str) -> &str {
    match s.rfind('v') {
        Some(k) if k + 1 < s.len() && s[k + 1..].bytes().all(|b| b.is_ascii_digit()) => &s[..k],
        _ => s,
    }
}

fn digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// `2301.01234`: year and month, then a 4 or 5 digit number.
fn new_style(s: &str) -> bool {
    let Some((month, number)) = without_version(s).split_once('.') else { return false };
    month.len() == 4 && digits(month) && (4..=5).contains(&number.len()) && digits(number)
}

/// `hep-th/9901001` or `math.GT/0309136`: the archive (and subject class), then year, month and a 3 digit number.
fn old_style(s: &str) -> bool {
    let Some((archive, number)) = without_version(s).split_once('/') else { return false };
    let (archive, class) = archive.split_once('.').unwrap_or((archive, "AA"));
    !archive.is_empty()
        && archive.bytes().all(|b| b.is_ascii_lowercase() || b == b'-')
        && class.len() == 2
        && class.bytes().all(|b| b.is_ascii_uppercase())
        && number.len() == 7
        && digits(number)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(text: &str) -> Option<String> {
        ArxivId::parse(text).map(|i| i.to_string())
    }

    #[test]
    fn numbers_are_read_however_they_are_written() {
        for (text, want) in [
            ("2301.01234", "2301.01234"),
            ("  1706.03762v7 ", "1706.03762v7"),
            ("arXiv:1706.03762", "1706.03762"),
            ("ARXIV:0704.0001", "0704.0001"),
            ("https://arxiv.org/abs/1706.03762v2", "1706.03762v2"),
            ("https://arxiv.org/pdf/1706.03762.pdf", "1706.03762"),
            ("arxiv.org/pdf/1706.03762v3?download=1", "1706.03762v3"),
            ("https://export.arxiv.org/abs/hep-th/9901001/", "hep-th/9901001"),
            ("math.GT/0309136v1", "math.GT/0309136v1"),
            ("cond-mat/0101001", "cond-mat/0101001"),
        ] {
            assert_eq!(id(text).as_deref(), Some(want), "{text}");
        }
    }

    #[test]
    fn anything_else_is_not_a_number() {
        for text in [
            "",
            "1706.037",
            "1706.037621",
            "17066.03762",
            "1706.03762v",
            "1706.03762x",
            "../../etc/passwd",
            "1706.03762/../x",
            "HEP-TH/9901001",
            "hep-th/990100",
            "math.gt/0309136",
            "https://example.com/abs/../1706.03762?x",
            "1706.03762 && rm",
        ] {
            assert_eq!(id(text), None, "{text}");
        }
    }

    #[test]
    fn the_source_url_and_folder() {
        let old = ArxivId::parse("hep-th/9901001v2").unwrap();
        assert_eq!(old.source_url(), "https://arxiv.org/src/hep-th/9901001v2");
        assert_eq!(old.folder_name(), "hep-th_9901001v2");
    }
}
