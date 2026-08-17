# spadefmt

[![unsafe forbidden](https://img.shields.io/badge/unsafe-forbidden-success.svg)](https://github.com/rust-secure-code/safety-dance/)

A tool for formatting Spade code according to style guidelines.

## Testing

`cargo test` checks the corpus under `asts/`: every `X.spade` input must
format to its `X-formatted.spade` golden twin, formatting must be idempotent,
and no file may make the formatter panic (see `tests/golden.rs` for the known
exceptions). After an intentional formatting change, regenerate the goldens
with

```sh
SPADEFMT_BLESS=1 cargo test
```

and review the resulting diff before committing.
