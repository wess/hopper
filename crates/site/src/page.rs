//! A content file: its front matter, and its body as HTML.

use pulldown_cmark::{html, Options, Parser};

/// One page of the site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    /// Output file stem — `install` becomes `install.html`.
    pub slug: String,
    pub title: String,
    /// Sidebar heading this page sits under.
    pub group: String,
    /// Position within the group. Ties fall back to the title.
    pub order: u32,
    /// One line, used on the docs index and in `<meta name="description">`.
    pub summary: String,
    /// Rendered body.
    pub body: String,
}

/// Split `---` front matter from the body.
///
/// Returns the raw body unchanged when there is no front matter, so a file
/// that forgets it still renders rather than vanishing.
pub fn split(raw: &str) -> (Vec<(String, String)>, &str) {
    let rest = match raw.strip_prefix("---\n") {
        Some(rest) => rest,
        None => return (Vec::new(), raw),
    };
    let Some(end) = rest.find("\n---") else {
        return (Vec::new(), raw);
    };
    let (head, tail) = rest.split_at(end);
    let body = tail.trim_start_matches("\n---").trim_start_matches('\n');

    let fields = head
        .lines()
        .filter_map(|line| {
            let (k, v) = line.split_once(':')?;
            Some((k.trim().to_string(), v.trim().to_string()))
        })
        .collect();
    (fields, body)
}

fn field(fields: &[(String, String)], key: &str) -> Option<String> {
    fields
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.clone())
        .filter(|v| !v.is_empty())
}

/// Markdown to HTML, with the extensions the docs actually use.
pub fn markdown(body: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_HEADING_ATTRIBUTES);

    let mut out = String::new();
    html::push_html(&mut out, Parser::new_ext(body, options));
    out
}

/// Build a page from a file's stem and contents.
pub fn parse(slug: &str, raw: &str) -> Page {
    let (fields, body) = split(raw);
    Page {
        slug: slug.to_string(),
        // A missing title is better than no page: fall back to the slug so the
        // problem is visible in the nav rather than silent.
        title: field(&fields, "title").unwrap_or_else(|| slug.to_string()),
        group: field(&fields, "group").unwrap_or_else(|| "Docs".to_string()),
        order: field(&fields, "order")
            .and_then(|v| v.parse().ok())
            .unwrap_or(u32::MAX),
        summary: field(&fields, "summary").unwrap_or_default(),
        body: markdown(body),
    }
}

/// Sort into sidebar order: group order first, then `order`, then title.
pub fn sort(pages: &mut [Page], groups: &[&str]) {
    let rank = |g: &str| groups.iter().position(|x| *x == g).unwrap_or(usize::MAX);
    pages.sort_by(|a, b| {
        rank(&a.group)
            .cmp(&rank(&b.group))
            .then(a.order.cmp(&b.order))
            .then(a.title.cmp(&b.title))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn front_matter_is_split_from_the_body() {
        let (fields, body) = split("---\ntitle: Install\norder: 2\n---\n# Hello\n");
        assert_eq!(field(&fields, "title").as_deref(), Some("Install"));
        assert_eq!(field(&fields, "order").as_deref(), Some("2"));
        assert_eq!(body, "# Hello\n");
    }

    #[test]
    fn a_file_without_front_matter_still_renders() {
        // Losing a page because someone forgot the header would be worse than
        // showing it with a slug for a title.
        let (fields, body) = split("# Just markdown\n");
        assert!(fields.is_empty());
        assert_eq!(body, "# Just markdown\n");
        assert_eq!(parse("stray", "# Just markdown\n").title, "stray");
    }

    #[test]
    fn a_value_containing_a_colon_survives() {
        let (fields, _) = split("---\nsummary: Docker, without the Desktop: really\n---\nx");
        assert_eq!(
            field(&fields, "summary").as_deref(),
            Some("Docker, without the Desktop: really")
        );
    }

    #[test]
    fn an_unterminated_header_is_treated_as_body_rather_than_eating_the_page() {
        let (fields, body) = split("---\ntitle: Oops\nno terminator here");
        assert!(fields.is_empty());
        assert!(body.starts_with("---"));
    }

    #[test]
    fn markdown_renders_headings_code_and_tables() {
        let html = markdown("# Title\n\n`code`\n\n| a | b |\n|---|---|\n| 1 | 2 |\n");
        assert!(html.contains("<h1>Title</h1>"));
        assert!(html.contains("<code>code</code>"));
        assert!(html.contains("<table>"));
    }

    #[test]
    fn pages_sort_by_group_then_order_then_title() {
        let page = |slug: &str, group: &str, order: u32| Page {
            slug: slug.into(),
            title: slug.into(),
            group: group.into(),
            order,
            summary: String::new(),
            body: String::new(),
        };
        let mut pages = vec![
            page("z", "Use", 1),
            page("a", "Start", 2),
            page("b", "Start", 1),
        ];
        sort(&mut pages, &["Start", "Use"]);
        let order: Vec<&str> = pages.iter().map(|p| p.slug.as_str()).collect();
        assert_eq!(order, vec!["b", "a", "z"]);
    }

    #[test]
    fn a_page_in_an_unknown_group_sorts_last_rather_than_disappearing() {
        let page = |slug: &str, group: &str| Page {
            slug: slug.into(),
            title: slug.into(),
            group: group.into(),
            order: 1,
            summary: String::new(),
            body: String::new(),
        };
        let mut pages = vec![page("odd", "Nowhere"), page("first", "Start")];
        sort(&mut pages, &["Start"]);
        assert_eq!(pages[0].slug, "first");
        assert_eq!(pages[1].slug, "odd");
    }

    #[test]
    fn a_missing_order_sorts_after_an_explicit_one() {
        let p = parse("x", "---\ntitle: X\ngroup: Start\n---\nbody");
        assert_eq!(p.order, u32::MAX);
        assert_eq!(p.group, "Start");
    }
}
