use machine::setup::{self, Decoder, Event, Phase, Status};

#[test]
fn fragmented_phases_finish_as_deployed_without_claiming_desktop_readiness() {
  let mut decoder = Decoder::default();
  let mut events = Vec::new();
  let mut stream = String::new();
  for phase in 0..12 {
    stream.push_str(&format!("HOPPERSETUP/1 phase {phase}\r\n"));
  }
  stream.push_str("HOPPERSETUP/1 deployed 11\r\n");
  for byte in stream.bytes() {
    events.extend(setup::feed(&mut decoder, &[byte]).unwrap());
  }
  assert_eq!(events.len(), 13);
  assert_eq!(events.first(), Some(&Event::Phase(Phase::Files)));
  assert_eq!(events.last(), Some(&Event::Deployed));
  assert_eq!(setup::finish(&mut decoder).unwrap(), Status::Deployed);
}

#[test]
fn failure_is_bound_to_the_current_phase_and_is_terminal() {
  let mut decoder = Decoder::default();
  let bytes = b"HOPPERSETUP/1 phase 0\r\nHOPPERSETUP/1 phase 0\r\nHOPPERSETUP/1 phase 1\r\nHOPPERSETUP/1 failed 1\r\n";
  assert_eq!(setup::feed(&mut decoder, bytes).unwrap().len(), 4);
  assert_eq!(
    setup::finish(&mut decoder).unwrap(),
    Status::Failed(Phase::Pe)
  );
  assert!(setup::feed(&mut decoder, b"HOPPERSETUP/1 phase 2\r\n").is_err());
  assert_eq!(setup::status(&decoder), Status::Invalid);
}

#[test]
fn skipped_reversed_and_premature_success_events_poison_the_stream() {
  for event in [
    "HOPPERSETUP/1 phase 2\r\n",
    "HOPPERSETUP/1 failed 1\r\n",
    "HOPPERSETUP/1 deployed 11\r\n",
  ] {
    let mut decoder = Decoder::default();
    setup::feed(&mut decoder, b"HOPPERSETUP/1 phase 0\r\n").unwrap();
    assert!(setup::feed(&mut decoder, event.as_bytes()).is_err());
    assert_eq!(setup::status(&decoder), Status::Invalid);
    assert!(setup::feed(&mut decoder, b"HOPPERSETUP/1 phase 1\r\n").is_err());
  }
  let mut decoder = Decoder::default();
  setup::feed(
    &mut decoder,
    b"HOPPERSETUP/1 phase 0\r\nHOPPERSETUP/1 phase 1\r\n",
  )
  .unwrap();
  assert!(setup::feed(&mut decoder, b"HOPPERSETUP/1 phase 0\r\n").is_err());
}

#[test]
fn malformed_or_truncated_events_never_finish_and_errors_do_not_echo_guest_data() {
  for event in [
    b"private guest output\r\n".as_slice(),
    b"HOPPERSETUP/2 phase 0\r\n",
    b"HOPPERSETUP/1 phase 0\n",
    b"HOPPERSETUP/1 phase 00\r\n",
    b"HOPPERSETUP/1 phase 12\r\n",
    b"HOPPERSETUP/1 ready 0\r\n",
    b"HOPPERSETUP/1 phase \xff\r\n",
  ] {
    let mut decoder = Decoder::default();
    let error = setup::feed(&mut decoder, event).unwrap_err();
    assert!(!error.to_string().contains("private guest output"));
    assert_eq!(setup::status(&decoder), Status::Invalid);
    assert!(setup::finish(&mut decoder).is_err());
  }
  for bytes in [
    b"".as_slice(),
    b"HOPPERSETUP/1 phase 0\r\n",
    b"HOPPERSETUP/1 phase 0\r",
  ] {
    let mut decoder = Decoder::default();
    setup::feed(&mut decoder, bytes).unwrap();
    assert!(setup::finish(&mut decoder).is_err());
    assert_eq!(setup::status(&decoder), Status::Invalid);
  }
}

#[test]
fn line_batch_bounds_and_bytes_after_completion_invalidate_the_result() {
  for bytes in [vec![b'x'; 65], vec![b'x'; 4097]] {
    let mut decoder = Decoder::default();
    assert!(setup::feed(&mut decoder, &bytes).is_err());
    assert_eq!(setup::status(&decoder), Status::Invalid);
  }
  let mut decoder = Decoder::default();
  for phase in 0..12 {
    setup::feed(
      &mut decoder,
      format!("HOPPERSETUP/1 phase {phase}\r\n").as_bytes(),
    )
    .unwrap();
  }
  assert!(setup::feed(&mut decoder, b"HOPPERSETUP/1 deployed 11\r\nprivate\r\n").is_err());
  assert_eq!(setup::status(&decoder), Status::Invalid);
}
