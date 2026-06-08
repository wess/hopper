// Strip Docker's stdcopy 8-byte frame headers from a raw log/exec buffer.
// Each frame: byte0=stream type, bytes1-3=0, bytes4-7=uint32 BE payload size,
// then the payload. If the bytes don't look framed (TTY mode), return as-is.

export const demux = (buf: Uint8Array): string => {
  const decoder = new TextDecoder();
  let out = "";
  let i = 0;
  while (i + 8 <= buf.length) {
    const type = buf[i];
    const framed =
      (type === 0 || type === 1 || type === 2) &&
      buf[i + 1] === 0 &&
      buf[i + 2] === 0 &&
      buf[i + 3] === 0;
    if (!framed) return decoder.decode(buf.slice(i));
    const size = (buf[i + 4]! << 24) | (buf[i + 5]! << 16) | (buf[i + 6]! << 8) | buf[i + 7]!;
    const start = i + 8;
    const end = Math.min(start + size, buf.length);
    out += decoder.decode(buf.slice(start, end));
    i = end;
  }
  if (i < buf.length) out += decoder.decode(buf.slice(i));
  return out;
};
