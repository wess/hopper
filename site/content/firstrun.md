---
title: First run
group: Start
order: 3
summary: What happens when you open Hopper, and what to do if no engine answers.
---

On launch Hopper picks an engine, points itself at it, and starts polling. The
status footer in the sidebar tells you where it got to.

## If Apple Containers is not installed

You get an offer rather than an error:

> **Run containers natively on this Mac**
> Apple Containers is not installed yet.
> *Hopper can download Apple's signed installer for you; macOS will ask you to
> approve it.*

Clicking **Install Apple Containers** resolves Apple's current release, downloads
the signed `.pkg`, and opens it. macOS takes over from there and asks for your
administrator password — Hopper never asks for it and never elevates.

Once the package is in, Hopper starts the services itself. That first start
fetches a kernel image, so give it a moment.

## If you already run Docker

Nothing to do. Hopper finds the socket and connects, and the footer says which
daemon answered — `Connected to Docker 29.7.2.` or similar.

You can still switch to Apple Containers later, and
[Import from Docker](import.html) will bring your images across when you do.

## The states you might see

| State | What it means |
|---|---|
| **connected** | An engine answered. Everything works. |
| **starting** | Hopper is bringing the engine up, or the first probe is in flight. |
| **stopped** | The engine is installed but not running. There is a Start button. |
| **not installed** | No engine here, and one can be offered or installed. |
| **needs permission** | The socket exists but Hopper is not allowed to open it. On Linux, usually the `docker` group. |
| **unreachable** | Something is listening but not answering properly. |
| **unsupported** | This engine cannot run on this machine — for example Apple Containers on macOS 15. |

Only the states Hopper can actually act on offer a button. A stopped engine that
Hopper manages gets **Start**; a missing Docker does not, because starting it is
not Hopper's to do.

## Autostart

On by default: if the selected engine is one Hopper manages and it is not
running, Hopper starts it in the background. Turn it off under
**Settings → Engine**.
