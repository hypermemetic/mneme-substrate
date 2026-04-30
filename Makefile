# mneme-substrate — host-side install + container management
#
# Quick start:
#   make install    # symlink the `mneme` CLI to ~/.local/bin
#   make build      # build the container image (~3 min cold, ~30s incremental)
#   make run        # start substrate, drop into container shell
#                   # (substrate runs in background; exit shell to leave it running)
#   make down       # stop the container
#
# Other targets:
#   make logs       # tail substrate logs
#   make shell      # exec into the running container
#   make test       # cargo test --lib (host-side)
#   make login      # claude /login (if no OAuth token detected)
#   make clean      # stop container + clear .plexus-state (loses session history)
#
# Legacy host-side cargo workflow (for substrate hacking, no container):
#   make build-host   # cargo build --bin mneme-substrate
#   make start-host   # background substrate process bound to 127.0.0.1:4444
#   make stop-host    # kill the host process
#   make log-host     # tail its log

PREFIX        ?= $(HOME)/.local
BIN_DIR       ?= $(PREFIX)/bin
SUBSTRATE_DIR := $(realpath .)
SCRIPTS_DIR   := $(SUBSTRATE_DIR)/scripts
SYNAPSE       := $(shell command -v synapse 2>/dev/null)
DOCKER        := $(shell command -v docker 2>/dev/null || command -v podman 2>/dev/null)
CLAUDE        := $(shell command -v claude 2>/dev/null)

# Legacy host-side substrate run targets keep working
HOST_BIN     := target/debug/mneme-substrate
HOST_LOG     := /tmp/substrate.log
HOST_PIDFILE := /tmp/substrate.pid

.PHONY: help install build run up down stop logs shell test login clean check-deps \
        build-host start-host stop-host restart-host log-host

help:
	@echo "mneme-substrate — make targets:"
	@echo ""
	@echo "  install   — symlink the mneme CLI into ~/.local/bin"
	@echo "  build     — build the container image"
	@echo "  run       — start substrate + drop into container shell"
	@echo "  up        — alias for run"
	@echo "  down      — stop the container (preserves state)"
	@echo "  logs      — tail substrate logs"
	@echo "  shell     — exec into a running container"
	@echo "  test      — cargo test --lib (host-side)"
	@echo "  login     — run claude /login if no OAuth token found"
	@echo "  clean     — stop container + nuke .plexus-state (loses history)"
	@echo ""
	@echo "  Legacy (no container):"
	@echo "    build-host  — cargo build the substrate binary"
	@echo "    start-host  — run substrate as a host process on port 4444"
	@echo "    stop-host / restart-host / log-host"
	@echo ""
	@echo "  Optional host tools:"
	@echo "    synapse  — Plexus RPC CLI client"
	@echo "    docker (or podman, colima)"
	@echo "    claude   — Claude Code CLI; supplies the OAuth token"
	@echo ""
	@echo "  See README.md for installation links."

check-deps:
	@if [ -z "$(DOCKER)" ]; then \
	  echo "ERROR: no Docker/Podman/Colima found on PATH"; exit 1; \
	fi
	@echo "✓ docker/podman: $(DOCKER)"
	@if [ -z "$(SYNAPSE)" ]; then \
	  echo "WARNING: synapse not on PATH. Some advanced workflows assume it."; \
	  echo "  Install: see https://github.com/hypermemetic/synapse"; \
	  echo "  (the in-container 'mneme' CLI works without host synapse)"; \
	else \
	  echo "✓ synapse: $(SYNAPSE)"; \
	fi
	@if [ -z "$(CLAUDE)" ]; then \
	  echo "WARNING: 'claude' CLI not on PATH."; \
	  echo "  Without it, OAuth login flow can't run automatically."; \
	  echo "  Install: https://docs.claude.com/en/docs/claude-code/quickstart"; \
	else \
	  echo "✓ claude: $(CLAUDE)"; \
	fi

install: check-deps
	@mkdir -p $(BIN_DIR)
	@if [ -L $(BIN_DIR)/mneme ] || [ -e $(BIN_DIR)/mneme ]; then \
	  rm -f $(BIN_DIR)/mneme; \
	fi
	@ln -s $(SCRIPTS_DIR)/mneme $(BIN_DIR)/mneme
	@echo "✓ mneme CLI symlinked: $(BIN_DIR)/mneme → $(SCRIPTS_DIR)/mneme"
	@if ! echo "$$PATH" | tr ':' '\n' | grep -qx "$(BIN_DIR)"; then \
	  echo ""; \
	  echo "  ⚠  $(BIN_DIR) is not on your PATH."; \
	  echo "  Add this to your shell rc:"; \
	  echo "      export PATH=\"$(BIN_DIR):\$$PATH\""; \
	fi
	@echo ""
	@echo "next: make build && make run"

build: check-deps
	bash scripts/run_container.sh build

run: check-deps
	bash scripts/run_container.sh up

up: run

down stop:
	bash scripts/run_container.sh stop

logs:
	bash scripts/run_container.sh logs

shell:
	bash scripts/run_container.sh shell

test:
	cargo test --lib

login:
	@if [ -z "$(CLAUDE)" ]; then \
	  echo "ERROR: 'claude' CLI not installed. https://docs.claude.com/en/docs/claude-code/quickstart"; \
	  exit 1; \
	fi
	claude /login

clean:
	@echo "stopping container..."
	-bash scripts/run_container.sh stop 2>/dev/null
	@echo "removing .plexus-state (loses all conversation history + calibration store)..."
	@read -p "are you sure? [y/N] " ans; [ "$$ans" = "y" ] || exit 1
	rm -rf .plexus-state
	@echo "✓ cleaned"

# ─── Legacy host-process targets (no container) ──────────────────────────

build-host:
	cargo build --bin mneme-substrate --features mcp-gateway

start-host: build-host
	@if [ -f $(HOST_PIDFILE) ] && kill -0 $$(cat $(HOST_PIDFILE)) 2>/dev/null; then \
	  echo "substrate already running (pid $$(cat $(HOST_PIDFILE)))"; \
	else \
	  nohup $(HOST_BIN) > $(HOST_LOG) 2>&1 & echo $$! > $(HOST_PIDFILE); \
	  echo "substrate started (pid $$(cat $(HOST_PIDFILE)))"; \
	fi

stop-host:
	@if [ -f $(HOST_PIDFILE) ]; then \
	  kill $$(cat $(HOST_PIDFILE)) 2>/dev/null || true; \
	  rm -f $(HOST_PIDFILE); \
	fi
	@echo "substrate stopped"

restart-host: stop-host build-host
	@sleep 1
	@nohup $(HOST_BIN) > $(HOST_LOG) 2>&1 & echo $$! > $(HOST_PIDFILE)
	@echo "substrate restarted (pid $$(cat $(HOST_PIDFILE)))"

log-host:
	@tail -f $(HOST_LOG)
