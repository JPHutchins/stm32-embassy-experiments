from camas import Claude, Config, Parallel, Sequential, Task

fmt = Parallel(
    Task("cargo fmt", mutates=True),
    Task("nixfmt flake.nix", mutates=True),
    Task("ruff format", mutates=True),
    Task("taplo fmt", mutates=True),
)

fmt_check = Parallel(
    "cargo fmt --check",
    "nixfmt --check flake.nix",
    "ruff format --check",
    "taplo fmt --check",
)
clippy = Task("cargo clippy --release -- -D warnings")
build = Task("cargo build --release")
check = Parallel(fmt_check, Sequential(clippy, build, name="cargo"))

flash = Task("cargo run --release")

_ = Config(
    default_task=check,
    github_task=check,
    agent=Claude(fix=fmt, check=check),
)
