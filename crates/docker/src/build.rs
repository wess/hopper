//! Image builds.
//!
//! The daemon wants the build context as a tar stream, so the work here is
//! mostly assembling that: walk the context directory, apply `.dockerignore`,
//! and always keep the Dockerfile itself even when a pattern would exclude it.

use crate::client::{Client, Req};
use crate::error::{DockerError, Result};
#[cfg(test)]
use bytes::Bytes;
use model::{BuildInput, BuildProgress};
use serde::Deserialize;
#[cfg(test)]
use std::path::{Path, PathBuf};

pub use crate::context::{is_excluded, load_dockerignore, tar_context};

/// The query string for a build request.
pub fn build_request(input: &BuildInput) -> Req {
    let mut req = Req::post("/build")
        .query("dockerfile", input.dockerfile_name())
        .flag("nocache", input.no_cache)
        .flag("pull", input.pull)
        .query("rm", "1")
        .no_timeout();

    if let Some(tag) = input.tag.as_deref().filter(|t| !t.trim().is_empty()) {
        req = req.query("t", tag);
    }
    if let Some(target) = input.target.as_deref().filter(|t| !t.trim().is_empty()) {
        req = req.query("target", target);
    }
    if let Some(platform) = input.platform.as_deref().filter(|p| !p.trim().is_empty()) {
        req = req.query("platform", platform);
    }
    if !input.build_args.is_empty() {
        let args = serde_json::to_string(&input.build_args).unwrap_or_else(|_| "{}".into());
        req = req.query("buildargs", args);
    }
    req
}

#[derive(Debug, Deserialize, Default)]
struct RawBuild {
    stream: Option<String>,
    status: Option<String>,
    error: Option<String>,
    aux: Option<Aux>,
}

#[derive(Debug, Deserialize, Default)]
struct Aux {
    #[serde(rename = "ID")]
    id: Option<String>,
}

