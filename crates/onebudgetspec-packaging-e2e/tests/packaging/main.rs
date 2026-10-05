//! The packaging journeys: each distribution built by `scripts/build-dist.sh`, installed
//! into a fresh environment from the local artifact, and driven as a user would. AGENTS.md
//! lists what each journey file proves.

mod common;

mod cli_wheel;
mod npm_launcher;
mod sdk_python;
mod sdk_typescript;
