//! Turning a parsed compose file into containers an engine can create.
//!
//! Everything here is pure over the spec, so the whole translation — ports,
//! mounts, ordering, and every "this engine will not do that" — is unit-tested
//! without a daemon. That matters more here than anywhere else in Hopper: a
//! wrong mount path or a dropped environment variable produces a stack that
//! comes up and then misbehaves, which is far harder to trace back than one
//! that refuses to start.
//!
//! The rule the whole module follows: never drop something the file asked for
//! without saying so. A key that cannot be honoured becomes a warning, and a
//! service that cannot run at all becomes `blocked` with the reason.

use crate::file::{self, File, PortEntry, Service, VolumeEntry};
use crate::names;
use crate::parse::Loaded;
use model::{
    ComposeNetwork, ComposePlan, ComposePlanService, ComposeVolume, EngineCapabilities,
    PortMapping, ResourceLimits, RunInput, VolumeMapping,
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// What the caller wants out of this plan.
///
/// Not to be confused with `model::ComposeOptions`, which is the flag set a
/// compose *command* takes. This is the input to planning.
#[derive(Clone, Debug, Default)]
pub struct PlanOptions {
    /// `-p`, overriding the file and the directory.
    pub project: Option<String>,
    /// `--profile`, repeatable. A service in no profile always runs.
    pub profiles: Vec<String>,
    /// Restrict the run to these services and what they depend on. Empty means
    /// the whole project.
    pub only: Vec<String>,
}

/// Build the plan.
///
/// `caps` decides what counts as a warning: the same file planned against
/// Docker and against Apple's runtime produces the same containers, and a
/// different list of things that will not be honoured.
pub fn build(loaded: &Loaded, opts: &PlanOptions, caps: &EngineCapabilities) -> Result<ComposePlan, String> {
    let file = &loaded.file;
    let project = names::project(
        opts.project.as_deref(),
        file.name.as_deref(),
        &loaded.project_dir,
    );

    let mut warnings = loaded.warnings.clone();
    for key in file.extra.keys() {
        warnings.push(format!(
            "`{key}` at the top level is not something Hopper implements, so it was ignored."
        ));
    }

    let networks = plan_networks(file, &project);
    let volumes = plan_volumes(file, &project);
    let known_volumes: BTreeMap<String, String> = file
        .volumes
        .iter()
        .map(|(key, def)| (key.clone(), volume_name(&project, key, def.as_ref())))
        .collect();
    let known_networks: BTreeMap<String, String> = file
        .networks
        .iter()
        .map(|(key, def)| (key.clone(), network_name(&project, key, def.as_ref())))
        .collect();

    let mut services: Vec<ComposePlanService> = Vec::new();
    for (name, service) in &file.services {
        services.push(plan_service(
            name,
            service,
            &project,
            loaded,
            &known_networks,
            &known_volumes,
            caps,
        ));
    }

    // Profiles decide who is in this run; `only` narrows it further, pulling
    // in dependencies so a selected service does not start without its
    // database.
    let active: BTreeSet<&str> = opts.profiles.iter().map(String::as_str).collect();
    for s in &mut services {
        s.selected = s.profiles.is_empty() || s.profiles.iter().any(|p| active.contains(p.as_str()));
    }
    if !opts.only.is_empty() {
        let wanted = with_dependencies(&services, &opts.only);
        for s in &mut services {
            s.selected = s.selected && wanted.contains(&s.service);
        }
    }

    let services = order(services)?;

    Ok(ComposePlan {
        project,
        working_dir: loaded.project_dir.to_string_lossy().to_string(),
        config_files: loaded.config_files.clone(),
        networks,
        volumes,
        services,
        warnings,
    })
}

/// The name a declared network or volume ends up with on the engine.
///
/// `external: { name: shared }` names it outright; `external: true` means the
/// key is already the real name. Anything else is scoped to the project.
fn declared_name(
    project: &str,
    key: &str,
    name: Option<&String>,
    external: Option<&file::External>,
) -> String {
    if let Some(ext) = external {
        return ext.name().unwrap_or(key).to_string();
    }
    name.cloned().unwrap_or_else(|| names::scoped(project, key))
}

fn network_name(project: &str, key: &str, def: Option<&file::NetworkDef>) -> String {
    declared_name(
        project,
        key,
        def.and_then(|d| d.name.as_ref()),
        def.and_then(|d| d.external.as_ref()),
    )
}

fn volume_name(project: &str, key: &str, def: Option<&file::VolumeDef>) -> String {
    declared_name(
        project,
        key,
        def.and_then(|d| d.name.as_ref()),
        def.and_then(|d| d.external.as_ref()),
    )
}

fn plan_networks(file: &File, project: &str) -> Vec<ComposeNetwork> {
    let mut out: Vec<ComposeNetwork> = file
        .networks
        .iter()
        .map(|(key, def)| ComposeNetwork {
            name: network_name(project, key, def.as_ref()),
            external: def
                .as_ref()
                .and_then(|d| d.external.as_ref())
                .is_some_and(|e| e.is_external()),
            internal: def.as_ref().and_then(|d| d.internal).unwrap_or(false),
        })
        .collect();

    // Compose puts every service that declares no network of its own onto one
    // it creates for the project. Without it, services cannot resolve each
    // other by name — which is the single thing people expect compose to do.
    let needs_default = file
        .services
        .values()
        .any(|s| s.networks.as_ref().is_none_or(|n| n.names().is_empty()));
    let default = names::default_network(project);
    if needs_default && !out.iter().any(|n| n.name == default) {
        out.insert(
            0,
            ComposeNetwork {
                name: default,
                external: false,
                internal: false,
            },
        );
    }
    out
}

fn plan_volumes(file: &File, project: &str) -> Vec<ComposeVolume> {
    file.volumes
        .iter()
        .map(|(key, def)| ComposeVolume {
            name: volume_name(project, key, def.as_ref()),
            external: def
                .as_ref()
                .and_then(|d| d.external.as_ref())
                .is_some_and(|e| e.is_external()),
        })
        .collect()
}

/// Keys Hopper reads elsewhere or deliberately treats as a no-op, so the
/// catch-all does not warn about them.
const QUIET_KEYS: [&str; 3] = ["expose", "platform", "pull_policy"];

fn plan_service(
    name: &str,
    service: &Service,
    project: &str,
    loaded: &Loaded,
    known_networks: &BTreeMap<String, String>,
    known_volumes: &BTreeMap<String, String>,
    caps: &EngineCapabilities,
) -> ComposePlanService {
    let mut warnings: Vec<String> = Vec::new();
    let mut blocked: Option<String> = None;

    for key in service.extra.keys() {
        if QUIET_KEYS.contains(&key.as_str()) {
            continue;
        }
        warnings.push(format!(
            "`{key}` is not something Hopper implements, so it was not applied."
        ));
    }

    // An image is the one thing a container cannot be created without.
    let image = match (&service.image, &service.build) {
        (Some(image), None) => image.clone(),
        (Some(image), Some(_)) => {
            warnings.push(
                "`build` was ignored — Hopper does not build images yet, so the `image` was used as it is."
                    .into(),
            );
            image.clone()
        }
        (None, Some(_)) => {
            blocked = Some(
                "this service is built from a Dockerfile, and Hopper does not build images yet. Build it yourself and add an `image:` to the file."
                    .into(),
            );
            String::new()
        }
        (None, None) => {
            blocked = Some("this service has neither an `image` nor a `build`.".into());
            String::new()
        }
    };

    let (env, env_warnings) = plan_env(service, loaded);
    warnings.extend(env_warnings);

    let (ports, port_warnings) = plan_ports(service);
    warnings.extend(port_warnings);

    let (volumes, volume_warnings) =
        plan_mounts(service, &loaded.project_dir, known_volumes);
    warnings.extend(volume_warnings);

    // One network at create time is all a `run` expresses; the rest are
    // reported so the caller can attach them or say it cannot.
    let attach: Vec<String> = service
        .networks
        .as_ref()
        .map(|n| n.names())
        .unwrap_or_default()
        .iter()
        .map(|key| {
            known_networks
                .get(key)
                .cloned()
                .unwrap_or_else(|| names::scoped(project, key))
        })
        .collect();
    let (network, extra_networks) = match attach.split_first() {
        Some((first, rest)) => (Some(first.clone()), rest.to_vec()),
        None => (Some(names::default_network(project)), Vec::new()),
    };

    let depends_on: Vec<String> = service
        .depends_on
        .clone()
        .map(|d| {
            d.into_pairs()
                .into_iter()
                .map(|(svc, condition)| {
                    // Ordering is all Hopper enforces. It starts `svc` first
                    // and moves on — it does not wait for a healthcheck to
                    // pass, on either engine, so this is never honoured as
                    // written and saying otherwise would be the lie.
                    if condition.as_deref() == Some("service_healthy") {
                        warnings.push(format!(
                            "`{svc}` is waited for until it starts, not until it is healthy — Hopper does not run healthchecks."
                        ));
                    }
                    svc
                })
                .collect()
        })
        .unwrap_or_default();

    // Not gated on the engine: Hopper creates containers without a healthcheck
    // whichever backend is answering, so a file that declares one loses it
    // either way.
    if service.healthcheck.as_ref().is_some_and(|h| !h.disable.unwrap_or(false)) {
        warnings.push("`healthcheck` was not applied — Hopper does not create containers with one.".into());
    }
    if service
        .restart
        .as_deref()
        .is_some_and(|r| !r.is_empty() && r != "no")
        && !caps.restart_policy
    {
        warnings.push(format!(
            "`restart: {}` was not applied — this engine has no restart policy.",
            service.restart.clone().unwrap_or_default()
        ));
    }
    if service.entrypoint.is_some() {
        warnings.push(
            "`entrypoint` was not applied — Hopper creates containers with the image's own entrypoint."
                .into(),
        );
    }
    if let Some(deploy) = &service.deploy {
        if deploy.replicas.unwrap_or(1) > 1 {
            warnings.push(
                "`deploy.replicas` was not applied — Hopper starts one container per service."
                    .into(),
            );
        }
    }

    let mut run = RunInput {
        image,
        name: Some(
            service
                .container_name
                .clone()
                .unwrap_or_else(|| names::container(project, name, 1)),
        ),
        env,
        ports,
        volumes,
        command: plan_command(service),
        restart: service.restart.clone(),
        auto_remove: false,
        network,
        workdir: service.working_dir.clone(),
        user: service.user.clone(),
        hostname: service.hostname.clone(),
        limits: plan_limits(service, &mut warnings),
        labels: user_labels(service, &mut warnings),
        tty: service.tty.unwrap_or(false),
    };
    // The hash covers the container as resolved, so it has to be taken before
    // the compose labels — one of which is the hash itself.
    let hash = names::config_hash(&canonical(&run));
    stamp(&mut run.labels, project, name, &hash, &depends_on, loaded);

    ComposePlanService {
        service: name.to_string(),
        run,
        depends_on,
        profiles: service.profiles.clone(),
        selected: true,
        extra_networks,
        blocked,
        warnings,
    }
}

/// `command:` as the single line `RunInput` carries.
///
/// A list form is re-joined with quoting so `["sh", "-c", "echo hi there"]`
/// survives the round trip through the splitter on the other side.
fn plan_command(service: &Service) -> Option<String> {
    let parts = service.command.clone()?.into_vec();
    match parts.len() {
        0 => None,
        1 => Some(parts[0].clone()),
        _ => Some(
            parts
                .iter()
                .map(|p| {
                    if p.contains(' ') || p.contains('"') {
                        format!("\"{}\"", p.replace('"', "\\\""))
                    } else {
                        p.clone()
                    }
                })
                .collect::<Vec<_>>()
                .join(" "),
        ),
    }
}

/// `environment:` and `env_file:`, in compose's precedence: the file's own
/// `environment` block wins over anything an env file supplied.
fn plan_env(service: &Service, loaded: &Loaded) -> (Vec<String>, Vec<String>) {
    let mut warnings = Vec::new();
    let mut pairs: Vec<(String, String)> = Vec::new();

    for (path, required) in service
        .env_file
        .clone()
        .map(file::EnvFile::into_paths)
        .unwrap_or_default()
    {
        let resolved = resolve_path(&loaded.project_dir, &path);
        match std::fs::read_to_string(&resolved) {
            Ok(text) => {
                for (k, v) in crate::interpolate::parse_env(&text) {
                    pairs.retain(|(existing, _)| existing != &k);
                    pairs.push((k, v));
                }
            }
            Err(e) if required => {
                warnings.push(format!(
                    "`env_file` {} could not be read ({e}), so its variables are missing.",
                    resolved.display()
                ));
            }
            Err(_) => {}
        }
    }

    for (key, value) in service
        .environment
        .clone()
        .map(file::MapOrList::into_pairs)
        .unwrap_or_default()
    {
        // A bare `FOO` takes its value from the environment Hopper is in.
        let value = match value {
            Some(v) => Some(v),
            None => loaded.vars.get(&key).cloned(),
        };
        let Some(value) = value else {
            warnings.push(format!(
                "`{key}` was to be passed through from the environment, which does not have it, so it was not set."
            ));
            continue;
        };
        pairs.retain(|(existing, _)| existing != &key);
        pairs.push((key, value));
    }

    (
        pairs.into_iter().map(|(k, v)| format!("{k}={v}")).collect(),
        warnings,
    )
}

fn plan_ports(service: &Service) -> (Vec<PortMapping>, Vec<String>) {
    let mut out = Vec::new();
    let mut warnings = Vec::new();

    for entry in &service.ports {
        match entry {
            PortEntry::Long(long) => {
                let Some(target) = file::scalar(&long.target) else {
                    warnings.push("a `ports` entry had no usable `target`.".into());
                    continue;
                };
                let published = long
                    .published
                    .as_ref()
                    .and_then(file::scalar)
                    .unwrap_or_else(|| target.clone());
                let host = match &long.host_ip {
                    Some(ip) if !ip.is_empty() => format!("{ip}:{published}"),
                    _ => published,
                };
                out.push(PortMapping {
                    host,
                    container: target,
                    proto: long.protocol.clone(),
                });
            }
            PortEntry::Short(value) => {
                let Some(text) = file::scalar(value) else {
                    warnings.push("a `ports` entry was not a port.".into());
                    continue;
                };
                match short_port(&text) {
                    Ok(mapping) => out.push(mapping),
                    Err(reason) => warnings.push(reason),
                }
            }
        }
    }
    (out, warnings)
}

/// `8080:80`, `127.0.0.1:8080:80`, `80/udp`, `3000`.
fn short_port(text: &str) -> Result<PortMapping, String> {
    let (body, proto) = match text.split_once('/') {
        Some((body, proto)) => (body, Some(proto.to_string())),
        None => (text, None),
    };
    if body.contains('-') {
        return Err(format!(
            "`{text}` is a port range, which Hopper does not expand — publish the ports individually."
        ));
    }

    let parts: Vec<&str> = body.split(':').collect();
    let (host, container) = match parts.as_slice() {
        // A bare container port publishes on a random host port under compose.
        // Hopper has no way to ask for one, so it reuses the same number rather
        // than leaving the port unreachable.
        [single] => {
            return Ok(PortMapping {
                host: (*single).to_string(),
                container: (*single).to_string(),
                proto,
            })
        }
        [host, container] => ((*host).to_string(), (*container).to_string()),
        [ip, host, container] => (format!("{ip}:{host}"), (*container).to_string()),
        _ => return Err(format!("`{text}` is not a port mapping Hopper understands.")),
    };
    Ok(PortMapping {
        host,
        container,
        proto,
    })
}

fn plan_mounts(
    service: &Service,
    project_dir: &Path,
    known_volumes: &BTreeMap<String, String>,
) -> (Vec<VolumeMapping>, Vec<String>) {
    let mut out = Vec::new();
    let mut warnings = Vec::new();

    for entry in &service.volumes {
        match entry {
            VolumeEntry::Long(long) => {
                for key in long.extra.keys() {
                    warnings.push(format!(
                        "`{key}` on the mount at {} is not something Hopper implements.",
                        long.target
                    ));
                }
                let Some(source) = &long.source else {
                    warnings.push(format!(
                        "the mount at {} has no source, so it was skipped — Hopper does not create anonymous volumes.",
                        long.target
                    ));
                    continue;
                };
                out.push(VolumeMapping {
                    host: mount_source(source, project_dir, known_volumes),
                    container: long.target.clone(),
                    ro: long.read_only.unwrap_or(false),
                });
            }
            VolumeEntry::Short(text) => match short_mount(text, project_dir, known_volumes) {
                Ok(mapping) => out.push(mapping),
                Err(reason) => warnings.push(reason),
            },
        }
    }
    (out, warnings)
}

/// A mount source is either a named volume the file declared or a host path.
///
/// A relative path resolves against the compose file's directory, not the
/// process's — running Hopper from elsewhere must not change what gets mounted.
fn mount_source(
    source: &str,
    project_dir: &Path,
    known_volumes: &BTreeMap<String, String>,
) -> String {
    if let Some(name) = known_volumes.get(source) {
        return name.clone();
    }
    if source.starts_with('.') || source.starts_with('/') || source.starts_with('~') {
        return resolve_path(project_dir, source).to_string_lossy().to_string();
    }
    source.to_string()
}

fn short_mount(
    text: &str,
    project_dir: &Path,
    known_volumes: &BTreeMap<String, String>,
) -> Result<VolumeMapping, String> {
    // Windows-style `C:\path:/target` is not something Hopper handles, and
    // splitting it naively would produce a mount from `C`.
    let parts: Vec<&str> = text.split(':').collect();
    match parts.as_slice() {
        [target] => Err(format!(
            "`{target}` is an anonymous volume, which Hopper does not create — give it a name or a host path."
        )),
        [source, target] => Ok(VolumeMapping {
            host: mount_source(source, project_dir, known_volumes),
            container: (*target).to_string(),
            ro: false,
        }),
        [source, target, mode] => Ok(VolumeMapping {
            host: mount_source(source, project_dir, known_volumes),
            container: (*target).to_string(),
            ro: mode.split(',').any(|m| m == "ro"),
        }),
        _ => Err(format!("`{text}` is not a mount Hopper understands.")),
    }
}

fn resolve_path(project_dir: &Path, path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    let p = Path::new(path);
    if p.is_absolute() {
        return p.to_path_buf();
    }
    // `./src` and `src` both mean "beside the compose file".
    normalize(&project_dir.join(p))
}

/// Collapse `.` and `..` without touching the filesystem, so a path that does
/// not exist yet still comes out readable rather than as `/srv/app/./../data`.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Just the labels the file asked for.
fn user_labels(service: &Service, warnings: &mut Vec<String>) -> BTreeMap<String, String> {
    let mut labels: BTreeMap<String, String> = BTreeMap::new();
    for (key, value) in service
        .labels
        .clone()
        .map(file::MapOrList::into_pairs)
        .unwrap_or_default()
    {
        match value {
            Some(v) => {
                labels.insert(key, v);
            }
            None => warnings.push(format!("the label `{key}` has no value, so it was skipped.")),
        }
    }
    labels
}

/// Add the labels that make this a compose stack.
///
/// They go on last and win over anything the file wrote: they are how Hopper,
/// and the real `docker compose`, find these containers again.
fn stamp(
    labels: &mut BTreeMap<String, String>,
    project: &str,
    name: &str,
    hash: &str,
    depends_on: &[String],
    loaded: &Loaded,
) {
    labels.insert(names::PROJECT.into(), project.to_string());
    labels.insert(names::SERVICE.into(), name.to_string());
    labels.insert(names::NUMBER.into(), "1".into());
    labels.insert(names::ONEOFF.into(), "False".into());
    labels.insert(names::CONFIG_HASH.into(), hash.to_string());
    labels.insert(names::DEPENDS_ON.into(), depends_on.join(","));
    labels.insert(names::WORKING_DIR.into(), loaded.project_dir.to_string_lossy().to_string());
    labels.insert(names::CONFIG_FILES.into(), loaded.config_files.join(","));
}

/// One line standing for everything about a container that, if it changed,
/// should mean the container is rebuilt.
///
/// Written by hand rather than serialized so that adding a field to `RunInput`
/// cannot silently invalidate every stack anyone has running.
fn canonical(run: &RunInput) -> String {
    let mounts: Vec<String> = run
        .volumes
        .iter()
        .map(|v| format!("{}:{}:{}", v.host, v.container, v.ro))
        .collect();
    let ports: Vec<String> = run
        .ports
        .iter()
        .map(|p| format!("{}:{}/{}", p.host, p.container, p.proto.clone().unwrap_or_default()))
        .collect();
    let labels: Vec<String> = run.labels.iter().map(|(k, v)| format!("{k}={v}")).collect();
    [
        run.image.clone(),
        run.name.clone().unwrap_or_default(),
        run.command.clone().unwrap_or_default(),
        run.network.clone().unwrap_or_default(),
        run.workdir.clone().unwrap_or_default(),
        run.user.clone().unwrap_or_default(),
        run.hostname.clone().unwrap_or_default(),
        run.restart.clone().unwrap_or_default(),
        run.tty.to_string(),
        run.env.join(","),
        ports.join(","),
        mounts.join(","),
        labels.join(","),
        format!("{:?}", run.limits),
    ]
    .join("|")
}

fn plan_limits(service: &Service, warnings: &mut Vec<String>) -> ResourceLimits {
    let mut limits = ResourceLimits::default();

    let resources = service.deploy.as_ref().and_then(|d| d.resources.as_ref());
    if let Some(l) = resources.and_then(|r| r.limits.as_ref()) {
        limits.cpus = l.cpus.as_ref().and_then(cpus);
        limits.memory = l.memory.as_ref().and_then(bytes);
    }
    if let Some(r) = resources.and_then(|r| r.reservations.as_ref()) {
        limits.memory_reservation = r.memory.as_ref().and_then(bytes);
    }
    // The pre-Specification keys, still in plenty of files. They lose to
    // `deploy` when both are present, which is what compose does.
    if limits.cpus.is_none() {
        limits.cpus = service.cpus.as_ref().and_then(cpus);
    }
    if limits.memory.is_none() {
        limits.memory = service.mem_limit.as_ref().and_then(bytes);
    }
    if service.mem_limit.is_some() && limits.memory.is_none() {
        warnings.push("`mem_limit` was not a size Hopper could read, so no memory cap was set.".into());
    }
    limits
}

fn cpus(value: &serde_yaml::Value) -> Option<f64> {
    file::scalar(value)?.trim().parse::<f64>().ok()
}

/// `512m`, `1g`, `1gb`, `1024`, or a plain YAML number.
fn bytes(value: &serde_yaml::Value) -> Option<u64> {
    let text = file::scalar(value)?;
    let text = text.trim().to_lowercase();
    let (digits, unit) = text.split_at(text.find(|c: char| !c.is_ascii_digit()).unwrap_or(text.len()));
    let n: u64 = digits.parse().ok()?;
    let scale = match unit.trim() {
        "" | "b" => 1,
        "k" | "kb" => 1024,
        "m" | "mb" => 1024 * 1024,
        "g" | "gb" => 1024 * 1024 * 1024,
        _ => return None,
    };
    Some(n * scale)
}

/// The named services plus everything they depend on, transitively.
fn with_dependencies(services: &[ComposePlanService], only: &[String]) -> BTreeSet<String> {
    let by_name: BTreeMap<&str, &ComposePlanService> =
        services.iter().map(|s| (s.service.as_str(), s)).collect();
    let mut wanted: BTreeSet<String> = BTreeSet::new();
    let mut queue: Vec<String> = only.to_vec();
    while let Some(name) = queue.pop() {
        if !wanted.insert(name.clone()) {
            continue;
        }
        if let Some(service) = by_name.get(name.as_str()) {
            queue.extend(service.depends_on.iter().cloned());
        }
    }
    wanted
}

/// Sort services so nothing starts before what it depends on.
///
/// Ties break alphabetically rather than by hash order, so the same file
/// always produces the same start order and a failure is reproducible.
fn order(services: Vec<ComposePlanService>) -> Result<Vec<ComposePlanService>, String> {
    let names: BTreeSet<String> = services.iter().map(|s| s.service.clone()).collect();
    let mut pending: BTreeMap<String, BTreeSet<String>> = services
        .iter()
        .map(|s| {
            (
                s.service.clone(),
                s.depends_on
                    .iter()
                    // A dependency on a service the file does not define is
                    // the file's mistake, not an ordering constraint.
                    .filter(|d| names.contains(*d))
                    .cloned()
                    .collect(),
            )
        })
        .collect();

    let mut sorted: Vec<String> = Vec::with_capacity(services.len());
    while !pending.is_empty() {
        let ready: Vec<String> = pending
            .iter()
            .filter(|(_, deps)| deps.is_empty())
            .map(|(name, _)| name.clone())
            .collect();
        if ready.is_empty() {
            let mut stuck: Vec<&str> = pending.keys().map(String::as_str).collect();
            stuck.sort();
            return Err(format!(
                "`depends_on` runs in a circle: {}. Nothing in that group can start first.",
                stuck.join(", ")
            ));
        }
        for name in ready {
            pending.remove(&name);
            for deps in pending.values_mut() {
                deps.remove(&name);
            }
            sorted.push(name);
        }
    }

    let mut by_name: BTreeMap<String, ComposePlanService> = services
        .into_iter()
        .map(|s| (s.service.clone(), s))
        .collect();
    Ok(sorted
        .into_iter()
        .filter_map(|name| by_name.remove(&name))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interpolate::Vars;

    fn loaded(yaml: &str) -> Loaded {
        Loaded {
            file: serde_yaml::from_str(yaml).expect("compose file parses"),
            project_dir: PathBuf::from("/srv/shop"),
            config_files: vec!["/srv/shop/compose.yaml".into()],
            vars: Vars::new(),
            warnings: Vec::new(),
        }
    }

    fn plan(yaml: &str) -> ComposePlan {
        build(&loaded(yaml), &PlanOptions::default(), &EngineCapabilities::engine_api()).unwrap()
    }

    fn apple_plan(yaml: &str) -> ComposePlan {
        build(&loaded(yaml), &PlanOptions::default(), &EngineCapabilities::apple()).unwrap()
    }

    fn service<'a>(plan: &'a ComposePlan, name: &str) -> &'a ComposePlanService {
        plan.services.iter().find(|s| s.service == name).unwrap()
    }

    #[test]
    fn a_service_becomes_a_container_named_the_way_compose_names_it() {
        let p = plan("services:\n  web:\n    image: nginx\n");
        assert_eq!(p.project, "shop");
        let web = service(&p, "web");
        assert_eq!(web.run.name.as_deref(), Some("shop-web-1"));
        assert_eq!(web.run.image, "nginx");
    }

    #[test]
    fn the_compose_labels_are_what_make_the_stack_findable_again() {
        let p = plan("services:\n  web:\n    image: nginx\n");
        let labels = &service(&p, "web").run.labels;
        assert_eq!(labels["com.docker.compose.project"], "shop");
        assert_eq!(labels["com.docker.compose.service"], "web");
        assert_eq!(labels["com.docker.compose.project.working_dir"], "/srv/shop");
        assert_eq!(
            labels["com.docker.compose.project.config_files"],
            "/srv/shop/compose.yaml"
        );
    }

    #[test]
    fn a_user_label_is_kept_but_cannot_overwrite_the_compose_ones() {
        let p = plan(
            "services:\n  web:\n    image: nginx\n    labels:\n      team: platform\n      com.docker.compose.project: hijack\n",
        );
        let labels = &service(&p, "web").run.labels;
        assert_eq!(labels["team"], "platform");
        assert_eq!(labels["com.docker.compose.project"], "shop");
    }

    #[test]
    fn every_service_joins_the_project_network_so_names_resolve() {
        let p = plan("services:\n  web:\n    image: nginx\n  db:\n    image: postgres\n");
        assert_eq!(p.networks[0].name, "shop_default");
        assert_eq!(service(&p, "web").run.network.as_deref(), Some("shop_default"));
        assert_eq!(service(&p, "db").run.network.as_deref(), Some("shop_default"));
    }

    #[test]
    fn declared_networks_are_scoped_to_the_project() {
        let p = plan(
            "services:\n  web:\n    image: nginx\n    networks: [front]\nnetworks:\n  front:\n",
        );
        assert!(p.networks.iter().any(|n| n.name == "shop_front"));
        assert_eq!(service(&p, "web").run.network.as_deref(), Some("shop_front"));
        // No service is left on the default, so it is not created.
        assert!(!p.networks.iter().any(|n| n.name == "shop_default"));
    }

    #[test]
    fn an_external_network_keeps_its_real_name_and_is_not_created_by_us() {
        let p = plan(
            "services:\n  web:\n    image: nginx\n    networks: [shared]\nnetworks:\n  shared:\n    external: true\n",
        );
        let net = p.networks.iter().find(|n| n.name == "shared").unwrap();
        assert!(net.external);
        assert_eq!(service(&p, "web").run.network.as_deref(), Some("shared"));
    }

    #[test]
    fn a_second_network_is_reported_rather_than_silently_dropped() {
        let p = plan(
            "services:\n  web:\n    image: nginx\n    networks: [front, back]\nnetworks:\n  front:\n  back:\n",
        );
        let web = service(&p, "web");
        assert_eq!(web.run.network.as_deref(), Some("shop_front"));
        assert_eq!(web.extra_networks, vec!["shop_back".to_string()]);
    }

    #[test]
    fn ports_translate_in_every_form() {
        let p = plan(
            "services:\n  web:\n    image: nginx\n    ports:\n      - 8080:80\n      - 127.0.0.1:5432:5432\n      - 53:53/udp\n      - target: 9000\n        published: 9001\n",
        );
        let ports = &service(&p, "web").run.ports;
        assert_eq!(ports[0].host, "8080");
        assert_eq!(ports[0].container, "80");
        assert_eq!(ports[1].host, "127.0.0.1:5432");
        assert_eq!(ports[2].proto.as_deref(), Some("udp"));
        assert_eq!(ports[3].host, "9001");
        assert_eq!(ports[3].container, "9000");
    }

    #[test]
    fn a_port_range_is_refused_with_a_reason_rather_than_mangled() {
        let p = plan("services:\n  web:\n    image: nginx\n    ports:\n      - 8000-8010:8000-8010\n");
        let web = service(&p, "web");
        assert!(web.run.ports.is_empty());
        assert!(web.warnings.iter().any(|w| w.contains("port range")));
    }

    #[test]
    fn a_relative_bind_mount_resolves_against_the_compose_file_not_the_process() {
        // Running Hopper from somewhere else must not change what is mounted.
        let p = plan("services:\n  web:\n    image: nginx\n    volumes:\n      - ./src:/app\n");
        let mount = &service(&p, "web").run.volumes[0];
        assert_eq!(mount.host, "/srv/shop/src");
        assert_eq!(mount.container, "/app");
        assert!(!mount.ro);
    }

    #[test]
    fn a_parent_relative_mount_is_collapsed_rather_than_left_literal() {
        let p = plan("services:\n  web:\n    image: nginx\n    volumes:\n      - ../shared:/data\n");
        assert_eq!(service(&p, "web").run.volumes[0].host, "/srv/shared");
    }

    #[test]
    fn a_read_only_mount_keeps_its_mode() {
        let p = plan("services:\n  web:\n    image: nginx\n    volumes:\n      - ./src:/app:ro\n");
        assert!(service(&p, "web").run.volumes[0].ro);
    }

    #[test]
    fn a_named_volume_is_scoped_to_the_project_rather_than_treated_as_a_path() {
        let p = plan(
            "services:\n  db:\n    image: postgres\n    volumes:\n      - data:/var/lib/postgresql/data\nvolumes:\n  data:\n",
        );
        assert_eq!(p.volumes[0].name, "shop_data");
        assert_eq!(service(&p, "db").run.volumes[0].host, "shop_data");
    }

    #[test]
    fn an_anonymous_volume_says_it_was_skipped() {
        let p = plan("services:\n  db:\n    image: postgres\n    volumes:\n      - /var/lib/data\n");
        let db = service(&p, "db");
        assert!(db.run.volumes.is_empty());
        assert!(db.warnings.iter().any(|w| w.contains("anonymous volume")));
    }

    #[test]
    fn environment_reaches_the_container_as_key_equals_value() {
        let p = plan(
            "services:\n  db:\n    image: postgres\n    environment:\n      POSTGRES_PASSWORD: secret\n      PORT: 5432\n",
        );
        let env = &service(&p, "db").run.env;
        assert!(env.contains(&"POSTGRES_PASSWORD=secret".to_string()));
        assert!(env.contains(&"PORT=5432".to_string()));
    }

    #[test]
    fn a_pass_through_variable_that_is_not_set_is_reported_not_blanked() {
        // `FOO=` and "FOO was never set" mean different things to a program.
        let p = plan("services:\n  web:\n    image: nginx\n    environment:\n      - MISSING\n");
        let web = service(&p, "web");
        assert!(web.run.env.is_empty());
        assert!(web.warnings.iter().any(|w| w.contains("MISSING")));
    }

    #[test]
    fn dependencies_decide_the_start_order() {
        let p = plan(
            "services:\n  web:\n    image: nginx\n    depends_on: [api]\n  api:\n    image: api\n    depends_on: [db]\n  db:\n    image: postgres\n",
        );
        let order: Vec<&str> = p.services.iter().map(|s| s.service.as_str()).collect();
        assert_eq!(order, vec!["db", "api", "web"]);
    }

    #[test]
    fn a_dependency_cycle_is_refused_by_name() {
        let err = build(
            &loaded("services:\n  a:\n    image: a\n    depends_on: [b]\n  b:\n    image: b\n    depends_on: [a]\n"),
            &PlanOptions::default(),
            &EngineCapabilities::engine_api(),
        )
        .unwrap_err();
        assert!(err.contains("circle"));
        assert!(err.contains('a') && err.contains('b'));
    }

    #[test]
    fn a_dependency_on_a_service_that_does_not_exist_does_not_deadlock_the_sort() {
        let p = plan("services:\n  web:\n    image: nginx\n    depends_on: [ghost]\n");
        assert_eq!(p.services.len(), 1);
    }

    #[test]
    fn a_service_built_from_a_dockerfile_is_blocked_with_a_reason() {
        let p = plan("services:\n  app:\n    build: .\n");
        let app = service(&p, "app");
        assert!(app.blocked.as_deref().unwrap().contains("does not build images"));
        assert!(p.runnable().is_empty());
    }

    #[test]
    fn a_service_with_both_build_and_image_runs_from_the_image_and_says_so() {
        let p = plan("services:\n  app:\n    build: .\n    image: app:dev\n");
        let app = service(&p, "app");
        assert!(app.blocked.is_none());
        assert_eq!(app.run.image, "app:dev");
        assert!(app.warnings.iter().any(|w| w.contains("build")));
    }

    #[test]
    fn profiles_keep_a_service_out_of_the_run_until_it_is_asked_for() {
        let yaml = "services:\n  web:\n    image: nginx\n  debug:\n    image: busybox\n    profiles: [tools]\n";
        let p = plan(yaml);
        assert!(!service(&p, "debug").selected);
        assert_eq!(p.runnable().len(), 1);

        let with = build(
            &loaded(yaml),
            &PlanOptions {
                profiles: vec!["tools".into()],
                ..Default::default()
            },
            &EngineCapabilities::engine_api(),
        )
        .unwrap();
        assert_eq!(with.runnable().len(), 2);
    }

    #[test]
    fn naming_one_service_pulls_in_what_it_depends_on() {
        // Starting `web` without `db` would come up broken.
        let p = build(
            &loaded("services:\n  web:\n    image: nginx\n    depends_on: [db]\n  db:\n    image: postgres\n  cache:\n    image: redis\n"),
            &PlanOptions { only: vec!["web".into()], ..Default::default() },
            &EngineCapabilities::engine_api(),
        )
        .unwrap();
        let running: Vec<&str> = p.runnable().iter().map(|s| s.service.as_str()).collect();
        assert_eq!(running, vec!["db", "web"]);
    }

    #[test]
    fn a_dropped_healthcheck_is_reported_on_both_engines_because_neither_gets_one() {
        // This warning is deliberately not gated on the engine. Hopper creates
        // containers without a healthcheck whichever backend answers, so
        // staying quiet on Docker would be the lie.
        let yaml = "services:\n  db:\n    image: postgres\n    healthcheck:\n      test: [CMD, pg_isready]\n";
        for p in [apple_plan(yaml), plan(yaml)] {
            assert!(p.services[0].warnings.iter().any(|w| w.contains("healthcheck")));
        }
    }

    #[test]
    fn a_restart_policy_is_reported_on_apple_and_kept_quiet_on_docker() {
        // This one really does differ: Docker applies it, Apple has none.
        let yaml = "services:\n  web:\n    image: nginx\n    restart: unless-stopped\n";
        assert!(apple_plan(yaml).services[0]
            .warnings
            .iter()
            .any(|w| w.contains("restart")));
        assert!(plan(yaml).services[0].warnings.is_empty());
        // Either way the policy is still carried, so the engine that can honour
        // it does.
        assert_eq!(
            apple_plan(yaml).services[0].run.restart.as_deref(),
            Some("unless-stopped")
        );
    }

    #[test]
    fn a_config_hash_rides_along_so_compose_recognizes_the_stack() {
        let p = plan("services:\n  web:\n    image: nginx\n");
        let labels = &service(&p, "web").run.labels;
        assert!(!labels["com.docker.compose.config-hash"].is_empty());
    }

    #[test]
    fn the_hash_changes_when_the_service_does_and_not_otherwise() {
        // This is what stops a second `up` from recreating a database that
        // nobody touched — and what makes it recreate one that changed.
        let hash = |yaml: &str| {
            plan(yaml).services[0].run.labels["com.docker.compose.config-hash"].clone()
        };
        let base = "services:\n  web:\n    image: nginx\n    ports: ['80:80']\n";
        assert_eq!(hash(base), hash(base));
        assert_ne!(hash(base), hash("services:\n  web:\n    image: nginx\n    ports: ['81:80']\n"));
        assert_ne!(hash(base), hash("services:\n  web:\n    image: caddy\n    ports: ['80:80']\n"));
    }

    #[test]
    fn a_health_gated_dependency_degrades_to_a_start_gate_with_a_warning() {
        let p = apple_plan(
            "services:\n  web:\n    image: nginx\n    depends_on:\n      db:\n        condition: service_healthy\n  db:\n    image: postgres\n",
        );
        let web = service(&p, "web");
        assert_eq!(web.depends_on, vec!["db".to_string()]);
        assert!(web.warnings.iter().any(|w| w.contains("healthy")));
    }

    #[test]
    fn an_unimplemented_key_becomes_a_warning_rather_than_a_silent_drop() {
        let p = plan("services:\n  web:\n    image: nginx\n    cap_add: [NET_ADMIN]\n");
        assert!(service(&p, "web").warnings.iter().any(|w| w.contains("cap_add")));
    }

    #[test]
    fn resource_limits_read_from_deploy_and_from_the_older_keys() {
        let p = plan(
            "services:\n  a:\n    image: a\n    deploy:\n      resources:\n        limits:\n          cpus: '0.5'\n          memory: 512M\n  b:\n    image: b\n    mem_limit: 1g\n    cpus: 2\n",
        );
        assert_eq!(service(&p, "a").run.limits.cpus, Some(0.5));
        assert_eq!(service(&p, "a").run.limits.memory, Some(512 * 1024 * 1024));
        assert_eq!(service(&p, "b").run.limits.memory, Some(1024 * 1024 * 1024));
        assert_eq!(service(&p, "b").run.limits.cpus, Some(2.0));
    }

    #[test]
    fn a_list_command_survives_being_flattened_to_one_line() {
        let p = plan("services:\n  web:\n    image: nginx\n    command: [sh, -c, \"echo hi there\"]\n");
        assert_eq!(
            service(&p, "web").run.command.as_deref(),
            Some("sh -c \"echo hi there\"")
        );
    }

    #[test]
    fn an_explicit_container_name_is_honoured_over_the_generated_one() {
        let p = plan("services:\n  web:\n    image: nginx\n    container_name: shop-frontend\n");
        assert_eq!(service(&p, "web").run.name.as_deref(), Some("shop-frontend"));
    }

    #[test]
    fn all_warnings_names_the_service_each_one_came_from() {
        let p = plan("services:\n  web:\n    image: nginx\n    cap_add: [NET_ADMIN]\n");
        assert!(p.all_warnings().iter().any(|w| w.starts_with("web: ")));
    }
}
