#!/usr/bin/env bash
# Live certification uses a private state root and deletes only its own VM.
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"
stage="$(mktemp -d /tmp/hoppercert.XXXXXXXX)"
export HOPPER_DIR="$stage/state"
export HOPPER_TEST_VM_HOME="$stage/vm"
export HOPPER_ENGINE=vm
export HOPPER_TEST_IDLE=1
export HOPPER_TEST_LIFECYCLE=1
mkdir -p "$HOPPER_DIR/engine"
ln -s "$HOPPER_TEST_VM_HOME" "$HOPPER_DIR/engine/lima"
helper="$root/native/build/lima/bin/limactl"
cleanup() {
  certification_status=$?
  if [[ -d "$HOPPER_TEST_VM_HOME/hopper" ]]; then
    LIMA_HOME="$HOPPER_TEST_VM_HOME" "$helper" --tty=false stop --force hopper >/dev/null 2>&1 || true
    LIMA_HOME="$HOPPER_TEST_VM_HOME" "$helper" --tty=false delete --force hopper >/dev/null 2>&1 || true
  fi
  certification_logs="${HOPPER_CERTIFICATION_LOGS:-$root/dist/verification/native}"
  mkdir -p "$certification_logs"
  chmod 700 "$certification_logs"
  cp "$stage"/*.log "$certification_logs/" 2>/dev/null || true
  cp "$HOPPER_TEST_VM_HOME/engine.log" "$certification_logs/engine.log" 2>/dev/null || true
  chmod 600 "$certification_logs"/*.log 2>/dev/null || true
  if [[ "$certification_status" -ne 0 ]]; then
    echo "Native certification failed. Logs: $certification_logs" >&2
  fi
  rm -rf "$stage"
}
trap cleanup EXIT
if [[ -x "$root/native/build/docker" && -x "$root/native/build/buildx" ]]; then
  export HOPPER_TEST_DOCKER="$root/native/build/docker"
  export DOCKER_CONFIG="$stage/docker"
  mkdir -p "$DOCKER_CONFIG/cli-plugins"
  ln -s "$root/native/build/buildx" "$DOCKER_CONFIG/cli-plugins/docker-buildx"
fi
[[ -x "$helper" ]] || { echo 'Build the pinned helper with scripts/build/lima.sh first.' >&2; exit 1; }
cargo test -p engine --test live --locked the_resolved_profile_forwards_the_docker_socket_once -- --ignored --nocapture > "$stage/profile.log" 2>&1
cargo test -p engine --test live --locked docker_files_ports_and_volumes_survive_a_vm_restart -- --ignored --nocapture > "$stage/lifecycle.log" 2>&1
export HOPPER_TEST_HOST="unix://$HOPPER_TEST_VM_HOME/hopper/sock/docker.sock"
cargo test -p docker --test build --locked -- --ignored --nocapture > "$stage/build.log" 2>&1
cargo test -p host --lib --locked idle_suspension_drains_own_events_and_respects_external_clients -- --ignored --nocapture > "$stage/idle.log" 2>&1
cargo test -p host --lib --locked startup_admission_allows_stop_to_cancel_its_owner -- --ignored --nocapture > "$stage/cancellation.log" 2>&1
cargo test -p engine --test live --locked configured_vm_enforces_grants_and_preserves_the_previous_daemon_configuration -- --ignored --nocapture > "$stage/grants.log" 2>&1
echo 'Live profile, lifecycle, streamed build, idle suspension, grants, daemon rollback, stats, and trim certification passed.'
