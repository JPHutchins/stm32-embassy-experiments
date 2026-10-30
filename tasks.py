from camas import Claude, Config, Parallel, Sequential, Task

fmt = Parallel(
    Task("cargo fmt", mutates=True),
    Task("nixfmt flake.nix", mutates=True),
    Task("ruff format", mutates=True),
    Task("taplo fmt", mutates=True),
    Task("yamlfmt .", mutates=True),
)

fmt_check = Parallel(
    "cargo fmt --check",
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

clippy = Task("cargo clippy --release -- -D warnings")
build = Task("cargo build --release")
check = Parallel(fmt_check, lint, Sequential(clippy, build, name="cargo"))

flash = Task("cargo run --release")

_ = Config(
    default_task=check,
    github_task=check,
    agent=Claude(fix=fmt, check=check),
)
