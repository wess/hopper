use machine::smccc::call;

#[test]
fn entropy_is_packed_low_bits_first_and_unused_bits_are_zero() {
  let mut fill = |bytes: &mut [u8]| {
    bytes.fill(0xff);
    Ok(())
  };
  assert_eq!(call(0xc4000053, 65, &mut fill), Some([0, 0, 1, u64::MAX]));
  assert_eq!(
    call(0x84000053, 96, &mut fill),
    Some([0, 0xffffffff, 0xffffffff, 0xffffffff])
  );
  assert_eq!(call(0xc4000053, 1, &mut fill), Some([0, 0, 0, 1]));
}

#[test]
fn invalid_requests_do_not_consume_entropy() {
  let mut fill =
    |_: &mut [u8]| -> anyhow::Result<()> { panic!("Invalid request reached entropy source") };
  for (command, bits) in [(0xc4000053, 0), (0xc4000053, 193), (0x84000053, 97)] {
    assert_eq!(
      call(command, bits, &mut fill),
      Some([(-2i64) as u64, 0, 0, 0])
    );
  }
  assert_eq!(call(0x80000000, 0, &mut fill), Some([0x10001, 0, 0, 0]));
}

#[test]
fn source_failure_reports_no_entropy_without_returning_partial_bytes() {
  let mut fill = |bytes: &mut [u8]| {
    bytes.fill(0xff);
    anyhow::bail!("Unavailable")
  };
  assert_eq!(
    call(0xc4000053, 192, &mut fill),
    Some([(-3i64) as u64, 0, 0, 0])
  );
}
