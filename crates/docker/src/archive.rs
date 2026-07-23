//! Copying files in and out of containers, and browsing their filesystems.
//!
//! Docker Desktop has a Files tab per container; the TypeScript build had no
//! equivalent, and "get a file out of a container" is a daily operation.
//!
//! `/containers/{id}/archive` moves tar streams both directions. Listing a
//! directory goes through `exec ls` instead, because pulling a tar of a
//! directory just to read its names would download every byte in it.

use crate::client::{Client, Req};
use crate::error::{DockerError, Result};
use crate::exec;
use bytes::Bytes;
use model::FileEntry;
use std::io::{Cursor, Read, Write};

/// Download a path from a container as a tar archive.
pub async fn download(client: &Client, id: &str, path: &str) -> Result<Bytes> {
    client
        .bytes(
            Req::get(format!("/containers/{id}/archive"))
                .query("path", path)
                .no_timeout(),
        )
        .await
}

/// Upload a tar archive, extracting it into `path` inside the container.
pub async fn upload(client: &Client, id: &str, path: &str, tar: Bytes) -> Result<()> {
    client
        .action(
            Req::put(format!("/containers/{id}/archive"))
                .query("path", path)
                .raw_body(tar, "application/x-tar")
                .no_timeout(),
        )
        .await
}

/// Read a single file's bytes, unwrapping the one-entry tar the daemon sends.
pub async fn read_file(client: &Client, id: &str, path: &str) -> Result<Vec<u8>> {
    let archive = download(client, id, path).await?;
    let mut tar = tar::Archive::new(Cursor::new(archive));
    let entries = tar
        .entries()
        .map_err(|e| DockerError::decode(format!("Could not read the archive: {e}")))?;

    for entry in entries {
        let mut entry =
            entry.map_err(|e| DockerError::decode(format!("Damaged archive entry: {e}")))?;
        if entry.header().entry_type().is_dir() {
            continue;
        }
        let mut buf = Vec::new();
        entry
            .read_to_end(&mut buf)
            .map_err(|e| DockerError::decode(format!("Could not read {path}: {e}")))?;
        return Ok(buf);
    }
    Err(DockerError::api(
        404,
        format!("{path} is not a file inside this container."),
    ))
}

/// Build a single-file tar, for writing into a container.
pub fn tar_single(name: &str, content: &[u8], mode: u32) -> Result<Bytes> {
    let mut builder = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    header.set_size(content.len() as u64);
    header.set_mode(mode);
    header.set_cksum();
    builder
        .append_data(&mut header, name, content)
        .map_err(|e| DockerError::transport(format!("Could not build the archive: {e}")))?;
    let out = builder
        .into_inner()
        .map_err(|e| DockerError::transport(format!("Could not finish the archive: {e}")))?;
    Ok(Bytes::from(out))
}

/// Write bytes to a path inside a container.
pub async fn write_file(client: &Client, id: &str, path: &str, content: &[u8]) -> Result<()> {
    let (dir, name) = split_path(path);
    let tar = tar_single(&name, content, 0o644)?;
    upload(client, id, &dir, tar).await
}

/// Split an absolute container path into its parent directory and file name.
pub fn split_path(path: &str) -> (String, String) {
    let trimmed = path.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(0) => ("/".to_string(), trimmed[1..].to_string()),
        Some(i) => (trimmed[..i].to_string(), trimmed[i + 1..].to_string()),
        None => (".".to_string(), trimmed.to_string()),
    }
}

/// Join a directory and an entry name into a container path.
pub fn join_path(dir: &str, name: &str) -> String {
    if dir == "/" {
        format!("/{name}")
    } else {
        format!("{}/{}", dir.trim_end_matches('/'), name)
    }
}

/// Parse one line of `ls -la` output.
///
/// GNU coreutils and BusyBox differ in spacing and in whether a group column
/// is present, so the parser works from the front (mode, links) and the back
/// (name), rather than assuming fixed column positions.
pub fn parse_ls_line(line: &str, dir: &str) -> Option<FileEntry> {
    let line = line.trim_end();
    if line.is_empty() || line.starts_with("total ") {
        return None;
    }
    let mut cols = line.split_whitespace();
    let mode = cols.next()?.to_string();
    if mode.len() < 10 {
        return None;
    }
    let _links = cols.next()?;
    let _owner = cols.next()?;
    let _group = cols.next()?;
    let size: i64 = cols.next()?.parse().unwrap_or(0);

    // The remaining columns are the timestamp (3 fields for both variants)
    // followed by the name, which may itself contain spaces.
    let rest: Vec<&str> = cols.collect();
    if rest.len() < 4 {
        return None;
    }
    let name = rest[3..].join(" ");
    // A symlink prints as "link -> target"; the entry is the link itself.
    let name = name.split(" -> ").next().unwrap_or(&name).to_string();
    if name == "." || name == ".." {
        return None;
    }

    let dir_entry = mode.starts_with('d');
    Some(FileEntry {
        path: join_path(dir, &name),
        name,
        size,
        dir: dir_entry,
        modified: 0,
        mode,
    })
}

