# spadefmt

[![unsafe forbidden](https://img.shields.io/badge/unsafe-forbidden-success.svg)](https://github.com/rust-secure-code/safety-dance/)

A tool for formatting Spade code according to style guidelines.

## Usage

```sh
spadefmt main.spade                # formatted source to stdout
spadefmt --in-place src/           # rewrite every *.spade under src/
spadefmt --check src/ main.spade   # list files that would change
spadefmt - < main.spade            # stdin to stdout
```

A directory is searched recursively for `*.spade` files (symlinked
directories are not followed); `-` reads stdin and writes stdout. Printing
to stdout takes exactly one input, while `--check` and `--in-place` take any
number. `--in-place` only writes files whose text changes, and `--check`
writes nothing: it prints `would reformat: <path>` for each file that would
change and exits 1 if there is any. An empty file stays empty.

### Configuration

`spadefmt.toml` is optional. For each input, spadefmt looks for one in the
file's directory and then in each parent directory up to the root, and uses
the first it finds (stdin looks from the working directory). Without one
the defaults apply. `--config <path>` uses that file instead. An unknown
key is an error.

```toml
max_width = 100   # line width to aim for (default 100)
indent = 4        # spaces per indent level (default 4)
```

### Exit codes

| Code | Meaning |
|------|---------|
| 0 | formatted, or already formatted |
| 1 | `--check` found a file that would change |
| 2 | usage, IO, or config error |
| 3 | an input does not parse |
| 4 | an input contains a construct spadefmt cannot format yet |

Every input is processed even after a failure, every diagnostic is printed,
and the exit code is the worst outcome. spadefmt's own errors print as
`error: <message>` on stderr, followed by `caused by:` lines where there is
a cause; `--no-color` or a non-empty `NO_COLOR` disables colors.

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

and review the resulting diff before committing. `tests/cli.rs` drives the
built binary through config discovery, the output modes, and the exit codes.

The full corpus includes the `asts/swim-templates` submodule; fetch it with
`git submodule update --init` (or clone with `--recursive`). Without it the
swim-templates part of the corpus sweep is skipped.

## Nix

The repository is a flake. Run it without installing:

```
nix run github:korken89/spadefmt -- --check src/
```

Use it from another flake:

```nix
{
  inputs.spadefmt.url = "github:korken89/spadefmt";

  outputs = { self, nixpkgs, spadefmt, ... }: {
    # a package: spadefmt.packages.${system}.default
    # or an overlay: nixpkgs.overlays = [ spadefmt.overlays.default ];
  };
}
```

`nix develop` gives a shell with the pinned stable toolchain, rust-analyzer,
and a nightly `rustfmt` wired to `cargo fmt` (the repo's `.rustfmt.toml`
uses nightly-only options).
