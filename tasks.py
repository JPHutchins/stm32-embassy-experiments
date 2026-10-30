from camas import Claude, Config, Parallel, Sequential, Task

fmt = Parallel(
    Task("cargo fmt --all", mutates=True),
    Task("nixfmt flake.nix", mutates=True),
    Task("ruff format", mutates=True),
    Task("taplo fmt", mutates=True),
    Task("yamlfmt .", mutates=True),
)

fmt_check = Parallel(
    "cargo fmt --all --check",
    "nixfmt --check flake.nix",
    "ruff format --check",
    "taplo fmt --check",
    "yamlfmt -lint .",
)

lint = Parallel(
    "ruff check",
    "camas --check",
    "actionlint",
    "zizmor .",
)

clippy = Task("cargo clippy --workspace --release -- -D warnings")
clippy_app = Task(
    "cargo clippy -p app --all-targets --target host-tuple -- -D warnings"
)
test = Task("cargo test -p app --target host-tuple")
build = Task("cargo build --release")
check = Parallel(
    fmt_check, lint, Sequential(clippy, clippy_app, test, build, name="cargo")
)

flash = Task("cargo run --release")

_ = Config(
    default_task=check,
    github_task=check,
    agent=Claude(fix=fmt, check=check),
)
