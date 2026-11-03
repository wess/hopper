mod group;
mod identity;
mod input;
mod output;
mod upload;
mod worker;

use anyhow::{ensure, Context};
use model::guest::{Command, Job, Phase};
use std::{
  collections::{HashMap, VecDeque},
  fs::File,
  path::Path,
  sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
  },
  time::Instant,
};

const HISTORY: usize = 16;
const ACTIVE: usize = 2;

pub(super) fn error(value: impl std::fmt::Display) -> String {
  let mut value = value.to_string();
  let mut end = value.len().min(512);
  while !value.is_char_boundary(end) {
    end -= 1;
  }
  value.truncate(end);
  value
}

pub(super) struct Control {
  result: Mutex<Job>,
  cancel: AtomicBool,
  done: AtomicBool,
}

pub struct Jobs {
  root: File,
  jobs: VecDeque<Arc<Control>>,
  uploads: Mutex<HashMap<String, upload::Producer>>,
}

pub fn open(root: &Path) -> anyhow::Result<Jobs> {
  Ok(Jobs {
    root: group::open(root)?,
    jobs: VecDeque::new(),
    uploads: Mutex::new(HashMap::new()),
  })
}

impl Jobs {
  pub fn start(&mut self, command: Command) -> anyhow::Result<Job> {
    self.admit(command, None)
  }

  pub fn create_file(&mut self, file: model::guest::files::Create) -> anyhow::Result<Job> {
    let command = file.command().context("Invalid guest file creation")?;
    let input = input::sealed(&file.bytes)?;
    self.admit(command, Some(input))
  }

  pub fn begin_upload(
    &mut self,
    file: model::guest::files::Upload,
  ) -> anyhow::Result<model::guest::files::Transfer> {
    let command = file.command().context("Invalid guest upload")?;
    let (producer, input) = upload::prepare(&file)?;
    let job = self.admit(command, Some(input))?;
    let mut uploads = self.uploads.lock().unwrap();
    uploads.retain(|id, _| {
      self
        .jobs
        .iter()
        .any(|control| control.result.lock().unwrap().id == *id)
    });
    let transfer = model::guest::files::Transfer {
      offset: producer.offset,
      size: producer.size,
      committed: false,
      job,
    };
    uploads.insert(transfer.job.id.clone(), producer);
    Ok(transfer)
  }

  pub fn upload_chunk(
    &self,
    id: &str,
    offset: u64,
    bytes: &[u8],
  ) -> anyhow::Result<model::guest::files::Transfer> {
    let job = self.poll(id, false)?;
    ensure!(!job.finished, "Guest upload job has finished");
    let mut uploads = self.uploads.lock().unwrap();
    self.upload_active(id)?;
    uploads
      .get_mut(id)
      .context("Guest upload is unavailable")?
      .append(offset, bytes)?;
    drop(uploads);
    self.upload_status(id)
  }

  pub fn commit_upload(&self, id: &str) -> anyhow::Result<model::guest::files::Transfer> {
    let job = self.poll(id, false)?;
    let mut uploads = self.uploads.lock().unwrap();
    let producer = uploads.get_mut(id).context("Guest upload is unavailable")?;
    if !producer.committed {
      self.upload_active(id)?;
    }
    ensure!(
      !job.finished || producer.committed,
      "Guest upload job has finished"
    );
    producer.commit()?;
    drop(uploads);
    self.upload_status(id)
  }

  pub fn upload_status(&self, id: &str) -> anyhow::Result<model::guest::files::Transfer> {
    let job = self.poll(id, false)?;
    let uploads = self.uploads.lock().unwrap();
    let producer = uploads.get(id).context("Guest upload is unavailable")?;
    Ok(model::guest::files::Transfer {
      job,
      offset: producer.offset,
      size: producer.size,
      committed: producer.committed,
    })
  }

  fn upload_active(&self, id: &str) -> anyhow::Result<()> {
    let control = self
      .jobs
      .iter()
      .find(|control| control.result.lock().unwrap().id == id)
      .context("Guest upload job is unavailable")?;
    ensure!(
      !control.cancel.load(Ordering::Acquire) && !control.done.load(Ordering::Acquire),
      "Guest upload job is cancelled or finished"
    );
    Ok(())
  }

  fn admit(&mut self, command: Command, input: Option<File>) -> anyhow::Result<Job> {
    let accepted = Instant::now();
    ensure!(
      command.valid_for(model::GuestOs::Linux),
      "Guest command exceeds bounds or has invalid fields"
    );
    ensure!(
      self
        .jobs
        .iter()
        .filter(|job| !job.done.load(Ordering::Acquire))
        .count()
        < ACTIVE,
      "Guest job capacity is busy"
    );
    let (identity, home) = identity::resolve(command.role)?;
    let id = model::new_uuid();
    let group = group::create(&self.root, &id)?;
    if self.jobs.len() == HISTORY {
      let oldest = self
        .jobs
        .iter()
        .position(|job| job.done.load(Ordering::Acquire))
        .context("Guest job history is busy")?;
      self.jobs.remove(oldest);
    }
    let result = Job {
      id,
      identity: None,
      phase: Phase::Starting,
      finished: false,
      exit_code: None,
      signal: None,
      stdout: Vec::new(),
      stderr: Vec::new(),
      stdout_truncated: false,
      stderr_truncated: false,
      error: None,
      cleanup_error: None,
    };
    let control = Arc::new(Control {
      result: Mutex::new(result.clone()),
      cancel: AtomicBool::new(false),
      done: AtomicBool::new(false),
    });
    let worker = control.clone();
    std::thread::Builder::new()
      .name("hopperjob".into())
      .spawn(move || worker::run(worker, command, identity, home, group, accepted, input))?;
    self.jobs.push_back(control);
    Ok(result)
  }

