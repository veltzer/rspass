# rspass

A Rust clone of [pass(1)](https://www.passwordstore.org/), the standard unix
password manager, built on the same project skeleton as
[rsconstruct](https://github.com/veltzer/rsconstruct) (clap CLI framework,
CI/CD workflows, Tera templating).

Like pass, rspass never implements crypto itself: every entry is a
`NAME.gpg` file encrypted by shelling out to GnuPG, recipients are resolved
from the nearest `.gpg-id` walking up toward the store root, and if the
store is a git repository every change is committed automatically. Stores
created by pass and rspass are interchangeable.

## Usage

```console
$ rspass init mark.veltzer@gmail.com     # create ~/.password-store
$ rspass generate web/github 32          # generate + store a password
$ rspass insert mail/proton              # prompt (hidden, twice) and store
$ rspass                                 # tree listing, like bare `pass`
$ rspass show web/github                 # decrypt and print
$ rspass show -c web/github              # copy to clipboard, clear in 45s
$ rspass edit web/github                 # $EDITOR on a /dev/shm temp file
$ rspass mv web/github work/github       # rename (re-encrypts if .gpg-id differs)
$ rspass git log                         # git passthrough inside the store
$ rspass complete bash                   # shell completions
```

The store location is `--store DIR`, else `$PASSWORD_STORE_DIR`, else
`~/.password-store`. `$PASSWORD_STORE_CLIP_TIME` and
`$PASSWORD_STORE_GPG_OPTS` are honored like in pass.

## Tera entry templates

Entry bodies can be rendered from [Tera](https://keats.github.io/tera/)
templates stored in `<store>/.templates/*.tera`:

```console
$ cat ~/.password-store/.templates/login.tera
{{ gen_password(length=32) }}
user: {{ user }}
url: {{ url }}
created: {{ now() }}

$ rspass insert --template login --var user=alice --var url=https://example.com web/example
$ rspass templates list
```

Templates get the full Tera language (`{% include %}` resolves against
sibling templates) plus two built-in functions:

* `gen_password(length=25, symbols=true)` — a fresh random password
* `now(format="%Y-%m-%d")` — the current local time

## Building

```console
$ cargo build --release
$ cargo test          # includes end-to-end tests against a throwaway GPG key
```

Releases are cut by pushing a `v*` tag (see `release.toml` /
`cargo release`); `.github/workflows/release.yml` builds Linux and macOS
binaries for x86_64 and aarch64 and attaches them to a GitHub release.
