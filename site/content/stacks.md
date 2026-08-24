---
title: Compose stacks
group: Use
order: 4
summary: Reading a compose file and running it, on any engine, with no Compose binary installed.
---

Hopper implements Compose rather than shelling out to it. It reads your
`compose.yaml`, resolves every service down to a container, and creates them
itself — so a stack comes up on **Apple Containers with no Docker on the
machine at all**.

It also groups containers into projects using the `com.docker.compose.*` labels,
which is what the real Compose CLI writes. Both directions work: a stack
`docker compose up` started appears in Hopper, and a stack Hopper started
appears in `docker compose ls` on a machine that has it.

## Bringing one up

**Open compose file…** takes a `compose.yaml` or the directory holding one.
Hopper looks for `compose.yaml`, `compose.yml`, `docker-compose.yaml` and
`docker-compose.yml`, and layers a `.override.` file over the base if there is
one.

A project already in the list has **Up**, which re-reads the file its containers
remember — Compose writes the path onto every container it creates, and so does
Hopper, so a stack can be brought back up without finding the file again.

**Up** is safe to run on a stack that is already up. Every container carries a
hash of the service it was created from; one that still matches is left running
untouched, and only a service you actually edited is recreated. Running **Up**
to check on a stack will not restart your database.

**Down** stops and removes the project's containers and the networks it created.
Named volumes are kept — that is the point of a volume.

## What it reads

Services: `image`, `command`, `environment`, `env_file`, `ports`, `volumes`,
`networks`, `depends_on`, `labels`, `restart`, `working_dir`, `user`,
`hostname`, `tty`, `profiles`, `container_name`, `deploy.resources.limits`, and
the older `mem_limit` / `cpus`. Top level: `name`, `services`, `networks`,
`volumes`.

Variables expand the way Compose expands them — `${VAR}`, `${VAR:-default}`,
`${VAR:?message}`, `$$` for a literal dollar — from the process environment
overlaid on a `.env` beside the file.

Relative paths resolve against the compose file's directory, not the one Hopper
was launched from. Running Hopper from somewhere else must not change what gets
mounted.

## What it does not

Every one of these is *reported* on the service it was written on, never
silently dropped:

- **`build:`.** Hopper does not build images. A service with a `build` and no
  `image` is refused with the reason rather than half-started; one with both
  runs from the `image`.
- **`healthcheck:`.** Containers are created without one, on either engine. A
  `depends_on` with `condition: service_healthy` is honoured as ordering only —
  the service it names is started first, not waited on until healthy.
- **A second network.** A container is created on one network; the others are
  named in the output.
- **`deploy.replicas`.** One container per service.
- **`entrypoint:`.** Containers get the image's own.
- **Port ranges** like `8000-8010:8000-8010`.
- **Anonymous volumes** — a mount with no source or name.
- Anything else the file declares that Hopper has no implementation for, listed
  by key.

On Apple Containers, `restart:` is reported too, since that runtime has no
restart policy.

## Where the names come from

Compose's own, because the two have to agree on them: containers are
`project-service-1`, networks and volumes are `project_name`, and the project
is the `name:` in the file, else the directory it sits in.
