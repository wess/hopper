---
title: Run a Compose stack
group: Tutorials
order: 3
summary: A web service and a database, brought up together from the Stacks view — on any engine.
---

Hopper reads the compose file and runs the services itself, so this works on
Apple Containers with no Docker and no Compose binary anywhere on the machine.

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

Open **Stacks**, then **Open compose file…**, and pick the file or the directory
it is in.

Hopper reads it, creates the network and the `pgdata` volume, pulls both images,
and starts `db` before `web` because `depends_on` says so. Every line of that
appears in the panel as it happens.

## 3. Watch it in Hopper

The project appears in the list with both services under it.

The grouping is read from the `com.docker.compose.*` labels on the containers,
not from the YAML — which means a stack someone else started with
`docker compose up`, or one started before Hopper was open, shows up just the
same. It works the other way too: `docker compose ls` on a machine that has it
will list the project Hopper just created.

Click into **Containers** and select `db`. The **Logs** tab shows Postgres
initialising. If you got `PGDATA` wrong, this is where you will see it say so.

## 4. Drive it

Start and stop the whole project from the Stacks view, or individual services
from Containers. Both are the same containers; there is no separate state.

## 5. Tear it down

**Down** on the project row stops and removes both containers and the network.
The volume outlives it — which is the point of a volume, and the reason the
database survives a rebuild.

Press **Up** again and the stack comes back with its data. Press **Up** on a
stack that is already running and nothing is disturbed: each container carries a
hash of the service it came from, and one that still matches is left alone.
