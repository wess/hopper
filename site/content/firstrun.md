---
title: First run
group: Start
order: 3
summary: What Hopper selects, how Hopper Engine starts, and what each engine state means.
---

On launch Hopper selects an engine, resolves its endpoint, and starts polling.
The sidebar footer names the provider and its current state.

## On a clean Mac

Hopper offers its managed engine instead of reporting that Docker is missing.
Choose **Start Hopper Engine**. The first start:

1. verifies the bundled Lima helper and templates;
2. downloads the pinned Linux base when it is not cached;
3. creates Hopper's private VM disk under `~/.hopper/engine/lima`;
4. applies CPU, memory, file-share, and Docker daemon settings;
5. starts the VM and waits for Docker to answer its private socket.

The footer moves through preparation, VM startup, and Docker readiness. A booted
VM with an unhealthy daemon is not shown as connected.

## If another engine is already running

Automatic selection can attach to Docker Desktop, Colima, Rancher Desktop,
Podman, or an explicit `DOCKER_HOST`. Open **Settings → Coexist or replace** to
keep using it, switch to Hopper Engine, import its workloads, or review removal
after independent backups and replacement verification.

Hopper never removes another runtime merely because you switched engines or
finished an import.

## The states

| State | Meaning |
|---|---|
| **connected** | The selected engine answered and is ready. |
| **starting** | Provisioning, boot, or a Docker readiness check is in flight. |
| **stopped** | The engine exists but is not running. Managed engines offer Start. |
| **not installed** | The selected external runtime is absent, or an optional backend can be installed. |
| **needs permission** | The endpoint exists but Hopper cannot open it. |
| **unreachable** | The endpoint exists but the engine did not answer correctly. |
| **unsupported** | This provider cannot run on the current hardware or build. |

Only states Hopper can act on get a lifecycle button. Hopper can start and stop
Hopper Engine; it does not launch or kill somebody else's desktop runtime.

## Autostart

Autostart is enabled by default for Hopper Engine. Turn it off under
**Settings → Engine** if you want the VM to remain stopped until requested.
