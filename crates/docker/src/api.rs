//! Negotiate the intersection of the client's tested range and the daemon's.

use crate::{DockerError, Result};

pub const TARGET_API: &str = "1.44";
pub const MIN_API: &str = "1.24";

fn parse(value: &str) -> Result<(u32, u32)> {
    let value = value.trim().trim_start_matches('v');
    let (major, minor) = value
        .split_once('.')
        .ok_or_else(|| DockerError::decode("The daemon reported an invalid API version."))?;
    if major.is_empty()
        || minor.is_empty()
        || !major
            .bytes()
            .chain(minor.bytes())
            .all(|b| b.is_ascii_digit())
    {
        return Err(DockerError::decode(
            "The daemon reported an invalid API version.",
        ));
    }
    Ok((
        major
            .parse()
            .map_err(|_| DockerError::decode("Invalid API major version."))?,
        minor
            .parse()
            .map_err(|_| DockerError::decode("Invalid API minor version."))?,
    ))
}

pub fn negotiate(maximum: &str, minimum: &str) -> Result<String> {
    let maximum = parse(maximum)?;
    // Older daemons do not advertise a minimum. Our own floor still applies.
    let minimum = if minimum.is_empty() {
        (0, 0)
    } else {
        parse(minimum)?
    };
    let selected = maximum.min(parse(TARGET_API)?);
    let floor = minimum.max(parse(MIN_API)?);
    if selected < floor || minimum > maximum {
        return Err(DockerError::api(400, format!(
            "Incompatible Docker API: Hopper supports {MIN_API}–{TARGET_API}, but the daemon supports {}.{}–{}.{}.",
            minimum.0, minimum.1, maximum.0, maximum.1,
        )));
    }
    Ok(format!("{}.{}", selected.0, selected.1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intersects_old_current_and_restricted_daemon_ranges() {
        for (max, min, expected) in [
            ("1.56", "1.40", "1.44"),
            ("1.52", "1.44", "1.44"),
            ("v1.41", "", "1.41"),
            ("1.24", "1.12", "1.24"),
        ] {
            assert_eq!(negotiate(max, min).unwrap(), expected);
        }
    }

    #[test]
    fn incompatible_and_malformed_ranges_fail_instead_of_inventing_support() {
        for (max, min) in [
            ("1.20", "1.12"),
            ("1.56", "1.45"),
            ("1.40", "1.44"),
            ("", ""),
            ("1.x", ""),
            ("1.44.0", ""),
            ("1.44", "garbage"),
        ] {
            assert!(negotiate(max, min).is_err(), "{min}–{max}");
        }
    }
}