/// Run a build, streaming the daemon's log lines to `on_progress`.
pub async fn run<F>(
    client: &Client,
    request_id: &str,
    input: &BuildInput,
    mut on_progress: F,
) -> Result<Option<String>>
where
    F: FnMut(BuildProgress),
{
    let crate::context::ContextStream {
        receiver,
        worker,
        cancel,
        finished,
    } = crate::context::stream_context(input.clone());
    let req = build_request(input).stream_body(receiver, "application/x-tar");

    let mut error: Option<String> = None;
    let mut image_id: Option<String> = None;

    let remote = client
        .ndjson::<RawBuild, _>(req, |frame| {
            if let Some(e) = frame.error {
                on_progress(BuildProgress {
                    request_id: request_id.to_string(),
                    status: Some(e.clone()),
                    done: true,
                    error: Some(e.clone()),
                    ..Default::default()
                });
                error = Some(e);
                return true;
            }
            if let Some(id) = frame.aux.and_then(|a| a.id) {
                image_id = Some(id);
            }
            on_progress(BuildProgress {
                request_id: request_id.to_string(),
                stream: frame.stream,
                status: frame.status,
                image_id: image_id.clone(),
                done: false,
                error: None,
            });
            true
        })
        .await;
    let locally_finished = finished.load(std::sync::atomic::Ordering::Acquire);
    drop(cancel);
    let local = worker
        .await
        .map_err(|e| DockerError::transport(format!("Build context worker failed: {e}")))?;
    if locally_finished {
        local?;
        remote?;
    } else {
        remote?;
        local?;
    }

    on_progress(BuildProgress {
        request_id: request_id.to_string(),
        image_id: image_id.clone(),
        done: true,
        error: error.clone(),
        ..Default::default()
    });

    match error {
        Some(e) => Err(DockerError::api(400, e)),
        None => Ok(image_id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::io::Read;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hopperbuild{}{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(dir: &Path, rel: &str, content: &str) {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, content).unwrap();
    }

    fn names_in(tar: &Bytes) -> Vec<String> {
        let mut archive = tar::Archive::new(std::io::Cursor::new(tar.clone()));
        archive
            .entries()
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path().unwrap().to_string_lossy().to_string())
            .collect()
    }

    #[test]
    fn a_context_without_a_dockerfile_is_refused_with_a_clear_message() {
        let dir = scratch("nodockerfile");
        write(&dir, "main.rs", "fn main() {}");
        let input = BuildInput {
            context_dir: dir.to_string_lossy().to_string(),
            ..Default::default()
        };
        let err = tar_context(&input).unwrap_err();
        assert!(err.message.contains("No Dockerfile"));
    }

    #[test]
    fn a_missing_context_directory_is_refused() {
        let input = BuildInput {
            context_dir: "/nonexistent/hopper/context".into(),
            ..Default::default()
        };
        assert!(tar_context(&input)
            .unwrap_err()
            .message
            .contains("not a directory"));
    }

    #[test]
    fn the_context_tar_contains_the_projects_files() {
        let dir = scratch("basic");
        write(&dir, "Dockerfile", "FROM scratch");
        write(&dir, "src/main.rs", "fn main() {}");
        let input = BuildInput {
            context_dir: dir.to_string_lossy().to_string(),
            ..Default::default()
        };
        let names = names_in(&tar_context(&input).unwrap());
        assert!(names.contains(&"Dockerfile".to_string()));
        assert!(names.contains(&"src/main.rs".to_string()));
    }

    #[test]
    fn dockerignore_patterns_are_applied() {
        let dir = scratch("ignore");
        write(&dir, "Dockerfile", "FROM scratch");
        write(&dir, "keep.txt", "keep");
        write(&dir, "secret.env", "SECRET=1");
        write(&dir, "node_modules/dep/index.js", "//");
        write(&dir, ".dockerignore", "*.env\nnode_modules\n");

        let input = BuildInput {
            context_dir: dir.to_string_lossy().to_string(),
            ..Default::default()
        };
        let names = names_in(&tar_context(&input).unwrap());
        assert!(names.contains(&"keep.txt".to_string()));
        assert!(
            !names.iter().any(|n| n.contains("secret.env")),
            "an ignored file must not be uploaded: {names:?}"
        );
        assert!(
            !names.iter().any(|n| n.contains("node_modules")),
            "an ignored directory must not be uploaded: {names:?}"
        );
    }

    #[test]
    fn a_dockerignore_negation_re_includes_a_file() {
        let dir = scratch("negation");
        write(&dir, "Dockerfile", "FROM scratch");
        write(&dir, "config/a.json", "{}");
        write(&dir, "config/keep.json", "{}");
        write(&dir, ".dockerignore", "config/*\n!config/keep.json\n");

        let input = BuildInput {
            context_dir: dir.to_string_lossy().to_string(),
            ..Default::default()
        };
        let names = names_in(&tar_context(&input).unwrap());
        assert!(
            names.contains(&"config/keep.json".to_string()),
            "the negation should win: {names:?}"
        );
        assert!(!names.contains(&"config/a.json".to_string()));
    }

    #[test]
    fn the_dockerfile_survives_a_catch_all_ignore_pattern() {
        let dir = scratch("catchall");
        write(&dir, "Dockerfile", "FROM scratch");
        write(&dir, "other.txt", "x");
        write(&dir, ".dockerignore", "*\n");

        let input = BuildInput {
            context_dir: dir.to_string_lossy().to_string(),
            ..Default::default()
        };
        let names = names_in(&tar_context(&input).unwrap());
        // Without this the build fails with a baffling "Dockerfile not found".
        assert!(names.contains(&"Dockerfile".to_string()));
        assert!(!names.contains(&"other.txt".to_string()));
    }

    #[test]
    fn a_custom_dockerfile_name_is_honored_and_kept() {
        let dir = scratch("custom");
        write(&dir, "Dockerfile.prod", "FROM scratch");
        write(&dir, ".dockerignore", "*\n");

        let input = BuildInput {
            context_dir: dir.to_string_lossy().to_string(),
            dockerfile: Some("Dockerfile.prod".into()),
            ..Default::default()
        };
        let names = names_in(&tar_context(&input).unwrap());
        assert!(names.contains(&"Dockerfile.prod".to_string()));
    }

    #[test]
    fn build_args_and_platform_reach_the_query_string() {
        let mut args = BTreeMap::new();
        args.insert("VERSION".to_string(), "1.2.3".to_string());
        let input = BuildInput {
            context_dir: ".".into(),
            tag: Some("app:latest".into()),
            target: Some("runtime".into()),
            platform: Some("linux/amd64".into()),
            build_args: args,
            no_cache: true,
            ..Default::default()
        };
        // Rendering goes through the client, so assert on what it produces.
        let client = Client::new(crate::Endpoint::default());
        let rendered = crate::client::render_for_test(&client, &build_request(&input));
        assert!(rendered.contains("t=app%3Alatest"));
        assert!(rendered.contains("target=runtime"));
        assert!(rendered.contains("platform=linux%2Famd64"));
        assert!(rendered.contains("nocache=1"));
        assert!(rendered.contains("VERSION"));
    }

    #[test]
    fn an_empty_tag_is_omitted_rather_than_sent_blank() {
        let input = BuildInput {
            context_dir: ".".into(),
            tag: Some("  ".into()),
            ..Default::default()
        };
        let client = Client::new(crate::Endpoint::default());
        let rendered = crate::client::render_for_test(&client, &build_request(&input));
        assert!(!rendered.contains("t="));
    }

    #[test]
    fn a_file_read_back_out_of_the_context_keeps_its_bytes() {
        let dir = scratch("content");
        write(&dir, "Dockerfile", "FROM scratch\nCOPY . /app\n");
        let input = BuildInput {
            context_dir: dir.to_string_lossy().to_string(),
            ..Default::default()
        };
        let tar = tar_context(&input).unwrap();
        let mut archive = tar::Archive::new(std::io::Cursor::new(tar));
        let mut found = String::new();
        for entry in archive.entries().unwrap() {
            let mut entry = entry.unwrap();
            if entry.path().unwrap().to_str() == Some("Dockerfile") {
                entry.read_to_string(&mut found).unwrap();
            }
        }
        assert_eq!(found, "FROM scratch\nCOPY . /app\n");
    }
}
