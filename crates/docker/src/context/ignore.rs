//! Docker ignore syntax uses rooted Go filepath globs plus **. It is not a
//! gitignore: braces and ! inside a class are literal. Negations apply to
//! excluded paths and inherit the matching state of each ancestor.

use crate::{DockerError, Result};
use regex::{RegexSet, RegexSetBuilder};
use std::io::Read;
use std::path::Path;

pub struct Rules {
    matcher: RegexSet,
    excludes: Vec<bool>,
    pub(super) negated: bool,
}

pub fn load_dockerignore(context: &Path) -> Result<Rules> {
    load_rules(context, "Dockerfile")
}

pub(super) fn load_rules(context: &Path, dockerfile: &str) -> Result<Rules> {
    let specific = context.join(format!("{dockerfile}.dockerignore"));
    let path = match std::fs::symlink_metadata(&specific) {
        Ok(_) => specific,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => context.join(".dockerignore"),
        Err(error) => {
            return Err(DockerError::transport(format!(
                "Could not inspect Docker ignore file: {error}"
            )))
        }
    };
    let file = match std::fs::File::open(&path) {
        Ok(file) => file,
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound
                && std::fs::symlink_metadata(&path)
                    .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
        {
            return Ok(Rules {
                matcher: RegexSet::empty(),
                excludes: Vec::new(),
                negated: false,
            })
        }
        Err(error) => {
            return Err(DockerError::transport(format!(
                "Could not read {}: {error}",
                path.display()
            )))
        }
    };
    let mut bytes = Vec::new();
    file.take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| DockerError::transport(format!("Could not read Docker ignore file: {e}")))?;
    if bytes.len() > 1024 * 1024 {
        return Err(DockerError::decode("The Docker ignore file exceeds 1 MiB."));
    }
    let text = String::from_utf8(bytes)
        .map_err(|_| DockerError::decode("The Docker ignore file must be UTF-8."))?;
    let mut patterns = Vec::new();
    let mut excludes = Vec::new();
    let mut negated = false;
    for (index, line) in text.trim_start_matches('\u{feff}').lines().enumerate() {
        if line.starts_with('#') {
            continue;
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let (pattern, excluded) = match line.strip_prefix('!') {
            Some(pattern) => {
                negated = true;
                (pattern, false)
            }
            None => (line, true),
        };
        let invalid = || {
            DockerError::decode(format!(
                "Invalid pattern on Docker ignore line {}.",
                index + 1
            ))
        };
        if pattern.is_empty() {
            return Err(invalid());
        }
        let pattern = if cfg!(windows) {
            pattern.replace('\\', "/")
        } else {
            pattern.to_string()
        };
        let mut clean = Vec::new();
        for part in pattern.split('/') {
            match part {
                "" | "." => {}
                ".." if clean.last().is_some_and(|part| *part != "..") => {
                    clean.pop();
                }
                _ => clean.push(part),
            }
        }
        if clean.is_empty() {
            continue;
        }
        patterns.push(glob(&clean.join("/")).ok_or_else(invalid)?);
        excludes.push(excluded);
        if patterns.len() > 4096 {
            return Err(DockerError::decode(
                "The Docker ignore file exceeds 4096 patterns.",
            ));
        }
    }
    let matcher = RegexSetBuilder::new(patterns)
        .dot_matches_new_line(true)
        .size_limit(16 * 1024 * 1024)
        .dfa_size_limit(8 * 1024 * 1024)
        .build()
        .map_err(|_| {
            DockerError::decode("Docker ignore patterns exceed the supported matcher complexity.")
        })?;
    Ok(Rules {
        matcher,
        excludes,
        negated,
    })
}

pub fn is_excluded(rules: &Rules, relative: &Path, _is_dir: bool, dockerfile: &str) -> bool {
    if relative == Path::new(dockerfile)
        || relative == Path::new(".dockerignore")
        || relative == Path::new(&format!("{dockerfile}.dockerignore"))
    {
        return false;
    }
    let mut ancestors: Vec<_> = relative
        .ancestors()
        .filter(|p| !p.as_os_str().is_empty())
        .collect();
    ancestors.reverse();
    let mut parents = vec![false; rules.excludes.len()];
    let mut next = parents.clone();
    let mut excluded = false;
    for path in ancestors {
        let text = path.to_string_lossy();
        let text = if cfg!(windows) {
            text.replace('\\', "/")
        } else {
            text.into_owned()
        };
        let direct = rules.matcher.matches(&text);
        excluded = false;
        next.fill(false);
        for index in 0..rules.excludes.len() {
            if rules.excludes[index] == excluded {
                continue;
            }
            let matched = parents[index] || direct.matched(index);
            next[index] = matched;
            if matched {
                excluded = rules.excludes[index];
            }
        }
        std::mem::swap(&mut parents, &mut next);
    }
    excluded
}

fn glob(pattern: &str) -> Option<String> {
    let mut regex = String::from("^");
    let mut chars = pattern.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '*' if chars.peek() == Some(&'*') => {
                while chars.peek() == Some(&'*') {
                    chars.next();
                }
                if chars.peek() == Some(&'/') {
                    chars.next();
                    regex.push_str("(?:.*/)?");
                } else {
                    regex.push_str(".*");
                }
            }
            '*' => regex.push_str("[^/]*"),
            '?' => regex.push_str("[^/]"),
            '\\' => regex.push_str(&regex::escape(&chars.next()?.to_string())),
            '[' => {
                let mut class = String::from("[");
                if chars.peek() == Some(&'^') {
                    chars.next();
                    class.push('^');
                }
                let mut count = 0;
                while chars.peek() != Some(&']') {
                    let start = match chars.next()? {
                        '\\' => chars.next()?,
                        c => c,
                    };
                    class.push_str(&regex::escape(&start.to_string()));
                    if chars.peek() == Some(&'-') {
                        chars.next();
                        let end = match chars.next()? {
                            '\\' => chars.next()?,
                            ']' => return None,
                            c => c,
                        };
                        class.push('-');
                        class.push_str(&regex::escape(&end.to_string()));
                    }
                    count += 1;
                }
                if count == 0 {
                    return None;
                }
                chars.next();
                class.push(']');
                regex.push_str(&format!("[{class}&&[^/]]"));
            }
            c => regex.push_str(&regex::escape(&c.to_string())),
        }
    }
    regex.push('$');
    Some(regex)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filepath_classes_braces_and_double_stars_keep_docker_meaning() {
        let matches = |pattern: &str, file: &str| {
            regex::Regex::new(&glob(pattern).unwrap())
                .unwrap()
                .is_match(file)
        };
        assert!(matches("[!a].txt", "!.txt"));
        assert!(!matches("[!a].txt", "b.txt"));
        assert!(matches("[^a].txt", "b.txt"));
        assert!(!matches("[^a].txt", "a.txt"));
        assert!(matches("{a,b}", "{a,b}"));
        assert!(!matches("{a,b}", "a"));
        assert!(matches("**/*.env", "secret.env"));
        assert!(matches("**/*.env", "a/b/secret.env"));
        assert!(!matches("*.env", "nested/secret.env"));
        assert!(glob("[").is_none());
        assert!(glob("[a-]").is_none());
    }
}
