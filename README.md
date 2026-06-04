# gproxy v2

Full rewrite of gproxy as a single-crate Rust binary with cross-model
load balancing and multi-instance support.

- Design: [docs/architecture-design.md](docs/architecture-design.md)
- Deployment entries and platform spikes: [deploy/](deploy/)
- Engineering conventions: inherited from the parent repo `CLAUDE.md`
  (file size limits, no TDD / no over-testing, check-before-write,
  `cargo fmt` + `cargo clippy` after every change).

This directory is an **isolated git repository**, intentionally ignored
by the parent repo (`/v2` in the parent `.gitignore`). It will be merged
into the main repo once v2 stabilises.
