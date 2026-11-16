#!/bin/sh
# Version and digest changes are release-reviewed. Never execute remote scripts.
set -eu
# A stopped legacy VM may already have a distro-managed Docker installation.
# Preserve it while removing the old moving installer from Lima's profile.
# Interrupted first provisioning continues below until the service exists.
if command -v docker >/dev/null 2>&1 && command -v dockerd >/dev/null 2>&1 && systemctl cat docker.service >/dev/null 2>&1; then
  systemctl enable --now docker.service
  exit 0
fi
version=29.9.3
digest=1b27fd739378ef938401e81ce5afb73b7ad7432d4f598814a9ff5f826bb464d2
install=/opt/hopper/docker/$version
if [ ! -x "$install/dockerd" ]; then
  stage=$(mktemp -d)
  trap 'rm -rf "$stage"' EXIT HUP INT TERM
  curl --fail --location --proto '=https' --tlsv1.2 --connect-timeout 20 --max-time 300 \
    "https://download.docker.com/linux/static/stable/aarch64/docker-$version.tgz" -o "$stage/docker.tgz"
  printf '%s  %s\n' "$digest" "$stage/docker.tgz" | sha256sum --check --status
  tar -xzf "$stage/docker.tgz" -C "$stage"
  mkdir -p /opt/hopper/docker
  mv "$stage/docker" "$install"
fi
# Installed binaries stay versioned. Existing VM disks are never upgraded by
# editing this profile: a guest runtime update is a deliberate release action.
for bin in docker dockerd containerd ctr runc docker-init docker-proxy containerd-shim-runc-v2; do
  ln -sfn "$install/$bin" "/usr/local/bin/$bin"
done
mkdir -p /etc/docker
if [ ! -f /etc/docker/daemon.json ]; then
  printf '%s\n' '{"log-driver":"local","log-opts":{"max-size":"10m","max-file":"3"},"builder":{"gc":{"enabled":true,"defaultKeepStorage":"5GB"}},"features":{"cdi":true,"containerd-snapshotter":true}}' > /etc/docker/daemon.json
  chmod 600 /etc/docker/daemon.json
fi
cat > /etc/systemd/system/docker.socket <<'UNIT'
[Unit]
Description=Hopper Docker API socket
[Socket]
ListenStream=/run/docker.sock
SocketMode=0600
SocketUser={{.User}}
[Install]
WantedBy=sockets.target
UNIT
cat > /etc/systemd/system/docker.service <<'UNIT'
[Unit]
Description=Hopper Docker Engine
Requires=docker.socket
After=network-online.target docker.socket
Wants=network-online.target
[Service]
Type=notify
Environment=PATH=/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
ExecStart=/usr/local/bin/dockerd --host=fd://
Restart=on-failure
RestartSec=3
TimeoutStartSec=90
TimeoutStopSec=120
Delegate=yes
KillMode=process
LimitNOFILE=infinity
LimitNPROC=infinity
TasksMax=infinity
[Install]
WantedBy=multi-user.target
UNIT
systemctl daemon-reload
systemctl enable --now docker.socket docker.service
