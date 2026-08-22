//! Build the gh-pages site: `site/content/*.md` -> `docs/*.html`.
//!
//! `docs/` is what the Pages workflow publishes, so the generated HTML is
//! committed rather than built in CI — the site stays reviewable in the diff,
//! and a broken generator cannot take the site down.
//!
//! Run it with `cargo run -p site`. The landing page (`docs/index.html`) is
//! hand-authored and left alone; everything else here is generated.

mod page;
mod render;

use std::path::{Path, PathBuf};

/// Sidebar order. A group not listed here still renders, at the end.
const GROUPS: &[&str] = &["Start", "Use", "Tutorials", "Reference"];

/// Pages that make up the tutorial track, listed on their own index.
const TUTORIAL_GROUP: &str = "Tutorials";

fn main() {
    let root = repo_root();
    let content = root.join("site/content");
    let out = root.join("docs");

    let mut pages = match read_all(&content) {
        Ok(pages) if !pages.is_empty() => pages,
        Ok(_) => {
            eprintln!("no markdown found in {}", content.display());
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("could not read {}: {e}", content.display());
            std::process::exit(1);
        }
    };
    page::sort(&mut pages, GROUPS);

    let mut written = 0;
    for p in &pages {
        let html = render::shell(p, &pages, GROUPS);
        let path = out.join(format!("{}.html", p.slug));
        if let Err(e) = std::fs::write(&path, html) {
            eprintln!("could not write {}: {e}", path.display());
            std::process::exit(1);
        }
        written += 1;
    }

    // Two indexes: everything, and the tutorial track on its own.
    let docs_index = render::index(
        &pages,
        GROUPS,
        "docs",
        "Documentation",
        "Install Hopper, point it at an engine, bring your Docker world across, \
         and drive it from the CLI or an AI client.",
    );
    let tutorials: Vec<page::Page> = pages
        .iter()
        .filter(|p| p.group == TUTORIAL_GROUP)
        .cloned()
        .collect();
    let tutorials_index = render::index(
        &tutorials,
        &[TUTORIAL_GROUP],
        "tutorials",
        "Tutorials",
        "Start-to-finish walkthroughs: your first container, moving off Docker \
         Desktop, and running a Compose stack.",
    );

    for (name, html) in [("docs", docs_index), ("tutorials", tutorials_index)] {
        let path = out.join(format!("{name}.html"));
        if let Err(e) = std::fs::write(&path, html) {
            eprintln!("could not write {}: {e}", path.display());
            std::process::exit(1);
        }
        written += 1;
    }

    println!("{written} pages -> {}", out.display());
}

/// Read every `.md` in a directory, sorted so the build is reproducible.
fn read_all(dir: &Path) -> std::io::Result<Vec<page::Page>> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "md"))
        .collect();
    entries.sort();

    entries
        .iter()
        .map(|path| {
            let slug = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("untitled")
                .to_string();
            let raw = std::fs::read_to_string(path)?;
            Ok(page::parse(&slug, &raw))
        })
        .collect()
}

/// The workspace root, so the generator works from any working directory.
fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is crates/site; the root is two levels up.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}
