# Image build — design notes

## Scope

Build a local image from a context folder + Dockerfile, with live build-log
output, using the daemon's classic `/build` endpoint.

## How it works

`src/host/docker/build.ts`:

1. Reads `.dockerignore` from the context dir and turns it into `tar --exclude`
   patterns (`dockerignoreExcludes()`). Comments, blanks, and negations (`!`) are
   dropped — tar can't express allow-list semantics, so negated paths are simply
   included (best-effort, noted as a limitation).
2. Packs the context with `tar -cf - -C <ctx> --exclude=… .` and hands the
   process's **stdout straight to fetch as a streaming request body** — the
   context is never buffered into memory.
3. POSTs to `/build` with `Content-Type: application/x-tar` and the query built
   by `buildQuery()` (`t`, `dockerfile`, `target`, `nocache`, `pull`,
   JSON-encoded `buildargs`).
4. Demuxes the daemon's JSON build log via `ndjson`; `mapBuildFrame()` turns each
   frame into a `BuildProgress` (`stream` log lines, `errorDetail`, and the final
   `aux.ID` image id), emitted to the UI on `buildProgress`.

## Prerequisite: raw request bodies

The build context tar is the reason `client.ts` `req()` learned to send raw /
streaming bodies and custom headers (`isRawBody`, the `headers` option, and
half-duplex mode for a `ReadableStream`). Push reuses the same header support
for `X-Registry-Auth`.

## Limitations / follow-ups

- **Classic builder only.** BuildKit (`/build?version=2` + a gRPC session) is a
  much larger surface and is intentionally out of scope for v1. Multi-stage
  `target` works fine on the classic builder.
- **`.dockerignore` negation** isn't honored (see above).
- Build args are sent as-is; they appear in the streamed build log, so don't use
  them for secrets.
