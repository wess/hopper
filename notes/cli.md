# Docker CLI

Hopper exposes its managed engine on:

```bash
unix://$HOME/.hopper/run/docker.sock
```

That socket is enough for normal shell tooling:

```bash
export DOCKER_HOST="unix://$HOME/.hopper/run/docker.sock"
docker info
docker compose version
```

The Settings view has a Docker CLI panel that creates or updates a Docker
context named `hopper` and switches the Docker CLI to it:

```bash
docker context create hopper --docker "host=unix://$HOME/.hopper/run/docker.sock"
docker context use hopper
```

Once the context is active, scripts that call `docker` or `docker compose`
target Hopper instead of Docker Desktop.
