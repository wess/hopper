//! The page shell: the same header, sidebar and footer around every page.
//!
//! Deliberately string templating rather than a template engine. There is one
//! layout, it fits on a screen, and a dependency that renders it would be
//! larger than the thing it renders.

use crate::page::Page;

/// Escape text destined for HTML.
pub fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The sidebar, grouped, with the current page marked.
pub fn nav(pages: &[Page], current: &str, groups: &[&str]) -> String {
    let mut out = String::new();
    for group in groups {
        let in_group: Vec<&Page> = pages.iter().filter(|p| p.group == *group).collect();
        if in_group.is_empty() {
            continue;
        }
        out.push_str(&format!("      <h5>{}</h5>\n", escape(group)));
        for p in in_group {
            let here = if p.slug == current { " class=\"active\"" } else { "" };
            out.push_str(&format!(
                "      <a href=\"{}.html\"{}>{}</a>\n",
                p.slug,
                here,
                escape(&p.title)
            ));
        }
    }
    out
}

/// Previous / next links, so the docs read as a sequence rather than a pile.
pub fn pager(pages: &[Page], current: &str) -> String {
    let Some(i) = pages.iter().position(|p| p.slug == current) else {
        return String::new();
    };
    let prev = i.checked_sub(1).and_then(|j| pages.get(j));
    let next = pages.get(i + 1);
    if prev.is_none() && next.is_none() {
        return String::new();
    }
    let mut out = String::from("      <nav class=\"pager\">\n");
    match prev {
        Some(p) => out.push_str(&format!(
            "        <a class=\"prev\" href=\"{}.html\"><span>Previous</span>{}</a>\n",
            p.slug,
            escape(&p.title)
        )),
        None => out.push_str("        <span></span>\n"),
    }
    if let Some(n) = next {
        out.push_str(&format!(
            "        <a class=\"next\" href=\"{}.html\"><span>Next</span>{}</a>\n",
            n.slug,
            escape(&n.title)
        ));
    }
    out.push_str("      </nav>\n");
    out
}

/// Wrap a rendered body in the site shell.
pub fn shell(page: &Page, pages: &[Page], groups: &[&str]) -> String {
    let description = if page.summary.is_empty() {
        format!("{} — Hopper documentation.", page.title)
    } else {
        page.summary.clone()
    };
    format!(
        r##"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{title} — Hopper</title>
  <meta name="description" content="{description}">
  <meta name="theme-color" content="#0c1018">
  <link rel="stylesheet" href="styles.css">
  <script src="app.js" defer></script>
</head>
<body>
  <div class="aurora" aria-hidden="true"></div>

  <header class="nav">
    <a class="brand" href="index.html">
      <svg viewBox="0 0 24 24" fill="none" aria-hidden="true"><rect x="1.5" y="1.5" width="21" height="21" rx="6" stroke="currentColor" stroke-width="1.4" opacity="0.5"/><rect x="7" y="7" width="10" height="10" rx="3" fill="var(--blue)"/></svg>
      Hopper
    </a>
    <nav>
      <a href="index.html#workflow">Workflow</a>
      <a href="index.html#engine">Engine</a>
      <a href="docs.html">Docs</a>
      <a href="tutorials.html">Tutorials</a>
    </nav>
    <a class="ghjump" href="https://github.com/wess/hopper">GitHub ↗</a>
  </header>

  <div class="wrap docs-layout">
    <aside class="docs-nav" aria-label="Documentation">
{nav}    </aside>

    <article class="docs-body">
      <h1>{title}</h1>
{lede}{body}
{pager}    </article>
  </div>

  <footer class="sitefooter">
    <div class="wrap">
      <span>Hopper — containers on macOS and Linux, natively.</span>
      <a href="https://github.com/wess/hopper">github.com/wess/hopper</a>
    </div>
  </footer>
</body>
</html>
"##,
        title = escape(&page.title),
        description = escape(&description),
        nav = nav(pages, &page.slug, groups),
        lede = if page.summary.is_empty() {
            String::new()
        } else {
            format!("      <p class=\"lede-sm\">{}</p>\n", escape(&page.summary))
        },
        body = page.body.trim_end(),
        pager = pager(pages, &page.slug),
    )
}

