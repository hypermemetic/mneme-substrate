#!/usr/bin/env bash
# Build + run the mneme-substrate container with sensible defaults.
#
# Auth: mounts ~/.claude into the container so claude-code uses your
# existing OAuth login. Override with ANTHROPIC_API_KEY=... if you'd
# rather use API billing.
#
# Persistent calibration store: a named docker volume `mneme-calibration`
# is mounted at /workspace/programs/_calibration so resolved-observation
# data survives container restarts.
#
# Vendored bench data: mounts the host's programs/_benchmarks/forecastbench
# into the container read-only so the bench script can find the question
# and resolution sets.
#
# Usage:
#   ./scripts/run_container.sh build       # build the image
#   ./scripts/run_container.sh up          # start substrate detached, drop into shell
#   ./scripts/run_container.sh up -d       # start detached, no shell
#   ./scripts/run_container.sh stop        # kill the running container
#   ./scripts/run_container.sh logs        # tail logs
#   ./scripts/run_container.sh shell       # exec a shell into the running container

set -euo pipefail

IMAGE_NAME="${IMAGE_NAME:-mneme-substrate:dev}"
CONTAINER_NAME="${CONTAINER_NAME:-mneme}"
HOST_PORT="${HOST_PORT:-4456}"
HOST_BENCH_DIR="${HOST_BENCH_DIR:-$(pwd)/programs/_benchmarks/forecastbench}"

cmd="${1:-up}"
shift || true

