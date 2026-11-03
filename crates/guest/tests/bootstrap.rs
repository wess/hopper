use guest::bootstrap::{number, port};

#[test]
fn device_names_are_literal_and_bounded() {
  for name in ["vport0p0", "vport1p123", "vport4294967295p1"] {
    assert!(port(name));
  }
  for name in [
    "",
    "vportp0",
    "vport0p",
    "vport01p0",
    "vport0p01",
    "vport0p0/other",
    "vport0p0\0",
    "vport4294967296p0",
    "../vport0p0",
    "vport0p0p0",
    "vport０p0",
  ] {
    assert!(!port(name), "{name:?}");
  }
}

#[test]
fn sysfs_numbers_are_canonical_and_no_extra_content_is_accepted() {
  assert_eq!(number("241:0\n").unwrap(), (241, 0));
  assert_eq!(number("0:4294967295").unwrap(), (0, u32::MAX));
  for value in [
    "",
    "1",
    "1:2:3",
    "01:2",
    "1:02",
    "-1:2",
    "+1:2",
    "1: 2",
    "1:2\n\n",
    "1:4294967296",
    "1:2\0",
  ] {
    assert!(number(value).is_err(), "{value:?}");
  }
}
