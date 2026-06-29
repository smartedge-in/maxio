# =============================================================================
# MaxIO — Production GNU Makefile
# Rust quality pipeline, security scanning, and container build automation.
# Portable across Linux and macOS.  GNU Make 3.81+ required.
# =============================================================================

SHELL         := /bin/bash
.SHELLFLAGS   := -eu -o pipefail -c
.ONESHELL:
MAKEFLAGS     += --no-builtin-rules
.DEFAULT_GOAL := help

# -----------------------------------------------------------------------------
# Tooling
# -----------------------------------------------------------------------------
CARGO_HOME      ?= $(HOME)/.cargo
BUN_INSTALL     ?= $(HOME)/.bun
LOCAL_BIN       ?= $(HOME)/.local/bin
export PATH     := $(CARGO_HOME)/bin:$(BUN_INSTALL)/bin:$(LOCAL_BIN):$(PATH)
CARGO           := $(firstword $(shell command -v cargo 2>/dev/null) $(CARGO_HOME)/bin/cargo)
RUSTUP          := $(firstword $(shell command -v rustup 2>/dev/null) $(CARGO_HOME)/bin/rustup)
TRIVY           := $(firstword $(shell command -v trivy 2>/dev/null) $(LOCAL_BIN)/trivy)
DOCKER          ?= docker
BUN             := $(shell command -v bun 2>/dev/null)
HAS_BUN         := $(if $(BUN),1,)

# -----------------------------------------------------------------------------
# Project / image / semver
# -----------------------------------------------------------------------------
PROJECT         ?= maxio
VERSION_FILE    ?= VERSION
VERSION         := $(shell tr -d '[:space:]' < $(VERSION_FILE) 2>/dev/null)
IMAGE_NAME      ?= maxio
IMAGE_TAG       ?= v$(VERSION)
IMAGE_REF       := $(IMAGE_NAME):$(IMAGE_TAG)

# -----------------------------------------------------------------------------
# Artifact paths
# -----------------------------------------------------------------------------
SBOM_FILE       ?= sbom.json
SARIF_FILE      ?= trivy-results.sarif
REPORT_FILE     ?= trivy-report.json
TRIVY_CACHE_DIR ?= $(or $(TMPDIR),/tmp)/$(PROJECT)-trivy-cache
COVERAGE_DIR    ?= coverage
REPORT_DIR      ?= reports

# -----------------------------------------------------------------------------
# Build / lint flags
# -----------------------------------------------------------------------------
CARGO_FLAGS     ?=
BUILD_FLAGS     ?= --locked
CLIPPY_FLAGS    ?= --workspace --all-targets --all-features -- -D warnings
COVERAGE_FLAGS  ?= --workspace --all-features --html --output-dir $(COVERAGE_DIR)
DENY_FLAGS      ?= licenses
TRIVY_FS_FLAGS  ?= --cache-dir $(TRIVY_CACHE_DIR)
TRIVY_IMG_FLAGS ?= --cache-dir $(TRIVY_CACHE_DIR)

# Skip embedded UI build (build.rs reads SKIP_FRONTEND=1).
# Auto-enabled when bun is not on PATH unless explicitly set to 0.
SKIP_FRONTEND   ?=
ifeq ($(SKIP_FRONTEND),)
  ifeq ($(HAS_BUN),)
    SKIP_FRONTEND := 1
  endif
endif
export SKIP_FRONTEND

