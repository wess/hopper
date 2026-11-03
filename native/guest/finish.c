/* keep the setup port open from completion publication through host acknowledgement. */
typedef void *Handle;
typedef unsigned long Dword;
typedef unsigned long long Word;
typedef struct {
  Word internal;
  Word high;
  Dword offset;
  Dword offsethigh;
  Handle event;
} Pending;

_Static_assert(sizeof(Dword) == 4 && sizeof(Word) == 8 && sizeof(Pending) == 32,
               "Windows ARM64 ABI required");

#define API __declspec(dllimport)
API Handle CreateFileA(const char *, Dword, Dword, void *, Dword, Dword, Handle);
API Handle CreateEventA(void *, int, int, const char *);
API int WriteFile(Handle, const void *, Dword, Dword *, Pending *);
API int ReadFile(Handle, void *, Dword, Dword *, Pending *);
API int GetOverlappedResult(Handle, Pending *, Dword *, int);
API Dword GetLastError(void);
API Dword WaitForSingleObject(Handle, Dword);
API int CancelIoEx(Handle, Pending *);
API int CloseHandle(Handle);
API int ResetEvent(Handle);
API Word GetTickCount64(void);
API __declspec(noreturn) void ExitProcess(Dword);

static int complete(Handle port, Pending *pending, int immediate, Dword *bytes, Word deadline) {
  if (!immediate) {
    if (GetLastError() != 997) return 0;
    Word now = GetTickCount64();
    if (now >= deadline || WaitForSingleObject(pending->event, (Dword)(deadline - now)) != 0) {
      CancelIoEx(port, pending);
      WaitForSingleObject(pending->event, 1000);
      return 0;
    }
  }
  return GetOverlappedResult(port, pending, bytes, 0);
}

void entry(void) {
  static const char deployed[] = "HOPPERSETUP/1 deployed 11\r\n";
  static const char expected[] = "HOPPER_SHUTDOWN\r\n";
  char reply[sizeof(expected) - 1];
  Handle port = CreateFileA("\\\\.\\org.hopper.setup", 0xc0000000, 0, 0, 3, 0x40000000, 0);
  if (port == (Handle)-1) ExitProcess(1);
  Pending pending = {0};
  pending.event = CreateEventA(0, 0, 0, 0);
  if (!pending.event) ExitProcess(2);
  Dword bytes = 0;
  Word deadline = GetTickCount64() + 60000;
  if (!complete(port, &pending, WriteFile(port, deployed, sizeof(deployed) - 1, &bytes, &pending), &bytes, deadline)
      || bytes != sizeof(deployed) - 1) ExitProcess(3);
  Dword received = 0;
  while (received < sizeof(reply)) {
    if (GetTickCount64() >= deadline || !ResetEvent(pending.event)) ExitProcess(4);
    pending.internal = 0;
    pending.high = 0;
    if (!complete(port, &pending, ReadFile(port, reply + received, sizeof(reply) - received, &bytes, &pending), &bytes, deadline)
        || bytes == 0 || bytes > sizeof(reply) - received) ExitProcess(4);
    received += bytes;
  }
  for (Dword i = 0; i < sizeof(reply); ++i) {
    if (reply[i] != expected[i]) ExitProcess(5);
  }
  CloseHandle(pending.event);
  CloseHandle(port);
  ExitProcess(0);
}
