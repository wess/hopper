use host::Host;
use model::guest::wire::{Action, Outcome};
use serde_json::{json, Value};

pub fn catalogue() -> Vec<Value> {
  let id = json!({"type":"string", "description":"VM id from vm.list."});
  let job = json!({"type":"string", "description":"Job id returned by vm.job_start."});
  let command = json!({"type":"object", "additionalProperties":false, "properties":{
    "executable":{"type":"string", "minLength":1, "maxLength":4096},
    "args":{"type":"array", "maxItems":128, "items":{"type":"string", "maxLength":4096}},
    "role":{"enum":["normal", "administrator"]},
    "directory":{"type":["string", "null"], "maxLength":4096, "description":"Optional absolute path inside the guest."},
    "timeoutSeconds":{"type":"integer", "minimum":1, "maximum":600}
  }, "required":["executable", "args", "role", "timeoutSeconds"]});
  let file = json!({"type":"object", "additionalProperties":false, "properties":{
    "path":{"type":"string", "minLength":1, "maxLength":4096, "description":"Absolute path inside the guest."},
    "offset":{"type":"integer", "minimum":0, "maximum":model::guest::files::LIMIT},
    "length":{"type":"integer", "minimum":1, "maximum":model::guest::files::CHUNK},
    "role":{"enum":["normal", "administrator"]},
    "stamp":{"type":["object", "null"], "description":"Stamp from the previous successful read receipt; reuse it to reject a changed file."}
  }, "required":["path", "offset", "length", "role"]});
  let create = json!({"type":"object", "additionalProperties":false, "properties":{
    "path":{"type":"string", "minLength":1, "maxLength":4096, "description":"New absolute file path inside the guest; existing destinations reject."},
    "bytes":{"type":"array", "maxItems":model::guest::files::CREATE_LIMIT,
      "items":{"type":"integer", "minimum":0, "maximum":255}},
    "role":{"enum":["normal", "administrator"]}
  }, "required":["path", "bytes", "role"]});
  let upload = json!({"type":"object", "additionalProperties":false, "properties":{
    "path":{"type":"string", "minLength":1, "maxLength":4096},
    "size":{"type":"integer", "minimum":0, "maximum":model::guest::files::UPLOAD_LIMIT},
    "sha256":{"type":"string", "pattern":"^[0-9a-fA-F]{64}$"},
    "timeoutSeconds":{"type":"integer", "minimum":1, "maximum":600},
    "role":{"enum":["normal", "administrator"]}
  }, "required":["path", "size", "sha256", "timeoutSeconds", "role"]});
  let bytes = json!({"type":"array", "minItems":1, "maxItems":model::guest::files::CREATE_LIMIT,
    "items":{"type":"integer", "minimum":0, "maximum":255}});
  let browse = json!({"type":"object", "additionalProperties":false, "properties":{
    "path":{"type":"string", "minLength":1, "maxLength":4096},
    "limit":{"type":"integer", "minimum":1, "maximum":model::guest::files::DIRECTORY_LIMIT},
    "after":{"type":["array", "null"], "minItems":1, "maxItems":255, "items":{"type":"integer", "minimum":0, "maximum":255}},
    "stamp":{"type":["object", "null"], "description":"Reuse the previous listing's directory stamp to reject observed changes."},
    "role":{"enum":["normal", "administrator"]}
  }, "required":["path", "limit", "role"]});
  [
    ("vm.file_browse", "Browse up to 32 native Linux guest directory entries as an asynchronous job. Poll for successful untruncated completion with exitCode 0; stdout contains a JSON listing with raw byte-array names, entry kinds, directory stamp and next cursor. Reuse next as after and the stamp for later pages. Names sort by bytes; symlink entries are not followed. Scans are bounded to 65536 entries and larger directories reject. Requires updated guest tools and explicit role; experimental and live-unverified. No host paths.", json!({"id":id, "directory":browse}), vec!["id", "directory"]),
    ("vm.upload_begin", "Begin a native Linux upload to a new guest file, up to 1 GiB. Supply exact size, SHA-256, explicit role and timeout. Returns a transfer containing its guest job and accepted offset. Requires updated guest tools and the running Hopper app; experimental and live-unverified. No host file access or overwrite.", json!({"id":id, "file":upload}), vec!["id", "file"]),
    ("vm.upload_chunk", "Send up to 32 KiB of binary byte values at the transfer's current offset. The reply reports accepted offset; backpressure can accept only part or none. Send remaining bytes from the returned offset. Accepted bytes are queued, not published. Query vm.upload_status after an ambiguous reply; never assume the whole chunk was accepted.", json!({"id":id, "job":job, "offset":{"type":"integer", "minimum":0, "maximum":model::guest::files::UPLOAD_LIMIT}, "bytes":bytes}), vec!["id", "job", "offset", "bytes"]),
    ("vm.upload_status", "Query a native Linux upload's accepted offset, size, commit state and supervised job. State is scoped to the authenticated guest session and does not survive reconnect. Cancel using vm.job_cancel. Requires the original VM agent policy.", json!({"id":id, "job":job}), vec!["id", "job"]),
    ("vm.upload_commit", "Commit only after all declared bytes have been accepted. Retry until committed is true, then poll the job to completion. Publication additionally requires EOF and matching SHA-256. Require successful untruncated completion with exitCode 0 and a JSON stdout receipt. A committed transfer is not itself proof of publication. Cancellation/failure after commit can leave the completed file; inspect before retrying.", json!({"id":id, "job":job}), vec!["id", "job"]),
    ("vm.job_start", "Start a literal executable with arguments inside native Linux. Requires the running Hopper app and installed guest tools; experimental and live-unverified. Choose normal or administrator explicitly. Returns a job id; poll for verified launch identity and bounded byte output. No implicit shell or host execution.", json!({"id":id, "command":command}), vec!["id", "command"]),
    ("vm.job_poll", "Poll a native Linux guest job for phase, launch identity and bounded stdout/stderr bytes. Requires the running Hopper app and the original VM agent policy.", json!({"id":id, "job":job}), vec!["id", "job"]),
    ("vm.job_cancel", "Cancel a native Linux guest job and its descendant processes. Poll until cleanup completes. Requires the running Hopper app and installed guest tools.", json!({"id":id, "job":job}), vec!["id", "job"]),
    ("vm.file_read", "Read up to 64 KiB from a regular native Linux guest file as an asynchronous guest job. Poll with vm.job_poll. A successful job has exitCode 0, stdout contains raw byte values, and stderr contains a JSON receipt with size, offset, EOF and a file stamp. Reuse the stamp on later chunks. Explicit normal/administrator role; no host file access. Requires updated guest tools; experimental and live-unverified.", json!({"id":id, "file":file}), vec!["id", "file"]),
    ("vm.file_create", "Create a new native Linux guest file atomically with up to 32 KiB of binary bytes and an explicit execution role. Existing destinations are preserved and rejected. Poll the returned job: successful completion has exitCode 0 and a JSON stdout receipt. Requires updated guest tools and destination support for anonymous temporary files. After cancellation or failure, inspect the destination before retrying: publication may already have completed. Experimental and live-unverified.", json!({"id":id, "file":create}), vec!["id", "file"]),
  ].into_iter().map(|(name, description, properties, required)| json!({
    "name":name, "description":description,
    "inputSchema":{"type":"object", "properties":properties, "required":required, "additionalProperties":false}
  })).collect()
}