# -----------------------------------------------------------------------------
# Colorized output (disabled when not a TTY or NO_COLOR is set)
# -----------------------------------------------------------------------------
ifeq ($(NO_COLOR),)
  ifneq ($(shell test -t 1 && echo 1),)
    COLOR_RESET  := \033[0m
    COLOR_BOLD   := \033[1m
    COLOR_DIM    := \033[2m
    COLOR_RED    := \033[31m
    COLOR_GREEN  := \033[32m
    COLOR_YELLOW := \033[33m
    COLOR_BLUE   := \033[34m
    COLOR_CYAN   := \033[36m
  endif
endif

# Print a bold status line before each target runs.
define log
	@printf "$(COLOR_BOLD)==> $(1)$(COLOR_RESET)\n"
endef

# Require a command to exist or exit with a helpful message.
define require_cmd
	@command -v $(1) >/dev/null 2>&1 || { \
		printf "$(COLOR_RED)error: $(1) not found. Run 'make install-tools'.$(COLOR_RESET)\n" >&2; \
		exit 1; \
	}
endef

# Install a cargo extension on demand (audit, deny, llvm-cov, etc.).
define ensure_cargo_ext
	@if command -v cargo-$(1) >/dev/null 2>&1; then \
		:; \
	else \
		printf "$(COLOR_YELLOW)installing cargo-$(1)...$(COLOR_RESET)\n"; \
		$(CARGO) install cargo-$(1) --locked; \
	fi
endef

# =============================================================================
# Primary pipeline
# =============================================================================

.PHONY: all ci
all: ci ## Run the complete production validation pipeline

ci: sync-version ## Run full CI pipeline in order (stops on first failure)
	@if [ "$(SKIP_FRONTEND)" = "1" ] && [ -z "$(HAS_BUN)" ]; then \
		printf "$(COLOR_YELLOW)warning: bun not found; SKIP_FRONTEND=1 (minimal embedded UI)$(COLOR_RESET)\n"; \
		printf "$(COLOR_DIM)Install bun: make install-tools  |  Full UI: make frontend release$(COLOR_RESET)\n"; \
	fi
	$(MAKE) --no-print-directory fmt
	$(MAKE) --no-print-directory check
	$(MAKE) --no-print-directory lint
	$(MAKE) --no-print-directory test
	$(MAKE) --no-print-directory coverage
	$(MAKE) --no-print-directory audit
	$(MAKE) --no-print-directory deny
	$(MAKE) --no-print-directory trivy-fs
	$(MAKE) --no-print-directory secrets
	$(MAKE) --no-print-directory config-scan
	$(MAKE) --no-print-directory licenses
	$(MAKE) --no-print-directory sbom
	$(MAKE) --no-print-directory trivy-sbom
	$(MAKE) --no-print-directory doc
	$(call log,Freeing debug build artifacts before release)
	@$(CARGO) clean
	$(MAKE) --no-print-directory release
	$(MAKE) --no-print-directory image
	$(MAKE) --no-print-directory trivy-image
	$(call log,CI pipeline completed successfully)
	@printf "$(COLOR_GREEN)All CI stages passed.$(COLOR_RESET)\n"

# =============================================================================
# Rust quality
# =============================================================================

.PHONY: sync-version version fmt check lint test coverage audit deny npm-licenses doc frontend release

sync-version: ## Sync VERSION file into Cargo.toml and ui/package.json
	$(call log,Syncing semantic version from $(VERSION_FILE))
	@./scripts/sync-version.sh

version: ## Print the current semantic version from VERSION
	@printf '%s\n' '$(VERSION)'

fmt: ## Check Rust code formatting (cargo fmt --check)
	$(call log,Checking Rust formatting)
	$(CARGO) fmt --all -- --check

check: sync-version ## Static compile check for the workspace
	$(call log,Running cargo check --workspace)
	$(CARGO) check --workspace $(CARGO_FLAGS)

lint: ## Run Clippy with warnings denied
	$(call log,Running cargo clippy)
	$(CARGO) clippy $(CLIPPY_FLAGS) $(CARGO_FLAGS)

test: ## Run workspace unit and integration tests
	$(call log,Running cargo test --workspace --all-features)
	$(CARGO) test --workspace --all-features $(CARGO_FLAGS)

coverage: ## Generate LLVM code-coverage report
	$(call log,Running cargo llvm-cov)
	$(call ensure_cargo_ext,llvm-cov)
	@mkdir -p "$(COVERAGE_DIR)"
	$(CARGO) llvm-cov $(COVERAGE_FLAGS)
	@printf "$(COLOR_GREEN)Coverage report written to $(COVERAGE_DIR)/$(COLOR_RESET)\n"

audit: ## Audit dependencies for known security vulnerabilities
	$(call log,Running cargo audit)
	$(call ensure_cargo_ext,audit)
	$(CARGO) audit

deny: ## Validate dependency licenses (cargo-deny; default: licenses only)
	$(call log,Running cargo deny check $(DENY_FLAGS))
	$(call ensure_cargo_ext,deny)
	$(CARGO) deny check $(DENY_FLAGS)

deny-all: ## Run full cargo-deny (licenses, advisories, bans, sources)
	$(call log,Running full cargo deny check)
	$(call ensure_cargo_ext,deny)
	$(CARGO) deny check

npm-licenses: ## Audit ui/ runtime dependency licenses (P3-24)
	$(call log,Running npm license audit)
	@command -v bun >/dev/null 2>&1 || { \
		printf "$(COLOR_RED)error: bun required for npm license audit. Run 'make install-tools'.$(COLOR_RESET)\n" >&2; \
		exit 1; \
	}
	bash scripts/check-npm-licenses.sh

doc: ## Build Rust API documentation (no dependencies)
	$(call log,Building documentation)
	$(CARGO) doc --no-deps --workspace $(CARGO_FLAGS)
	@printf "$(COLOR_GREEN)Documentation available under target/doc/$(COLOR_RESET)\n"

frontend: ## Build embedded web console (requires bun)
	$(call log,Building frontend assets)
	@command -v bun >/dev/null 2>&1 || { \
		printf "$(COLOR_RED)error: bun not found. Run 'make install-tools' or set SKIP_FRONTEND=1.$(COLOR_RESET)\n" >&2; \
		exit 1; \
	}
	cd ui && bun install --frozen-lockfile
	cd ui && bun run build
	@printf "$(COLOR_GREEN)Frontend built in ui/build/$(COLOR_RESET)\n"

release: sync-version ## Build optimized release binaries
	$(call log,Building release binaries v$(VERSION))
	@if [ -n "$(HAS_BUN)" ] && [ "$(SKIP_FRONTEND)" != "1" ]; then \
		$(MAKE) --no-print-directory frontend; \
		env -u SKIP_FRONTEND $(CARGO) build --release $(BUILD_FLAGS) $(CARGO_FLAGS); \
	else \
		$(CARGO) build --release $(BUILD_FLAGS) $(CARGO_FLAGS); \
	fi
	@printf "$(COLOR_GREEN)Release binaries in target/release/$(COLOR_RESET)\n"

# =============================================================================
# Trivy — filesystem scanning
# =============================================================================

.PHONY: trivy-fs trivy-fs-critical trivy-sarif sbom trivy-sbom secrets config-scan licenses report

trivy-fs: ## Scan filesystem for vulnerabilities, secrets, and misconfigurations
	$(call log,Running Trivy filesystem scan)
	$(call require_cmd,$(TRIVY))
	@mkdir -p "$(TRIVY_CACHE_DIR)"
	$(TRIVY) fs \
		--scanners vuln,secret,misconfig \
		$(TRIVY_FS_FLAGS) \
		.

trivy-fs-critical: ## Scan filesystem; fail only on HIGH and CRITICAL vulnerabilities
	$(call log,Running Trivy filesystem scan (HIGH/CRITICAL only))
	$(call require_cmd,$(TRIVY))
	@mkdir -p "$(TRIVY_CACHE_DIR)"
	$(TRIVY) fs \
		--scanners vuln,secret,misconfig \
		--severity HIGH,CRITICAL \
		$(TRIVY_FS_FLAGS) \
		.

trivy-sarif: ## Generate SARIF report for GitHub Code Scanning
	$(call log,Generating Trivy SARIF report)
	$(call require_cmd,$(TRIVY))
	@mkdir -p "$(REPORT_DIR)" "$(TRIVY_CACHE_DIR)"
	$(TRIVY) fs \
		--scanners vuln,secret,misconfig \
		--format sarif \
		--output "$(REPORT_DIR)/$(SARIF_FILE)" \
		$(TRIVY_FS_FLAGS) \
		.
	@printf "$(COLOR_GREEN)SARIF report: $(REPORT_DIR)/$(SARIF_FILE)$(COLOR_RESET)\n"

sbom: ## Generate CycloneDX SBOM from filesystem
	$(call log,Generating CycloneDX SBOM)
	$(call require_cmd,$(TRIVY))
	@mkdir -p "$(TRIVY_CACHE_DIR)"
	$(TRIVY) fs \
		--format cyclonedx \
		--output "$(SBOM_FILE)" \
		$(TRIVY_FS_FLAGS) \
		.
	@printf "$(COLOR_GREEN)SBOM written to $(SBOM_FILE)$(COLOR_RESET)\n"

trivy-sbom: ## Scan generated SBOM for vulnerabilities
	$(call log,Scanning SBOM for vulnerabilities)
	$(call require_cmd,$(TRIVY))
	@test -f "$(SBOM_FILE)" || { \
		printf "$(COLOR_RED)error: $(SBOM_FILE) not found. Run 'make sbom' first.$(COLOR_RESET)\n" >&2; \
		exit 1; \
	}
	$(TRIVY) sbom \
		--severity UNKNOWN,LOW,MEDIUM,HIGH,CRITICAL \
		$(TRIVY_FS_FLAGS) \
		"$(SBOM_FILE)"

secrets: ## Run Trivy secret scanning only
	$(call log,Running Trivy secret scan)
	$(call require_cmd,$(TRIVY))
	@mkdir -p "$(TRIVY_CACHE_DIR)"
	$(TRIVY) fs \
		--scanners secret \
		$(TRIVY_FS_FLAGS) \
		.

config-scan: ## Run Trivy misconfiguration scanning only
	$(call log,Running Trivy misconfiguration scan)
	$(call require_cmd,$(TRIVY))
	@mkdir -p "$(TRIVY_CACHE_DIR)"
	$(TRIVY) fs \
		--scanners misconfig \
		$(TRIVY_FS_FLAGS) \
		.

licenses: ## Run Trivy license scanning
	$(call log,Running Trivy license scan)
	$(call require_cmd,$(TRIVY))
	@mkdir -p "$(TRIVY_CACHE_DIR)"
	$(TRIVY) fs \
		--scanners license \
		$(TRIVY_FS_FLAGS) \
		.

report: ## Generate JSON Trivy report (suitable for HTML conversion)
	$(call log,Generating Trivy JSON report)
	$(call require_cmd,$(TRIVY))
	@mkdir -p "$(REPORT_DIR)" "$(TRIVY_CACHE_DIR)"
	$(TRIVY) fs \
		--scanners vuln,secret,misconfig,license \
		--format json \
		--output "$(REPORT_DIR)/$(REPORT_FILE)" \
		$(TRIVY_FS_FLAGS) \
		.
	@printf "$(COLOR_GREEN)JSON report: $(REPORT_DIR)/$(REPORT_FILE)$(COLOR_RESET)\n"
	@printf "$(COLOR_DIM)Convert to HTML with your preferred SARIF/JSON viewer or CI integration.$(COLOR_RESET)\n"

# =============================================================================
# Container image
# =============================================================================

.PHONY: image trivy-image trivy-image-critical

image: sync-version ## Build Docker container image
	$(call log,Building Docker image $(IMAGE_REF))
	$(call require_cmd,$(DOCKER))
	$(DOCKER) build \
		--build-arg MAXIO_VERSION="$(VERSION)" \
		-t "$(IMAGE_REF)" \
		-f Dockerfile \
		.
	@printf "$(COLOR_GREEN)Image built: $(IMAGE_REF)$(COLOR_RESET)\n"

trivy-image: ## Scan Docker image for vulnerabilities
	$(call log,Scanning Docker image $(IMAGE_REF))
	$(call require_cmd,$(TRIVY))
	@mkdir -p "$(TRIVY_CACHE_DIR)"
	$(TRIVY) image \
		--scanners vuln,secret,misconfig \
		$(TRIVY_IMG_FLAGS) \
		"$(IMAGE_REF)"

trivy-image-critical: ## Scan Docker image; fail only on HIGH and CRITICAL
	$(call log,Scanning Docker image $(IMAGE_REF) (HIGH/CRITICAL only))
	$(call require_cmd,$(TRIVY))
	@mkdir -p "$(TRIVY_CACHE_DIR)"
	$(TRIVY) image \
		--scanners vuln,secret,misconfig \
		--severity HIGH,CRITICAL \
		$(TRIVY_IMG_FLAGS) \
		"$(IMAGE_REF)"

# =============================================================================
# Utility targets
# =============================================================================

.PHONY: clean update tree deps install-tools help

clean: ## Remove build artifacts, coverage output, and scan caches
	$(call log,Cleaning build artifacts)
	$(CARGO) clean
	@rm -rf "$(COVERAGE_DIR)" "$(TRIVY_CACHE_DIR)" "$(REPORT_DIR)" "$(SBOM_FILE)"
	@printf "$(COLOR_GREEN)Clean complete.$(COLOR_RESET)\n"

update: ## Update Cargo.lock dependency versions
	$(call log,Updating Cargo dependencies)
	$(CARGO) update
	@printf "$(COLOR_GREEN)Dependencies updated.$(COLOR_RESET)\n"

tree: ## Display dependency tree
	$(call log,Displaying dependency tree)
	$(CARGO) tree $(CARGO_FLAGS)

deps: ## Display workspace dependency metadata summary
	$(call log,Displaying dependency metadata)
	@$(CARGO) metadata --format-version 1 --no-deps \
		| python3 -c "\
import json,sys; \
data=json.load(sys.stdin); \
roots=[p for p in data.get('packages',[]) if p['name'] in ('maxio','maxio-admin')]; \
print('$(COLOR_BOLD)Workspace crates:$(COLOR_RESET)'); \
[print(f\"  - {p['name']} {p['version']} ({p.get('license','unknown')})\") for p in sorted(roots,key=lambda x:x['name'])]; \
print(); \
print('$(COLOR_BOLD)Direct dependencies (maxio):$(COLOR_RESET)'); \
root=next((p for p in roots if p['name']=='maxio'),None); \
deps=(root or {}).get('dependencies',[]); \
[print(f\"  - {d['name']} {d.get('req','')}\") for d in sorted(deps,key=lambda x:x['name'])] \
		" 2>/dev/null \
		|| $(CARGO) tree --depth 1

install-tools: ## Install required developer and security tooling (do not use sudo)
	@if [ -n "$${SUDO_UID:-}" ] || [ "$$(id -u)" -eq 0 ]; then \
		printf "$(COLOR_RED)error: do not run 'make install-tools' with sudo.$(COLOR_RESET)\n" >&2; \
		printf "$(COLOR_DIM)Rust/cargo tools install per-user under $$HOME/.cargo (run as your normal user).$(COLOR_RESET)\n" >&2; \
		printf "$(COLOR_DIM)For system-wide Trivy only: curl -sfL https://raw.githubusercontent.com/aquasecurity/trivy/main/contrib/install.sh | sudo sh -s -- -b /usr/local/bin$(COLOR_RESET)\n" >&2; \
		exit 1; \
	fi
	@test -x "$(RUSTUP)" || { \
		printf "$(COLOR_RED)error: rustup not found at $(RUSTUP)$(COLOR_RESET)\n" >&2; \
		printf "$(COLOR_DIM)Install Rust: https://rustup.rs/  then re-run make install-tools$(COLOR_RESET)\n" >&2; \
		exit 1; \
	}
	$(call log,Installing Rust toolchain components)
	"$(RUSTUP)" component add rustfmt clippy llvm-tools-preview rust-src
	$(call log,Installing bun (frontend toolchain))
	@if command -v bun >/dev/null 2>&1; then \
		printf "$(COLOR_GREEN)bun already installed: $$(bun --version)$(COLOR_RESET)\n"; \
	elif command -v unzip >/dev/null 2>&1; then \
		curl -fsSL https://bun.sh/install | bash; \
		printf "$(COLOR_GREEN)bun installed to $(BUN_INSTALL)/bin/bun$(COLOR_RESET)\n"; \
	else \
		printf "$(COLOR_YELLOW)warning: skipping bun install (unzip not found).$(COLOR_RESET)\n"; \
		printf "$(COLOR_DIM)  Install unzip for frontend builds, or use SKIP_FRONTEND=1 for Rust-only CI.$(COLOR_RESET)\n"; \
	fi
	$(call log,Installing cargo extensions)
	@command -v cargo-audit >/dev/null 2>&1 \
		|| "$(CARGO)" install cargo-audit --locked
	@command -v cargo-deny >/dev/null 2>&1 \
		|| "$(CARGO)" install cargo-deny --locked
	@command -v cargo-llvm-cov >/dev/null 2>&1 \
		|| "$(CARGO)" install cargo-llvm-cov --locked
	$(call log,Installing Trivy)
	@if command -v trivy >/dev/null 2>&1; then \
		printf "$(COLOR_GREEN)Trivy already installed: $$(trivy --version | head -1)$(COLOR_RESET)\n"; \
	else \
		mkdir -p "$(LOCAL_BIN)"; \
		curl -sfL https://raw.githubusercontent.com/aquasecurity/trivy/main/contrib/install.sh \
			| sh -s -- -b "$(LOCAL_BIN)"; \
		printf "$(COLOR_GREEN)Trivy installed to $(LOCAL_BIN)/trivy$(COLOR_RESET)\n"; \
	fi
	@printf "$(COLOR_GREEN)All developer tools are ready.$(COLOR_RESET)\n"

help: ## Show available targets and descriptions
	@printf "$(COLOR_BOLD)Available Targets$(COLOR_RESET)\n\n"
	@printf "$(COLOR_DIM)Usage: make <target>  |  default: make help$(COLOR_RESET)\n\n"
	@grep -hE '^[a-zA-Z][a-zA-Z0-9_.-]*:.*## ' $(MAKEFILE_LIST) \
		| sort -u \
		| awk 'BEGIN {FS = ":.*## "}; {printf "  $(COLOR_GREEN)make %-22s$(COLOR_RESET) %s\n", $$1, $$2}'
	@printf "\n$(COLOR_BOLD)Variables$(COLOR_RESET)\n\n"
	@printf "  $(COLOR_CYAN)IMAGE_NAME$(COLOR_RESET)      = $(IMAGE_NAME)\n"
	@printf "  $(COLOR_CYAN)VERSION$(COLOR_RESET)         = $(VERSION) (from $(VERSION_FILE))\n"
	@printf "  $(COLOR_CYAN)IMAGE_TAG$(COLOR_RESET)       = $(IMAGE_TAG)\n"
	@printf "  $(COLOR_CYAN)SBOM_FILE$(COLOR_RESET)       = $(SBOM_FILE)\n"
	@printf "  $(COLOR_CYAN)TRIVY_CACHE_DIR$(COLOR_RESET) = $(TRIVY_CACHE_DIR)\n"
	@printf "  $(COLOR_CYAN)SKIP_FRONTEND$(COLOR_RESET)   = $(SKIP_FRONTEND) (auto 1 when bun missing)\n"
	@printf "  $(COLOR_CYAN)HAS_BUN$(COLOR_RESET)         = $(if $(HAS_BUN),yes,no) ($(BUN))\n"
	@printf "  $(COLOR_CYAN)CARGO$(COLOR_RESET)           = $(CARGO)\n"
	@printf "  $(COLOR_CYAN)RUSTUP$(COLOR_RESET)          = $(RUSTUP)\n"
	@printf "  $(COLOR_CYAN)DENY_FLAGS$(COLOR_RESET)        = $(DENY_FLAGS) (default: licenses)\n"
	@printf "\n$(COLOR_BOLD)Examples$(COLOR_RESET)\n\n"
	@printf "  make install-tools   # run as normal user, not sudo\n"
	@printf "  make ci\n"
	@printf "  make test SKIP_FRONTEND=1\n"
	@printf "  make image                 # tags image as $(IMAGE_NAME):v\$$VERSION\n"
	@printf "  make deny-all          # full cargo-deny including advisories\n"
	@printf "  make trivy-fs-critical\n"