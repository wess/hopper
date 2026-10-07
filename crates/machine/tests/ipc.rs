use machine::ipc::{self, Stream};
use model::native::{Command, Request, Response, Result as Reply};
use std::io::{Cursor, ErrorKind, Read, Write};

fn capture(id: u64) -> Request {
  Request {
    id,
    command: Command::Capture {},
  }
}

struct Reader {
  bytes: Cursor<Vec<u8>>,
  available: usize,
}

impl Read for Reader {
  fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
    if self.available == 0 {
      return Err(ErrorKind::WouldBlock.into());
    }
    let length = bytes.len().min(self.available).min(3);
    let read = self.bytes.read(&mut bytes[..length])?;
    self.available -= read;
    Ok(read)
  }
}

struct Writer {
  bytes: Vec<u8>,
  available: usize,
}

impl Write for Writer {
  fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
    if self.available == 0 {
      return Err(ErrorKind::WouldBlock.into());
    }
    let length = bytes.len().min(self.available).min(7);
    self.bytes.extend(&bytes[..length]);
    self.available -= length;
    Ok(length)
  }
  fn flush(&mut self) -> std::io::Result<()> {
    Ok(())
  }
}

#[test]
fn fragmented_commands_preserve_order_and_never_read_a_later_request() {
  let first = ipc::encode(&capture(1), &[]).unwrap();
  let second = ipc::encode(&capture(2), &[]).unwrap();
  let mut reader = Reader {
    bytes: Cursor::new([first.clone(), second].concat()),
    available: 5,
  };
  let mut stream = Stream::default();
  assert!(ipc::receive(&mut stream, &mut reader).unwrap().is_none());
  reader.available = 7;
  assert!(ipc::receive(&mut stream, &mut reader).unwrap().is_none());
  reader.available = usize::MAX;
  assert_eq!(
    ipc::receive(&mut stream, &mut reader).unwrap().unwrap().id,
    1
  );
  assert_eq!(reader.bytes.position(), first.len() as u64);
  assert_eq!(
    ipc::receive(&mut stream, &mut reader).unwrap().unwrap().id,
    2
  );
  assert!(ipc::receive(&mut stream, &mut reader).is_err());
}

#[test]
fn invalid_versions_bounds_partial_eof_and_reserved_ids_are_rejected() {
  for prefix in [
    [0; 8],
    [b'H', b'P', b'V', b'1', 0, 0, 0, 0],
    [b'H', b'P', b'V', b'1', 1, 64, 0, 0],
  ] {
    assert!(ipc::header_length(&prefix).is_err());
    assert!(ipc::receive(&mut Stream::default(), &mut Cursor::new(prefix)).is_err());
  }
  let packet = ipc::encode(&capture(1), &[]).unwrap();
  for length in [1, 7, 8, packet.len() - 1] {
    assert!(ipc::receive(&mut Stream::default(), &mut Cursor::new(&packet[..length])).is_err());
  }
  assert!(ipc::receive(
    &mut Stream::default(),
    &mut Cursor::new(ipc::encode(&capture(0), &[]).unwrap())
  )
  .is_err());
  let json = serde_json::json!({"id": 1, "command": {"type": "capture", "hostPath": "/private"}});
  assert!(ipc::receive(
    &mut Stream::default(),
    &mut Cursor::new(ipc::encode(&json, &[]).unwrap())
  )
  .is_err());
}

#[test]
fn stalled_replies_resume_without_duplication_and_reject_another_reply() {
  let response = Response {
    id: 3,
    result: Reply::Frame {
      width: 2,
      height: 1,
      generation: 9,
    },
  };
  let pixels = [0, 0, 0, 255, 255, 32, 0, 255];
  let mut stream = Stream::default();
  let mut writer = Writer {
    bytes: Vec::new(),
    available: 5,
  };
  ipc::send(&mut stream, &response, &pixels).unwrap();
  assert_eq!(stream.flush(&mut writer).unwrap(), 5);
  assert_eq!(stream.flush(&mut writer).unwrap(), 0);
  assert!(stream.pending());
  assert!(ipc::send(
    &mut stream,
    &Response {
      id: 4,
      result: Reply::Accepted {}
    },
    &[]
  )
  .is_err());
  writer.available = usize::MAX;
  stream.flush(&mut writer).unwrap();
  assert!(!stream.pending());
  let mut reader = Cursor::new(writer.bytes);
  let (read, payload) = ipc::read_response(&mut reader).unwrap();
  assert_eq!(read.id, 3);
  assert!(matches!(read.result, Reply::Frame { generation: 9, .. }));
  assert_eq!(payload, pixels);
  assert_eq!(reader.position(), reader.get_ref().len() as u64);
}

#[test]
fn frame_limits_and_mismatched_payloads_fail_before_publication() {
  for (width, height) in [(0, 1), (1, 0), (4097, 1), (1, u32::MAX)] {
    assert!(ipc::frame_size(width, height).is_err());
  }
  assert_eq!(ipc::frame_size(4096, 4096).unwrap(), ipc::FRAME_LIMIT);
  let mut stream = Stream::default();
  let frame = Response {
    id: 1,
    result: Reply::Frame {
      width: 1,
      height: 1,
      generation: 0,
    },
  };
  assert!(ipc::send(&mut stream, &frame, &[0; 3]).is_err());
  assert!(!stream.pending());
  assert!(ipc::send(
    &mut stream,
    &Response {
      id: 1,
      result: Reply::Accepted {}
    },
    &[1]
  )
  .is_err());
  let bytes = ipc::encode(&frame, &[0; 3]).unwrap();
  assert!(ipc::read_response(&mut Cursor::new(bytes)).is_err());
}

#[test]
fn flush_yields_after_a_bounded_write_budget() {
  let mut stream = Stream::default();
  let response = Response {
    id: 1,
    result: Reply::Frame {
      width: 256,
      height: 256,
      generation: 1,
    },
  };
  ipc::send(&mut stream, &response, &vec![0; 256 * 256 * 4]).unwrap();
  let mut bytes = Vec::new();
  assert_eq!(stream.flush(&mut bytes).unwrap(), 64 * 1024);
  assert!(stream.pending());
  while stream.pending() {
    stream.flush(&mut bytes).unwrap();
  }
  assert_eq!(
    ipc::read_response(&mut Cursor::new(bytes)).unwrap().1.len(),
    256 * 256 * 4
  );
}
