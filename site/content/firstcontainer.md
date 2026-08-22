---
title: Run your first container
group: Tutorials
order: 1
summary: From a fresh install to nginx serving on localhost, in about five minutes.
---

This assumes a Mac with nothing container-shaped on it yet.

## 1. Install Hopper

```sh
brew install --cask wess/packages/hopper
```

Open it. The sidebar footer will say what it found.

## 2. Get an engine

On a clean macOS 26 machine, Hopper will say:

> **Run containers natively on this Mac**
> Apple Containers is not installed yet.

Click **Install Apple Containers**. Hopper downloads the package Apple signed and
hands it to the system installer; macOS asks for your password. Approve it.

When it finishes, Hopper starts the services itself. The first start pulls a
kernel image, so it takes a moment. The footer goes **starting → connected**.

> Already running Docker Desktop or Colima? Hopper will just connect to it and
> you can skip this step entirely.

## 3. Find an image

Go to **Registry** and search for `nginx`. This search hits Docker Hub over
HTTP, so it works even before an engine is up.

Click **Run** on the official nginx result.

## 4. Configure the run

The run dialog opens with the image filled in. Set:

- **Name** — `web`
- **Publish ports** — `8080:80`

Leave the rest. Click **Run**.

## 5. See it

Go to **Containers**. `web` is there, `running`, showing `8080→80`.

```sh
curl http://localhost:8080
```

You should get nginx's welcome page.

Click the row. The detail pane opens with **Logs** streaming — you will see your
own `curl` show up as an access log line.

## 6. Clean up

**Stop** on the row, then **Delete**.

---

## What just happened

On Apple Containers, `web` is not a process sharing your kernel — it is its own
lightweight virtual machine with its own network address, and macOS forwarded
port 8080 to it. That is stronger isolation than a traditional container runtime
gives you, for roughly the same startup cost.

**Next:** [Move off Docker Desktop](offdockerdesktop.html) if you have an
existing setup to bring across.
