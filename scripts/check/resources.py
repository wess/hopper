#!/usr/bin/env python3
"""Read-only sampling for Hopper, Lima and explicitly included VM processes.

Example: resources.py --pid 123 --lima-home /tmp/hoppervm --extra-pid 456 --seconds 30
CPU comes from process-time deltas, not ps's lifetime-average percentage.
RSS is summed (shared pages may be counted twice); guest allocation and the
system's VM/kernel overhead are not interchangeable with this RSS metric.
"""
import argparse
import json
import pathlib
import statistics
import subprocess
import time


def cpu_seconds(value):
    days, _, clock = value.rpartition("-")
    fields = [float(part) for part in (clock or value).split(":")]
    seconds = 0
    for part in fields:
        seconds = seconds * 60 + part
    return seconds + (int(days) * 86400 if days else 0)


def snapshot(root_pid, home, extra_pids=()):
    raw = subprocess.check_output(["ps", "-axo", "pid=,ppid=,rss=,time=,command="], text=True)
    processes = {}
    selected = {root_pid, *extra_pids}
    for line in raw.splitlines():
        fields = line.split(None, 4)
        if len(fields) != 5:
            continue
        pid, parent, rss, cpu, command = fields
        processes[int(pid)] = {"parent": int(parent), "rssKiB": int(rss), "cpuSeconds": cpu_seconds(cpu), "executable": command.split()[0]}
        if home and home in command and "limactl hostagent" in command:
            selected.add(int(pid))
    while True:
        descendants = {pid for pid, proc in processes.items() if proc["parent"] in selected}
        if descendants.issubset(selected):
            break
        selected.update(descendants)
    return {pid: processes[pid] for pid in selected if pid in processes}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pid", type=int, required=True)
    parser.add_argument("--lima-home", type=pathlib.Path)
    parser.add_argument("--extra-pid", type=int, action="append", default=[], help="Include a verified out-of-tree VM/XPC process; repeat as needed.")
    parser.add_argument("--seconds", type=int, default=30)
    parser.add_argument("--output", type=pathlib.Path, required=True)
    args = parser.parse_args()
    if not 2 <= args.seconds <= 3600:
        parser.error("seconds must be between 2 and 3600")
    home = str(args.lima_home.resolve()) if args.lima_home else None
    baseline = snapshot(args.pid, home, args.extra_pid)
    if args.pid not in baseline:
        raise SystemExit("App PID is not running")
    if not set(args.extra_pid).issubset(baseline):
        raise SystemExit("An explicitly included VM/XPC process is not running")
    started = previous_at = time.monotonic()
    previous = baseline
    samples = []
    for _ in range(args.seconds):
        time.sleep(1)
        current = snapshot(args.pid, home, args.extra_pid)
        at = time.monotonic()
        if args.pid not in current:
            raise SystemExit("App exited during measurement")
        if not set(args.extra_pid).issubset(current) or any(current[pid]["executable"] != baseline[pid]["executable"] for pid in args.extra_pid):
            raise SystemExit("An included VM/XPC process exited or changed identity during measurement")
        elapsed = at - previous_at
        cpu = sum(max(0, proc["cpuSeconds"] - previous[pid]["cpuSeconds"]) for pid, proc in current.items() if pid in previous)
        samples.append({"elapsedSeconds": round(at - started, 3), "rssKiB": sum(p["rssKiB"] for p in current.values()), "cpuPercent": round(100 * cpu / elapsed, 3), "processes": current})
        previous, previous_at = current, at
    result = {"scope": "app, Lima/VZ hostagent, explicit extra VM/XPC processes, and their live descendants", "extraPids": args.extra_pid, "notes": ["RSS can double-count shared pages and does not measure all host VM/kernel overhead.", "Apple's Virtualization.VirtualMachine XPC service is outside the process tree. Its verified PID must be supplied with --extra-pid for a full-engine observation.", "CPU of children that exit between samples is not counted. Use Instruments or platform profiling for short subprocess workloads.", "This captures a workload observation; it does not establish a Docker Desktop comparison."], "durationSeconds": round(time.monotonic() - started, 3), "medianRssKiB": statistics.median(s["rssKiB"] for s in samples), "peakRssKiB": max(s["rssKiB"] for s in samples), "meanCpuPercent": statistics.mean(s["cpuPercent"] for s in samples), "samples": samples}
    if args.lima_home:
        files = [p.stat() for p in args.lima_home.rglob("*") if p.is_file() and not p.is_symlink()]
        result["vmHomeAllocatedBytes"] = sum(getattr(s, "st_blocks", 0) * 512 for s in files)
        result["vmHomeLogicalBytes"] = sum(s.st_size for s in files)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps({key: value for key, value in result.items() if key not in ["samples", "notes"]}, indent=2))


if __name__ == "__main__":
    main()