pub async fn call(host: &Host, name: &str, args: &Value, policy: u64) -> anyhow::Result<Value> {
  let id = super::arg(args, "id")?;
  let action = match name {
    "vm.file_browse" => {
      let directory: model::guest::files::Browse = serde_json::from_value(
        args
          .get("directory")
          .cloned()
          .ok_or_else(|| anyhow::anyhow!("Provide `directory`"))?,
      )?;
      Action::Start {
        command: directory
          .command()
          .ok_or_else(|| anyhow::anyhow!("Invalid guest directory request"))?,
      }
    }
    "vm.upload_begin" => Action::BeginUpload {
      file: serde_json::from_value(
        args
          .get("file")
          .cloned()
          .ok_or_else(|| anyhow::anyhow!("Provide `file`"))?,
      )?,
    },
    "vm.upload_chunk" => Action::UploadChunk {
      job: super::arg(args, "job")?.into(),
      offset: args
        .get("offset")
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow::anyhow!("Provide integer `offset`"))?,
      bytes: serde_json::from_value(
        args
          .get("bytes")
          .cloned()
          .ok_or_else(|| anyhow::anyhow!("Provide `bytes`"))?,
      )?,
    },
    "vm.upload_status" => Action::UploadStatus {
      job: super::arg(args, "job")?.into(),
    },
    "vm.upload_commit" => Action::CommitUpload {
      job: super::arg(args, "job")?.into(),
    },
    "vm.file_create" => Action::CreateFile {
      file: serde_json::from_value(
        args
          .get("file")
          .cloned()
          .ok_or_else(|| anyhow::anyhow!("Provide `file`"))?,
      )?,
    },
    "vm.file_read" => {
      let file: model::guest::files::Read = serde_json::from_value(
        args
          .get("file")
          .cloned()
          .ok_or_else(|| anyhow::anyhow!("Provide `file`"))?,
      )?;
      Action::Start {
        command: file
          .command()
          .ok_or_else(|| anyhow::anyhow!("Invalid guest file read"))?,
      }
    }
    "vm.job_start" => Action::Start {
      command: serde_json::from_value(
        args
          .get("command")
          .cloned()
          .ok_or_else(|| anyhow::anyhow!("Provide `command`"))?,
      )?,
    },
    "vm.job_poll" => Action::Poll {
      job: super::arg(args, "job")?.into(),
    },
    "vm.job_cancel" => Action::Cancel {
      job: super::arg(args, "job")?.into(),
    },
    _ => anyhow::bail!("Unknown guest job operation"),
  };
  #[cfg(unix)]
  match host.request_guest_job(id, policy, action).await? {
    Outcome::Job { job } => Ok(crate::protocol::text_result(serde_json::to_string(&job)?)),
    Outcome::Transfer { transfer } => Ok(crate::protocol::text_result(serde_json::to_string(
      &transfer,
    )?)),
    Outcome::Error { message } => anyhow::bail!("{message}"),
  }
  #[cfg(not(unix))]
  {
    let _ = (host, id, action);
    anyhow::bail!("Guest job transport is unavailable on this host");
  }
}
