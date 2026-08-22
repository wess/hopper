---
title: Run a Compose stack
group: Tutorials
order: 3
summary: A web service and a database, brought up together and managed from the Stacks view.
---

Compose needs an engine that has Compose, which means **a Docker or Podman
engine, not Apple Containers**. See [Stacks](stacks.html) for why.

Switch under **Settings → Engine** if you are on Apple's runtime.

## 1. A stack to run

```yaml
# compose.yaml
services:
  db:
    image: postgres:17-alpine
    environment:
      POSTGRES_PASSWORD: example
      PGDATA: /var/lib/postgresql/data/pgdata
    volumes:
      - pgdata:/var/lib/postgresql/data
  web:
    image: nginx:alpine
    ports:
      - "8080:80"
    depends_on:
      - db

volumes:
  pgdata:
```

The `PGDATA` line is not decoration. A named volume can arrive with a
`lost+found` in it, and Postgres refuses to initialise into a non-empty
directory. Pointing at a subdirectory sidesteps it, and costs nothing when the
volume is empty.

## 2. Bring it up

```sh
docker compose up -d
```

Hopper bundles Compose, so this works even after Docker Desktop is gone — see
[Docker CLI](cli.html) for putting it on your `PATH`.

## 3. Watch it in Hopper

Open **Stacks**. The project appears with both services under it.

Hopper reconstructs this grouping from the `com.docker.compose.*` labels Compose
writes onto each container — it is reading what is actually running, not parsing
your YAML. Which means a stack someone else started, or one started before Hopper
was open, shows up just the same.

Click into **Containers** and select `db`. The **Logs** tab shows Postgres
initialising. If you got `PGDATA` wrong, this is where you will see it say so.

## 4. Drive it

Start and stop the whole project from the Stacks view, or individual services
from Containers. Both are the same containers; there is no separate state.

## 5. Tear it down

```sh
docker compose down          # keeps the volume
docker compose down -v       # takes it too
```

The volume outlives `down` unless you ask otherwise — which is the point of a
volume, and the reason a database survives a rebuild.
