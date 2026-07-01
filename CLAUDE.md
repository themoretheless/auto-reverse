# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project state

This is a freshly scaffolded Rust binary crate (`cargo new`), currently just the
default "Hello, world!" in [src/main.rs](src/main.rs). There is no architecture,
module structure, or dependency set established yet — this section should be
expanded as the codebase grows.

## Commands

- Build: `cargo build`
- Run: `cargo run`
- Check (fast type/borrow check without producing a binary): `cargo check`
- Test: `cargo test` (run a single test with `cargo test <test_name>`)
- Format: `cargo fmt`
- Lint: `cargo clippy`
