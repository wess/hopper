typedef unsigned long word;
static long call(word n, word a, word b, word c) {
  register word x8 __asm__("x8") = n;
  register word x0 __asm__("x0") = a;
  register word x1 __asm__("x1") = b;
  register word x2 __asm__("x2") = c;
  __asm__ volatile("svc #0" : "+r"(x0) : "r"(x8), "r"(x1), "r"(x2) : "memory", "cc");
  return x0;
}
__attribute__((noreturn)) void enter(word *stack) {
  word argc = stack[0];
  char **argv = (char **)(stack + 1);
  char **env = argv + argc + 1;
  word uid = 0;
  if (argc < 3) goto failed;
  for (char *p = argv[1]; *p; p++) {
    if (*p < '0' || *p > '9') goto failed;
    uid = uid * 10 + *p - '0';
    if (uid > 65535) goto failed;
  }
  if (!uid) goto failed;
  if (call(159, 0, 0, 0)) goto failed;
  if (call(144, uid, 0, 0)) goto failed;
  if (call(146, uid, 0, 0)) goto failed;
  call(221, (word)argv[2], (word)(argv + 2), (word)env);
failed:
  call(64, 2, (word)"HOPPER_UID_SWITCH_FAILED\n", 25);
  call(93, 64, 0, 0);
  for (;;) {}
}
__asm__(".global _start\n_start:\nmov x0, sp\nbl enter\n");
