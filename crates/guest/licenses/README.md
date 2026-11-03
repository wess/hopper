The complete `musl.txt` copyright document comes from the official musl 1.2.5
release archive, retrieved over HTTPS on 2026-10-12:

https://www.musl-libc.org/releases/musl-1.2.5.tar.gz

Archive SHA-256: `a9a118bbe84d8764da0ea0d28b3ab3fae8477fc7e4085d90102b8596fc7c75e4`.
COPYRIGHT SHA-256: `f9bc4423732350eb0b3f7ed7e91d530298476f8fec0c6c427a1c04ade22655af`.
These hashes record the retrieved bytes, not an independently signed upstream checksum.

Rust 1.99.0 builds its musl target from 1.2.5 with the security patches listed in
https://github.com/rust-lang/rust/blob/1.99.0/src/ci/docker/scripts/musl.sh.
The generator includes the entire notice, including its BSD and public-domain
component terms, alongside crate and standard-library notices. It checks the
reviewed toolchain and notice digest instead of silently applying this inventory
to a different toolchain. Review the linkage and notices before changing that pin.

This accounts for the guest helper's musl notice; it does not establish the full
application's redistribution inventory or audit other bundled components.
