#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use machine::ipc::{self, Output};
use model::native::{Response, Result as Reply};
use std::{
  fs::File,
  os::{fd::OwnedFd, unix::net::UnixStream},
  time::{Duration, Instant},
};

fn pair() -> (Output, UnixStream) {
  let (writer, reader) = UnixStream::pair().unwrap();
  writer.set_nonblocking(true).unwrap();
  let file = File::from(OwnedFd::from(writer));
  (Output::new(file).unwrap(), reader)
}

fn frame(id: u64) -> Response {
  Response {
    id,
    result: Reply::Frame {
      width: 1024,
      height: 1024,
      generation: 1,
    },
  }
}

fn wait(output: &mut Output) -> anyhow::Result<()> {
  let start = Instant::now();
  while output.pending()? {
    assert!(start.elapsed() < Duration::from_secs(7));
    std::thread::sleep(Duration::from_millis(1));
  }
  Ok(())
}

#[test]
fn full_frames_drain_independently_and_preserve_serial_replies() {
  let (mut output, mut reader) = pair();
  let payload: Vec<u8> = (0..1024 * 1024 * 4).map(|n| (n % 251) as u8).collect();
  let expected = payload.clone();
  let thread = std::thread::spawn(move || {
    for id in [1, 2] {
      let (reply, bytes) = ipc::read_response(&mut reader).unwrap();
      assert_eq!(reply.id, id);
      assert_eq!(bytes, expected);
    }
  });
  for id in [1, 2] {
    output.send(&frame(id), &payload).unwrap();
    // no CPU-owner callback is needed to move the payload through the socket.
    std::thread::sleep(Duration::from_millis(20));
    wait(&mut output).unwrap();
  }
  thread.join().unwrap();
}

#[test]
fn maximum_frame_keeps_its_header_and_complete_payload() {
  let (mut output, mut reader) = pair();
  let reader = std::thread::spawn(move || {
    let (reply, bytes) = ipc::read_response(&mut reader).unwrap();
    assert!(matches!(
      reply.result,
      Reply::Frame {
        width: 4096,
        height: 4096,
        ..
      }
    ));
    assert_eq!(bytes.len(), ipc::FRAME_LIMIT);
    assert!(bytes.iter().all(|byte| *byte == 0x56));
  });
  output
    .send(
      &Response {
        id: 1,
        result: Reply::Frame {
          width: 4096,
          height: 4096,
          generation: 1,
        },
      },
      &vec![0x56; ipc::FRAME_LIMIT],
    )
    .unwrap();
  wait(&mut output).unwrap();
  reader.join().unwrap();
}

#[test]
fn stalled_reader_retains_one_reply_and_drop_cancels_the_writer() {
  let (mut output, _reader) = pair();
  output.send(&frame(1), &vec![0; 1024 * 1024 * 4]).unwrap();
  std::thread::sleep(Duration::from_millis(30));
  assert!(output.pending().unwrap());
  assert!(output.send(&frame(2), &vec![0; 1024 * 1024 * 4]).is_err());
  let start = Instant::now();
  drop(output);
  assert!(start.elapsed() < Duration::from_secs(1));
}

#[test]
fn disconnected_reader_reports_failure_to_the_owner() {
  let (mut output, reader) = pair();
  output.send(&frame(1), &vec![0; 1024 * 1024 * 4]).unwrap();
  drop(reader);
  assert!(wait(&mut output).is_err());
}

#[test]
fn stalled_reader_expires_without_owner_polling() {
  let (mut output, _reader) = pair();
  output.send(&frame(1), &vec![0; 1024 * 1024 * 4]).unwrap();
  let start = Instant::now();
  assert!(wait(&mut output).is_err());
  assert!(start.elapsed() >= Duration::from_secs(5));
}

#[test]
fn invalid_payload_does_not_occupy_the_writer() {
  let (mut output, mut reader) = pair();
  assert!(output.send(&frame(1), &[0; 3]).is_err());
  assert!(!output.pending().unwrap());
  output
    .send(
      &Response {
        id: 2,
        result: Reply::Accepted {},
      },
      &[],
    )
    .unwrap();
  let (reply, bytes) = ipc::read_response(&mut reader).unwrap();
  assert_eq!(reply.id, 2);
  assert!(bytes.is_empty());
  wait(&mut output).unwrap();
}

#[test]
fn blocking_descriptors_are_rejected_before_starting_a_writer() {
  let (writer, _reader) = UnixStream::pair().unwrap();
  assert!(Output::new(File::from(OwnedFd::from(writer))).is_err());
}
