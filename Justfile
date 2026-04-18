set dotenv-load := true
cargo-hack      := `which cargo-hack`

# Print this list
default:
    just --list

################################################################################
# DEVELOPMENT
################################################################################

# Run all checks (~local CI, but allows warnings)
[group('development')]
ci: fmt lint doc test (coverage "lcov") coverage
    @echo "All checks passed! :)"

################################################################################
# QUALITY & LINTING
################################################################################

# Format code
[group('lint')]
[arg("check", long="check", value="true")]
fmt check="":
    cargo +nightly fmt --all {{ if check == "" { "" } else { "-- --check" } }}

# Run clippy
[group('lint')]
[arg("fix", long="fix", value="true")]
[arg("force", long="force", value="true")]
[arg("hack", long="hack", value="true")]
[arg("extra", long="extra", value="true")]
[arg("meta", long="meta", value="true")]
[arg("docs", long="docs", value="true")]
clippy fix="" force="" hack="" extra="" meta="" docs="":
    cargo {{ if hack == "" { "" } else { "hack" } }} clippy \
        {{ if hack == "" { "--all-features" } else { "--feature-powerset" } }} \
        --all-targets --workspace \
        {{ if fix == "" { "" }
            else if force == "" { "--fix" }
            else { "--fix --allow-dirty" }
        }} \
        -- \
        {{ if extra == "" {
            ""
        } else {
            "-Wclippy::all -Wclippy::pedantic -Wclippy::nursery"
        } }} \
        {{ if meta == "" {
            ""
        } else {
            ""
        } }} \
        {{ if docs == "" { ""
        } else { "" } }}

alias c := clippy

# Run clippy with all lints
[group('lint')]
lint: (clippy "" "" "y" "y" "y" "y")

# Run tests
[group('lint')]
test:
    cargo hack test --all-targets --feature-powerset --workspace --exclude e2e
    # cargo hack test --doc --all-features --workspace

alias t := test

################################################################################
# COVERAGE
################################################################################

# Generate code coverage report
[group('lint')]
[arg("format", long="format")] 
coverage format="":
    cargo llvm-cov --all-targets --all-features --workspace {{
        if format == "lcov" {
            "--lcov --output-path lcov.info"
        } else if format == "html" {
            "--html"
        } else if format == "text" {
            "--text"
        } else {
            ""
        }
    }}

alias cov := coverage

################################################################################
# DOCUMENTATION
################################################################################

# Build docs
[group('documentation')]
[arg("no_deps", long="no-deps", value="true")]
[arg("private", long="private", value="true")]
[arg("open", long="open", value="true")]
doc no_deps="" private="" open="":
    RUSTDOCFLAGS="${RUSTDOCFLAGS:-} -Zunstable-options \
                  --default-theme=ayu --cfg docsrs" \
    cargo +nightly hack doc --workspace --feature-powerset \
        {{ if no_deps == "" { "" } else { "--no-deps" } }} \
        {{ if private == "" { "" } else { "--document-private-items" } }} \
        {{ if open == "" { "" } else { "--open" } }}

# Build docs for all features and dependencies and open in default browser
[group('documentation')]
doco:
    RUSTDOCFLAGS="${RUSTDOCFLAGS:-} -Zunstable-options \
                  --default-theme=ayu --cfg docsrs" \
    cargo +nightly doc --all-features --open \
        $(cargo tree --depth 1 -e normal --prefix none \
          | cut -d' ' -f1 \
          | xargs printf -- '-p %s ')

################################################################################
# CLEANUP
################################################################################

# Clean build artifacts, docs, or docker resources. "build" (default), "docs", "docker", or "all"
[group('cleanup')]
clean what="build":
    {{ if what == "build" { "cargo clean" } else { "" } }}
    {{ if what == "docs" { "cargo clean --doc" } else { "" } }}
    {{ if what == "docker" {
        "just docker-down && docker system prune -f"
    } else { "" } }}
    {{ if what == "all" {
        "just clean build && just clean docker"
    } else { "" } }}
