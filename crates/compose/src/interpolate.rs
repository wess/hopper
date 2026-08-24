//! `${VARIABLE}` expansion, the way compose does it.
//!
//! Substitution happens on the raw text before the YAML is parsed, which is
//! what compose itself does — `${TAG}` appears inside unquoted scalars, and
//! resolving it afterwards would mean walking every value in the tree.
//!
//! Getting this wrong is quiet: an unset variable that expands to nothing
//! turns `image: app:${TAG}` into `image: app:` and the stack fails somewhere
//! else entirely. So an unresolved name is reported, not swallowed.

use std::collections::BTreeMap;

/// The variables a file is expanded against: the `.env` file first, overlaid
/// by the process environment, which is the precedence compose uses.
pub type Vars = BTreeMap<String, String>;

/// What expansion could not resolve.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Unresolved {
    /// Names with no value and no default, which expanded to nothing.
    pub missing: Vec<String>,
    /// `${VAR:?message}` names that are set to nothing or absent. Compose
    /// treats these as fatal, and so does Hopper.
    pub required: Vec<(String, String)>,
}

/// Expand every `$VAR`, `${VAR}`, `${VAR:-default}`, `${VAR-default}`,
/// `${VAR:?err}` and `${VAR?err}` in `text`.
///
/// `$$` is an escaped dollar and comes out as a single `$`, which is how a
/// compose file writes a literal one (a shell command in an `entrypoint`, say).
pub fn expand(text: &str, vars: &Vars) -> (String, Unresolved) {
    let mut out = String::with_capacity(text.len());
    let mut unresolved = Unresolved::default();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] != '$' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        // `$$` escapes a literal dollar.
        if i + 1 < chars.len() && chars[i + 1] == '$' {
            out.push('$');
            i += 2;
            continue;
        }
        if i + 1 < chars.len() && chars[i + 1] == '{' {
            match find_close(&chars, i + 2) {
                Some(end) => {
                    let body: String = chars[i + 2..end].iter().collect();
                    out.push_str(&resolve(&body, vars, &mut unresolved));
                    i = end + 1;
                }
                // An unterminated `${` is not a variable; leave it as written.
                None => {
                    out.push('$');
                    i += 1;
                }
            }
            continue;
        }
        // Bare `$NAME`, which has no default or error form.
        let start = i + 1;
        let mut end = start;
        while end < chars.len() && (chars[end].is_alphanumeric() || chars[end] == '_') {
            end += 1;
        }
        if end == start {
            out.push('$');
            i += 1;
            continue;
        }
        let name: String = chars[start..end].iter().collect();
        out.push_str(&resolve(&name, vars, &mut unresolved));
        i = end;
    }

    (out, unresolved)
}

/// The matching `}` for a `${`, allowing one nested level so a default can
/// itself contain a variable.
fn find_close(chars: &[char], from: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (offset, c) in chars[from..].iter().enumerate() {
        match c {
            '{' => depth += 1,
            '}' if depth == 0 => return Some(from + offset),
            '}' => depth -= 1,
            _ => {}
        }
    }
    None
}

/// Resolve one `${...}` body.
fn resolve(body: &str, vars: &Vars, unresolved: &mut Unresolved) -> String {
    // `:-` and `:?` treat an empty value as unset; `-` and `?` only treat it
    // as unset when the name is absent entirely.
    for (sep, empty_counts_as_unset) in [(":-", true), (":?", true), ("-", false), ("?", false)] {
        if let Some((name, rest)) = split_once_operator(body, sep) {
            let value = vars.get(name).cloned();
            let set = match &value {
                Some(v) => !(empty_counts_as_unset && v.is_empty()),
                None => false,
            };
            if set {
                return value.unwrap_or_default();
            }
            return if sep.ends_with('?') {
                unresolved
                    .required
                    .push((name.to_string(), rest.trim().to_string()));
                String::new()
            } else {
                rest.to_string()
            };
        }
    }

    match vars.get(body) {
        Some(v) => v.clone(),
        None => {
            if !unresolved.missing.iter().any(|m| m == body) {
                unresolved.missing.push(body.to_string());
            }
            String::new()
        }
    }
}

