# Contributing

Commits follow [Conventional Commits](https://www.conventionalcommits.org/): `feat:` and `fix:`
bump the version, the rest are changelog only. release-please reads them.

Before a pull request:

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

A change to what the converter logs updates the table in `README.md` and in `crates/ulog-rrd/src/lib.rs`,
and the views in `crates/ulog-rrd-py/python/ulog_rrd/views.py` when a view should show it.
