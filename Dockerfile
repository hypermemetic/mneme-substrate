# syntax=docker/dockerfile:1.7
# Mneme substrate container.
#
# What's inside:
# - mneme-substrate Rust binary (release build)
# - Node.js + @anthropic-ai/claude-code (the CLI mneme shells out to)
# - Minimal runtime deps (sqlite, ca-certs)
#
# Auth options at run time:
#   1. Mount the host's ~/.claude into the container's /root/.claude
#      (uses your existing OAuth login; ties to your user's session).
#      docker run -v ~/.claude:/root/.claude:ro ...
#   2. Pass ANTHROPIC_API_KEY via env (uses API billing instead of Pro plan).
#      docker run -e ANTHROPIC_API_KEY=... ...
#
# Persistent state:
#   The substrate writes per-program state under /workspace/programs/.
#   For organizational calibration data to accumulate across container
#   restarts, mount a named volume:
#      docker run -v mneme-calibration:/workspace/programs/_calibration ...
#
# Vendored bench data lives at /workspace/programs/_benchmarks/ (mount
# read-only from host or bake into the image — see scripts/run.sh).

# ============================================================================
# Stage 1 — build the Rust binary
# ============================================================================
FROM rust:1.92-slim-bookworm AS builder

# Build deps for sqlx (sqlite), reqwest (openssl), pkg-config for resolving
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential \
        pkg-config \
        libssl-dev \
        libsqlite3-dev \
        ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Substrate's `forecast` activation does `include_str!("../../../../skills/skills/forecast/SKILL.md")`,
# so the build context must include both `mneme-substrate/` and its
# sibling `skills/` directory. Build with:
#     docker build -f mneme-substrate/Dockerfile -t mneme-substrate:dev .
# from the hypermemetic root (i.e. the parent of mneme-substrate/).
COPY mneme-substrate/Cargo.toml ./mneme-substrate/
COPY mneme-substrate/src/ ./mneme-substrate/src/
COPY mneme-substrate/examples/ ./mneme-substrate/examples/
COPY skills/skills/forecast/SKILL.md ./skills/skills/forecast/SKILL.md

# Release build of the main binary.
#
# BuildKit cache mounts: cargo's registry/git caches and the target dir
# survive across `docker build` invocations without ending up in the image
# layer. First build is full (~3 min); subsequent code-only changes
# rebuild incrementally in ~10-30s (target/ is reused).
#
# Cache mounts are only visible during this RUN — so we copy the produced
# binary out to a path that ISN'T a cache mount, where the runtime stage
# can `COPY --from=builder` it.
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/build/mneme-substrate/target,sharing=locked \
    cd mneme-substrate \
    && cargo build --release --bin mneme-substrate \
    && cp target/release/mneme-substrate /build/mneme-substrate-bin

# ============================================================================
# Stage 2 — runtime image
# ============================================================================
FROM debian:bookworm-slim AS runtime

# Runtime deps: openssl + sqlite for the substrate; node for claude-code.
# Keep the install minimal; this image is the security boundary.
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
        libssl3 \
        libsqlite3-0 \
        curl \
        gnupg \
    && curl -fsSL https://deb.nodesource.com/setup_20.x | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && rm -rf /var/lib/apt/lists/* \
    && npm install -g @anthropic-ai/claude-code \
    && npm cache clean --force

# Substrate binary (copied out of the cache mount during the build RUN above)
COPY --from=builder /build/mneme-substrate-bin /usr/local/bin/mneme-substrate

# Helper scripts available inside the container — bench runners, market
# watchers, ticket-as-forecast pipelines. Users invoke them as
# `python3 /workspace/scripts/<name>.py` from the in-container shell.
COPY mneme-substrate/scripts/ /workspace/scripts/
RUN chmod +x /workspace/scripts/* 2>/dev/null || true

# Where the substrate writes its per-program state. Mount a host dir or
# named volume here for persistence.
WORKDIR /workspace
RUN mkdir -p /workspace/programs

# A friendly bash prompt + helpful welcome banner for the in-container
# shell. Shown when the user `docker exec`s in (e.g. via `mneme up`).
RUN echo 'export PS1="\[\033[1;36m\]mneme\[\033[0m\] \[\033[1;32m\]\W\[\033[0m\] $ "' >> /root/.bashrc \
    && echo 'cat /etc/motd 2>/dev/null || true' >> /root/.bashrc

RUN cat > /etc/motd <<'EOF'

╭──────────────────────────────────────────────────────────────╮
│  mneme — forecasting substrate (BLF, Murphy 2026)            │
│                                                              │
│  Substrate is running on ws://localhost:4456 inside this     │
│  container.                                                  │
│                                                              │
│  Fire a forecast (returns a program_id immediately):         │
│                                                              │
│    synapse substrate forecast update \                       │
│        --program-id MY-Q \                                   │
│        --new-evidence "Will X happen by Y?" \                │
│        --trials 3 --iterative-max-steps 5                    │
│                                                              │
│  Copy the program_id from the output, then wait on it:       │
│                                                              │
│    synapse substrate programs wait \                         │
│        --program-id <paste_uuid_here>                        │
│                                                              │
│  Batch pipelines:                                            │
│    python3 /workspace/scripts/forecastbench_live_run.py …    │
│    python3 /workspace/scripts/marketwatch_live.py            │
│    python3 /workspace/scripts/ticket_forecast.py             │
│                                                              │
│  exit returns to your host shell; substrate keeps running    │
│  until `make down` (or scripts/run_container.sh stop).       │
╰──────────────────────────────────────────────────────────────╯

EOF

# Substrate listens on this port; map to host with -p 4456:4456.
EXPOSE 4456

# Default tracing config; can override with -e RUST_LOG=...
ENV RUST_LOG=info

# Bind to all interfaces inside the container so the host's port-forward
# can reach the substrate. Override at runtime with --bind if needed.
ENV MNEME_BIND=0.0.0.0

# Default command. Override with `docker run ... mneme-substrate --help`
# to see all flags.
CMD ["mneme-substrate", "--port", "4456"]