/// Split `NAME<sep>rest`, refusing a separator that is really the start of a
/// two-character one (`FOO-bar` must not match inside `FOO:-bar`).
fn split_once_operator<'a>(body: &'a str, sep: &str) -> Option<(&'a str, &'a str)> {
    let at = body.find(sep)?;
    if sep.len() == 1 && at > 0 && &body[at - 1..at] == ":" {
        return None;
    }
    Some((&body[..at], &body[at + sep.len()..]))
}

/// Parse a `.env` / `env_file` body into pairs.
///
/// Comments, blanks and a leading `export ` are skipped; a value wrapped in
/// matching quotes is unwrapped, because `PASSWORD="s3cret"` means the six
/// characters and not the eight.
pub fn parse_env(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        let value = value.trim();
        let value = match (value.chars().next(), value.chars().last(), value.len()) {
            (Some('"'), Some('"'), n) if n >= 2 => &value[1..n - 1],
            (Some('\''), Some('\''), n) if n >= 2 => &value[1..n - 1],
            _ => value,
        };
        out.push((key.to_string(), value.to_string()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> Vars {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn a_set_variable_expands_in_both_forms() {
        let v = vars(&[("TAG", "1.2")]);
        assert_eq!(expand("app:${TAG}", &v).0, "app:1.2");
        assert_eq!(expand("app:$TAG", &v).0, "app:1.2");
    }

    #[test]
    fn an_unset_variable_expands_to_nothing_and_is_reported() {
        let (text, unresolved) = expand("app:${TAG}", &Vars::new());
        assert_eq!(text, "app:");
        // Silence here is how `image: app:` reaches the daemon.
        assert_eq!(unresolved.missing, vec!["TAG".to_string()]);
    }

    #[test]
    fn a_default_is_used_when_the_name_is_unset() {
        assert_eq!(expand("${TAG:-latest}", &Vars::new()).0, "latest");
        assert_eq!(expand("${TAG-latest}", &Vars::new()).0, "latest");
        // And is not reported: a default is an answer, not a gap.
        assert!(expand("${TAG:-latest}", &Vars::new()).1.missing.is_empty());
    }

    #[test]
    fn colon_dash_treats_an_empty_value_as_unset_and_plain_dash_does_not() {
        let v = vars(&[("TAG", "")]);
        assert_eq!(expand("${TAG:-latest}", &v).0, "latest");
        assert_eq!(expand("${TAG-latest}", &v).0, "");
    }

    #[test]
    fn a_required_variable_is_collected_with_its_message() {
        let (_, unresolved) = expand("${DB_PASSWORD:?set a password}", &Vars::new());
        assert_eq!(
            unresolved.required,
            vec![("DB_PASSWORD".to_string(), "set a password".to_string())]
        );
    }

    #[test]
    fn a_double_dollar_is_an_escaped_literal() {
        // Otherwise a shell snippet in an entrypoint gets mangled.
        assert_eq!(expand("echo $$HOME", &Vars::new()).0, "echo $HOME");
    }

    #[test]
    fn a_lone_dollar_is_left_alone() {
        assert_eq!(expand("100$ and $ ", &Vars::new()).0, "100$ and $ ");
    }

    #[test]
    fn an_unterminated_brace_is_not_treated_as_a_variable() {
        assert_eq!(expand("${OPEN", &Vars::new()).0, "${OPEN");
    }

    #[test]
    fn a_name_is_reported_once_however_often_it_appears() {
        let (_, unresolved) = expand("${A}-${A}-${A}", &Vars::new());
        assert_eq!(unresolved.missing.len(), 1);
    }

    #[test]
    fn env_files_skip_comments_and_blanks_and_unwrap_quotes() {
        let text = "# comment\n\nexport A=1\nB=\"two words\"\nC='three'\nnot a pair\n";
        assert_eq!(
            parse_env(text),
            vec![
                ("A".to_string(), "1".to_string()),
                ("B".to_string(), "two words".to_string()),
                ("C".to_string(), "three".to_string()),
            ]
        );
    }

    #[test]
    fn an_env_value_may_itself_contain_an_equals_sign() {
        // Base64 and connection strings both do.
        assert_eq!(
            parse_env("TOKEN=abc==\n"),
            vec![("TOKEN".to_string(), "abc==".to_string())]
        );
    }
}