/// The docs index: one card per page, grouped.
pub fn index(pages: &[Page], groups: &[&str], slug: &str, title: &str, lede: &str) -> String {
    // A single group whose name is the page title would just repeat the <h1>.
    let repeats_title = groups.len() == 1 && groups[0].eq_ignore_ascii_case(title);

    let mut body = String::new();
    for group in groups {
        let in_group: Vec<&Page> = pages.iter().filter(|p| p.group == *group).collect();
        if in_group.is_empty() {
            continue;
        }
        if !repeats_title {
            body.push_str(&format!("<h2>{}</h2>\n", escape(group)));
        }
        body.push_str("<div class=\"cards\">\n");
        for p in in_group {
            body.push_str(&format!(
                "  <a class=\"card\" href=\"{}.html\"><h3>{}</h3><p>{}</p></a>\n",
                p.slug,
                escape(&p.title),
                escape(&p.summary)
            ));
        }
        body.push_str("</div>\n");
    }
    let page = Page {
        slug: slug.to_string(),
        title: title.to_string(),
        group: String::new(),
        order: 0,
        summary: lede.to_string(),
        body,
    };
    shell(&page, pages, groups)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(slug: &str, group: &str, order: u32) -> Page {
        Page {
            slug: slug.into(),
            title: slug.to_uppercase(),
            group: group.into(),
            order,
            summary: format!("about {slug}"),
            body: "<p>body</p>".into(),
        }
    }

    fn pages() -> Vec<Page> {
        vec![
            page("install", "Start", 1),
            page("engines", "Start", 2),
            page("cli", "Use", 1),
        ]
    }

    #[test]
    fn html_special_characters_are_escaped() {
        assert_eq!(escape("a & b < c > \"d\""), "a &amp; b &lt; c &gt; &quot;d&quot;");
    }

    #[test]
    fn the_nav_groups_pages_and_marks_the_current_one() {
        let html = nav(&pages(), "engines", &["Start", "Use"]);
        assert!(html.contains("<h5>Start</h5>"));
        assert!(html.contains("<h5>Use</h5>"));
        assert!(html.contains(r#"<a href="engines.html" class="active">ENGINES</a>"#));
        assert!(html.contains(r#"<a href="install.html">INSTALL</a>"#));
    }

    #[test]
    fn an_empty_group_is_not_given_a_heading() {
        // A heading with nothing under it reads as a broken build.
        let html = nav(&pages(), "install", &["Start", "Use", "Nothing"]);
        assert!(!html.contains("Nothing"));
    }

    #[test]
    fn the_pager_links_both_ways_in_the_middle() {
        let html = pager(&pages(), "engines");
        assert!(html.contains(r#"href="install.html""#));
        assert!(html.contains(r#"href="cli.html""#));
    }

    #[test]
    fn the_first_page_has_no_previous_and_the_last_no_next() {
        let first = pager(&pages(), "install");
        assert!(!first.contains("class=\"prev\""));
        assert!(first.contains("class=\"next\""));

        let last = pager(&pages(), "cli");
        assert!(last.contains("class=\"prev\""));
        assert!(!last.contains("class=\"next\""));
    }

    #[test]
    fn a_single_page_gets_no_pager_at_all() {
        assert_eq!(pager(&[page("only", "Start", 1)], "only"), "");
    }

    #[test]
    fn an_unknown_slug_gets_no_pager_rather_than_panicking() {
        assert_eq!(pager(&pages(), "does-not-exist"), "");
    }

    #[test]
    fn the_shell_carries_title_description_and_body() {
        let all = pages();
        let html = shell(&all[0], &all, &["Start", "Use"]);
        assert!(html.contains("<title>INSTALL — Hopper</title>"));
        assert!(html.contains(r#"<meta name="description" content="about install">"#));
        assert!(html.contains("<p>body</p>"));
        assert!(html.starts_with("<!doctype html>"));
    }

    #[test]
    fn a_page_without_a_summary_still_gets_a_description() {
        let mut p = page("x", "Start", 1);
        p.summary = String::new();
        let html = shell(&p, &[p.clone()], &["Start"]);
        assert!(html.contains("Hopper documentation"));
        assert!(!html.contains("lede-sm"), "no summary means no lede paragraph");
    }

    #[test]
    fn a_lone_group_named_after_the_page_does_not_repeat_the_heading() {
        // The tutorials index is <h1>Tutorials</h1>; an <h2>Tutorials</h2>
        // under it says the word twice for no reason.
        let all = vec![page("first", "Tutorials", 1)];
        let html = index(&all, &["Tutorials"], "tutorials", "Tutorials", "walkthroughs");
        assert!(html.contains("<h1>Tutorials</h1>"));
        assert!(!html.contains("<h2>Tutorials</h2>"));
        assert!(html.contains(r#"href="first.html""#));
    }

    #[test]
    fn several_groups_keep_their_headings() {
        let all = pages();
        let html = index(&all, &["Start", "Use"], "docs", "Documentation", "everything");
        assert!(html.contains("<h2>Start</h2>"));
        assert!(html.contains("<h2>Use</h2>"));
    }

    #[test]
    fn the_index_lists_every_page_as_a_card() {
        let all = pages();
        let html = index(&all, &["Start", "Use"], "docs", "Documentation", "everything");
        for p in &all {
            assert!(html.contains(&format!(r#"href="{}.html""#, p.slug)));
        }
        assert!(html.contains("<h1>Documentation</h1>"));
    }
}