  pub fn poll(&self, id: &str, cancel: bool) -> anyhow::Result<Job> {
    ensure!(id.len() == 36, "Guest job ID is invalid");
    let job = self
      .jobs
      .iter()
      .find(|job| job.result.lock().unwrap().id == id)
      .context("Guest job is unavailable")?;
    if cancel {
      job.cancel.store(true, Ordering::Release);
    }
    let result = job.result.lock().unwrap().clone();
    if cancel || result.finished {
      if let Some(producer) = self.uploads.lock().unwrap().get_mut(id) {
        producer.close();
      }
    }
    Ok(result)
  }

  pub fn cancel_all(&self) {
    for job in &self.jobs {
      job.cancel.store(true, Ordering::Release);
    }
    for producer in self.uploads.lock().unwrap().values_mut() {
      producer.close();
    }
  }

  pub fn idle(&self) -> bool {
    self.jobs.iter().all(|job| job.done.load(Ordering::Acquire))
  }
}

impl Drop for Jobs {
  fn drop(&mut self) {
    self.cancel_all();
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use model::guest::{files::Upload, Role};
  use std::io::Read;

  fn fixture() -> (Jobs, String, File) {
    let file = Upload {
      path: "/home/hopper/new".into(),
      size: 3,
      sha256: "ab".repeat(32),
      timeout_seconds: 30,
      role: Role::Normal,
    };
    let (mut producer, input) = upload::prepare(&file).unwrap();
    producer.append(0, b"abc").unwrap();
    let id = model::new_uuid();
    let control = Arc::new(Control {
      result: Mutex::new(Job {
        id: id.clone(),
        identity: None,
        phase: Phase::Starting,
        finished: false,
        exit_code: None,
        signal: None,
        stdout: Vec::new(),
        stderr: Vec::new(),
        stdout_truncated: false,
        stderr_truncated: false,
        error: None,
        cleanup_error: None,
      }),
      cancel: AtomicBool::new(false),
      done: AtomicBool::new(false),
    });
    let jobs = Jobs {
      root: File::open("/dev/null").unwrap(),
      jobs: VecDeque::from([control]),
      uploads: Mutex::new(HashMap::from([(id.clone(), producer)])),
    };
    (jobs, id, input)
  }

  fn uncommitted_eof(input: &mut File) {
    let mut bytes = Vec::new();
    input.read_to_end(&mut bytes).unwrap();
    assert_eq!(&bytes[..32], &[0xab; 32]);
    assert_eq!(&bytes[32..], b"abc");
  }

  #[test]
  fn cancellation_closes_even_a_fully_queued_upload_and_rejects_commit() {
    let (jobs, id, mut input) = fixture();
    jobs.poll(&id, true).unwrap();
    assert!(jobs.jobs[0].cancel.load(Ordering::Acquire));
    assert!(jobs.commit_upload(&id).is_err());
    assert!(jobs.upload_chunk(&id, 3, b"x").is_err());
    let status = jobs.upload_status(&id).unwrap();
    assert_eq!(status.offset, 3);
    assert!(!status.committed);
    uncommitted_eof(&mut input);
  }

  #[test]
  fn service_teardown_marks_jobs_and_closes_uncommitted_channels() {
    let (jobs, id, mut input) = fixture();
    jobs.cancel_all();
    assert!(jobs.jobs[0].cancel.load(Ordering::Acquire));
    assert!(jobs.commit_upload(&id).is_err());
    uncommitted_eof(&mut input);
  }

  #[test]
  fn cancellation_intent_rejects_commit_before_channel_cleanup() {
    let (jobs, id, mut input) = fixture();
    jobs.jobs[0].cancel.store(true, Ordering::Release);
    assert!(jobs.commit_upload(&id).is_err());
    assert!(!jobs.upload_status(&id).unwrap().committed);
    jobs.cancel_all();
    uncommitted_eof(&mut input);
  }

  #[test]
  fn finished_job_closes_its_producer_without_claiming_commit() {
    let (jobs, id, mut input) = fixture();
    {
      let mut result = jobs.jobs[0].result.lock().unwrap();
      result.finished = true;
      result.phase = Phase::TimedOut;
    }
    let status = jobs.upload_status(&id).unwrap();
    assert!(status.job.finished && !status.committed);
    assert!(jobs.commit_upload(&id).is_err());
    uncommitted_eof(&mut input);
  }
}
