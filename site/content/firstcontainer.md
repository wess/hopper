---
title: Run your first container
group: Tutorials
order: 1
summary: Start Hopper Engine and serve nginx on localhost.
---

This walkthrough starts on an Apple silicon Mac with no container runtime.

## 1. Install Hopper

```sh
brew install --cask wess/packages/hopper
```

Open the app. Hopper selects its managed engine and shows its current state in
the sidebar.

## 2. Start Hopper Engine

Click **Start Hopper Engine**. On the first run Hopper downloads the pinned Linux
base, creates its private VM disk, starts Docker, and waits for the daemon. Keep
the app open until the footer says **Connected**.

Already running Docker Desktop, Colima, Rancher Desktop, or Podman? Hopper can
attach to it instead. Select the engine under Settings.

## 3. Find an image

Open **Registry** and search for `nginx`. Registry search talks to the registry
over HTTP, so it does not depend on daemon-side search.

Choose the official nginx image and click **Run**.

## 4. Configure the run

- **Name** — `web`
- **Publish ports** — `8080:80`

Leave the other fields at their defaults and click **Run**.

## 5. Verify it

The Containers view shows `web` as running with `8080→80`.

```sh
curl http://localhost:8080
```

The nginx welcome page confirms port forwarding. Open the container detail pane
to see the request in Logs, inspect live stats, browse files, or open a shell.

## 6. Clean up

Stop the container, then remove it. The image remains available for another run.

**Next:** [Move off Docker Desktop](offdockerdesktop.html) if you have an
existing local environment to bring across.