case "$cmd" in
  build)
    # Build context is the parent of mneme-substrate/ so the skills/
    # sibling directory is visible (forecast/SKILL.md is include_str!'d).
    # --load so the image lands in the local docker store (not just the
    # buildkit cache).
    docker build --load -f Dockerfile -t "$IMAGE_NAME" ..
    ;;
  up)
    docker rm -f "$CONTAINER_NAME" 2>/dev/null || true
    auth_args=()
    # Resolve auth in priority order:
    #   1. ANTHROPIC_API_KEY env (uses API billing)
    #   2. CLAUDE_CODE_OAUTH_TOKEN env (already-exported OAuth token)
    #   3. macOS Keychain entry "Claude Code-credentials" (Claude Pro/Max session)
    # Mounting ~/.claude alone does NOT work — the OAuth token lives in the
    # Keychain on macOS, not in the .claude directory. Pattern lifted from
    # juggernautlabs/claude-container/lib/auth.sh.
    if [[ -n "${ANTHROPIC_API_KEY:-}" ]]; then
      echo "auth: ANTHROPIC_API_KEY (env)"
      auth_args=(-e "ANTHROPIC_API_KEY=$ANTHROPIC_API_KEY")
    elif [[ -n "${CLAUDE_CODE_OAUTH_TOKEN:-}" ]]; then
      echo "auth: CLAUDE_CODE_OAUTH_TOKEN (env, ${CLAUDE_CODE_OAUTH_TOKEN:0:20}...)"
      auth_args=(-e "CLAUDE_CODE_OAUTH_TOKEN=$CLAUDE_CODE_OAUTH_TOKEN")
    elif command -v security >/dev/null 2>&1; then
      kc_blob=$(security find-generic-password -s "Claude Code-credentials" -w 2>/dev/null || true)
      if [[ -n "$kc_blob" ]]; then
        # The keychain blob is JSON: {"claudeAiOauth":{"accessToken":"sk-ant-oat01-...",...}}
        token=$(printf '%s' "$kc_blob" | python3 -c 'import json,sys; print(json.load(sys.stdin)["claudeAiOauth"]["accessToken"])' 2>/dev/null || true)
        if [[ -n "$token" ]]; then
          echo "auth: macOS Keychain (Claude Code-credentials, ${token:0:20}...)"
          auth_args=(-e "CLAUDE_CODE_OAUTH_TOKEN=$token")
        fi
      fi
    fi
    if [[ ${#auth_args[@]} -eq 0 ]]; then
      echo "no Claude OAuth token found." >&2
      if command -v claude >/dev/null 2>&1; then
        echo "running 'claude /login' to fetch one — accept the OAuth flow in your browser..." >&2
        claude /login || {
          echo "ERROR: 'claude /login' failed; aborting" >&2
          exit 1
        }
        # retry the keychain after login
        kc_blob=$(security find-generic-password -s "Claude Code-credentials" -w 2>/dev/null || true)
        if [[ -n "$kc_blob" ]]; then
          token=$(printf '%s' "$kc_blob" | python3 -c 'import json,sys; print(json.load(sys.stdin)["claudeAiOauth"]["accessToken"])' 2>/dev/null || true)
          if [[ -n "$token" ]]; then
            echo "auth: macOS Keychain (post-login, ${token:0:20}...)"
            auth_args=(-e "CLAUDE_CODE_OAUTH_TOKEN=$token")
          fi
        fi
      else
        echo "ERROR: no auth source AND 'claude' CLI not installed." >&2
        echo "  Install Claude Code from https://docs.claude.com/en/docs/claude-code/quickstart" >&2
        echo "  OR set ANTHROPIC_API_KEY before running this script." >&2
        exit 1
      fi
    fi
    if [[ ${#auth_args[@]} -eq 0 ]]; then
      echo "ERROR: still no auth after login attempt" >&2
      exit 1
    fi
    # Single unified bind mount of host programs/ → container /workspace/programs/.
    # This gives both sides the same view: vendored bench data is visible
    # to the substrate, calibration accumulates on host disk (survives
    # container restarts), and the host bench script's file polling
    # sees what the container writes.
    mkdir -p "$(pwd)/programs"
    echo "programs: bind-mounting $(pwd)/programs to /workspace/programs"

    # CRITICAL: bind-mount the substrate's per-activation state directory
    # so arbor, claudecode, cone, lattice, orcha, pm DBs (and everything
    # else) survive container restarts. Without this, every `up` (which
    # does `docker rm -f`) wipes all conversation history, the calibration
    # store fits, etc. The full conversation log lives under arbor/ here.
    mkdir -p "$(pwd)/.plexus-state"
    echo "plexus state: bind-mounting $(pwd)/.plexus-state to /root/.plexus"

    # Default `up` (no flags): start substrate detached, then exec into
    # an interactive shell with the substrate already running. The user
    # gets a prompt where `synapse`, `mneme`, etc. work immediately.
    # `up -d` skips the interactive shell (substrate stays detached).
    detach_flag="-d"
    interactive_after=true
    for a in "$@"; do
      if [[ "$a" == "-d" || "$a" == "--detach" ]]; then
        interactive_after=false
      fi
    done
    docker run -d \
      --name "$CONTAINER_NAME" \
      -p "$HOST_PORT:4456" \
      -v "$(pwd)/programs:/workspace/programs" \
      -v "$(pwd)/.plexus-state:/root/.plexus" \
      -v "$(pwd):/workspace/host:ro" \
      "${auth_args[@]}" \
      "$IMAGE_NAME" >/dev/null
    # wait briefly for substrate readiness
    for _ in $(seq 1 20); do
      if docker logs "$CONTAINER_NAME" 2>&1 | grep -q "Substrate Plexus RPC server started"; then
        break
      fi
      sleep 0.5
    done
    echo
    echo "✓ substrate up at ws://localhost:$HOST_PORT (container: $CONTAINER_NAME)"
    echo "  inside the container, try:  mneme forecast \"Will X happen by date Y?\""
    echo "  on the host:                 mneme last  |  mneme down"
    if $interactive_after; then
      echo
      echo "dropping you into the container shell. exit returns to your host;"
      echo "the substrate keeps running until 'mneme down' or 'scripts/run_container.sh stop'."
      echo
      docker exec -it "$CONTAINER_NAME" bash
    fi
    ;;
  stop)
    docker rm -f "$CONTAINER_NAME"
    ;;
  logs)
    docker logs -f "$CONTAINER_NAME"
    ;;
  shell)
    docker exec -it "$CONTAINER_NAME" bash
    ;;
  *)
    echo "Unknown command: $cmd"
    echo "Usage: $0 {build|up|stop|logs|shell} [extra docker args]"
    exit 1
    ;;
esac
