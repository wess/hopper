typedef unsigned long word;
static long call(word n, word a, word b, word c) {
  register word x8 __asm__("x8") = n;
  register word x0 __asm__("x0") = a;
  register word x1 __asm__("x1") = b;
  register word x2 __asm__("x2") = c;
  __asm__ volatile("svc #0" : "+r"(x0) : "r"(x8), "r"(x1), "r"(x2) : "memory", "cc");
  return x0;
}
struct address {
  unsigned short family, reserved;
  unsigned int port, cid;
  unsigned char flags, zero[3];
};
__attribute__((noreturn)) void enter(void) {
  struct address peer = {40, 0, 6200, 2, 0, {0, 0, 0}};
  char reply[3];
  long fd;
  if (call(159, 0, 0, 0) || call(144, 1001, 0, 0) || call(146, 1001, 0, 0)) goto failed;
  if (call(174, 0, 0, 0) != 1001) goto failed;
  fd = call(198, 40, 1, 0);
  if (fd < 0 || call(203, fd, (word)&peer, sizeof(peer))) goto failed;
  if (call(64, fd, (word)"guest-1001\n", 11) != 11) goto failed;
  for (word n = 0; n < sizeof(reply);) {
    long count = call(63, fd, (word)(reply + n), sizeof(reply) - n);
    if (count <= 0) goto failed;
    n += count;
  }
  if (reply[0] != 'o' || reply[1] != 'k' || reply[2] != '\n') goto failed;
  call(57, fd, 0, 0);
  call(64, 1, (word)"HOPPER_SOCKET_GUEST_OK\n", 23);
  call(93, 0, 0, 0);
failed:
  call(64, 2, (word)"HOPPER_SOCKET_GUEST_FAILED\n", 27);
  call(93, 1, 0, 0);
  for (;;) {}
}
__asm__(".global _start\n_start:\nbl enter\n");
