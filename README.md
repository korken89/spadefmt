# spadefmt

[![unsafe forbidden](https://img.shields.io/badge/unsafe-forbidden-success.svg)](https://github.com/rust-secure-code/safety-dance/)

A tool for formatting Spade code according to style guidelines.

## Testing

`cargo test` checks the corpus under `asts/`: every `X.spade` input must
format to its `X-formatted.spade` golden twin, formatting must be idempotent,
and every file must either format, fail to parse, or report its unsupported
constructs as located errors - never panic (see `tests/golden.rs` for the
known exceptions). Constructs the formatter cannot print yet each have a
fixture under `asts/unsupported/`; implementing one moves its fixture up
into `asts/` as a golden pair. After an intentional formatting change,
regenerate the goldens with

```sh
SPADEFMT_BLESS=1 cargo test
```

and review the resulting diff before committing.

The full corpus includes the `asts/swim-templates` submodule; fetch it with
`git submodule update --init` (or clone with `--recursive`). Without it the
swim-templates part of the corpus sweep is skipped.