/// List a directory inside a container.
pub async fn list_dir(client: &Client, id: &str, dir: &str) -> Result<Vec<FileEntry>> {
    let argv = vec![
        "/bin/sh".to_string(),
        "-c".to_string(),
        format!("ls -la {} 2>/dev/null || ls -la {}", shell_quote(dir), shell_quote(dir)),
    ];
    let (out, code) = exec::run_once(client, id, &argv).await?;
    if code != 0 && out.trim().is_empty() {
        return Err(DockerError::api(
            404,
            format!("Could not list {dir} — the container may have no shell."),
        ));
    }

    let mut entries: Vec<FileEntry> = out
        .lines()
        .filter_map(|l| parse_ls_line(l, dir))
        .collect();
    // Directories first, then names, so the tree reads like a file browser.
    entries.sort_by(|a, b| b.dir.cmp(&a.dir).then_with(|| a.name.cmp(&b.name)));
    Ok(entries)
}

/// Quote a path for `sh -c`.
pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Export a whole path to a tar file on the host.
pub async fn export_to(client: &Client, id: &str, path: &str, dest: &std::path::Path) -> Result<()> {
    let bytes = download(client, id, path).await?;
    let mut file = std::fs::File::create(dest)
        .map_err(|e| DockerError::transport(format!("Could not create {}: {e}", dest.display())))?;
    file.write_all(&bytes)
        .map_err(|e| DockerError::transport(format!("Could not write {}: {e}", dest.display())))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_split_into_directory_and_name() {
        assert_eq!(
            split_path("/etc/nginx/nginx.conf"),
            ("/etc/nginx".to_string(), "nginx.conf".to_string())
        );
        assert_eq!(
            split_path("/toplevel"),
            ("/".to_string(), "toplevel".to_string())
        );
        assert_eq!(
            split_path("relative"),
            (".".to_string(), "relative".to_string())
        );
    }

    #[test]
    fn a_trailing_slash_does_not_produce_an_empty_name() {
        assert_eq!(
            split_path("/var/log/"),
            ("/var".to_string(), "log".to_string())
        );
    }

    #[test]
    fn joining_does_not_double_the_root_slash() {
        assert_eq!(join_path("/", "etc"), "/etc");
        assert_eq!(join_path("/var", "log"), "/var/log");
        assert_eq!(join_path("/var/", "log"), "/var/log");
    }

    #[test]
    fn parses_gnu_coreutils_ls_output() {
        let line = "-rw-r--r-- 1 root root 1024 Jan  1 12:00 nginx.conf";
        let e = parse_ls_line(line, "/etc").unwrap();
        assert_eq!(e.name, "nginx.conf");
        assert_eq!(e.path, "/etc/nginx.conf");
        assert_eq!(e.size, 1024);
        assert!(!e.dir);
    }

    #[test]
    fn parses_busybox_ls_output() {
        let line = "drwxr-xr-x    2 root     root          4096 Jan  1 12:00 conf.d";
        let e = parse_ls_line(line, "/etc").unwrap();
        assert_eq!(e.name, "conf.d");
        assert!(e.dir);
        assert_eq!(e.size, 4096);
    }

    #[test]
    fn the_total_line_and_dot_entries_are_skipped() {
        assert!(parse_ls_line("total 48", "/etc").is_none());
        assert!(parse_ls_line("drwxr-xr-x 2 root root 4096 Jan 1 12:00 .", "/etc").is_none());
        assert!(parse_ls_line("drwxr-xr-x 2 root root 4096 Jan 1 12:00 ..", "/etc").is_none());
        assert!(parse_ls_line("", "/etc").is_none());
    }

    #[test]
    fn names_containing_spaces_survive() {
        let line = "-rw-r--r-- 1 root root 12 Jan  1 12:00 my file.txt";
        let e = parse_ls_line(line, "/data").unwrap();
        assert_eq!(e.name, "my file.txt");
        assert_eq!(e.path, "/data/my file.txt");
    }

    #[test]
    fn a_symlink_lists_as_the_link_not_its_target() {
        let line = "lrwxrwxrwx 1 root root 7 Jan  1 12:00 latest -> current";
        let e = parse_ls_line(line, "/opt").unwrap();
        assert_eq!(e.name, "latest");
        assert!(!e.dir);
    }

    #[test]
    fn a_malformed_line_is_skipped_rather_than_panicking() {
        assert!(parse_ls_line("garbage", "/etc").is_none());
        assert!(parse_ls_line("-rw-r--r-- 1 root", "/etc").is_none());
    }

    #[test]
    fn shell_quoting_survives_an_embedded_single_quote() {
        assert_eq!(shell_quote("/tmp/it's"), r"'/tmp/it'\''s'");
        assert_eq!(shell_quote("/plain"), "'/plain'");
    }

    #[test]
    fn a_single_file_tar_round_trips() {
        let content = b"hello from hopper";
        let tar = tar_single("greeting.txt", content, 0o644).unwrap();

        let mut archive = tar::Archive::new(Cursor::new(tar));
        let mut entries = archive.entries().unwrap();
        let mut entry = entries.next().unwrap().unwrap();
        assert_eq!(entry.path().unwrap().to_str().unwrap(), "greeting.txt");
        let mut buf = Vec::new();
        entry.read_to_end(&mut buf).unwrap();
        assert_eq!(buf, content);
    }

    #[test]
    fn an_empty_file_still_produces_a_valid_tar() {
        let tar = tar_single("empty", b"", 0o644).unwrap();
        let mut archive = tar::Archive::new(Cursor::new(tar));
        assert_eq!(archive.entries().unwrap().count(), 1);
    }
}
