# gitalong

Gitalong helps teams sharing a Git repository avoid stepping on each other's
files. Each clone publishes the changes it has in flight (committed but
unpushed work, plus uncommitted edits when the option is on) to a shared
*store*, and the CLI lets anyone query "who's currently editing this file?"
or "can I claim this file for editing?".

> [!NOTE]
> The `1.x` line is a complete rewrite in Rust, distributed as a single
> binary. The Python `0.x` releases (and the `gitalong` PyPI package) live on
> the [`python`](https://github.com/douglaslassance/gitalong/tree/python)
> branch and are kept in maintenance mode for legacy users.

## Install

A single static binary, no Python runtime required.

```shell
# Homebrew (preferred)
brew install douglaslassance/tap/gitalong

# From source via Cargo
cargo install gitalong
```

Requires Git ≥ 2.35 on `PATH`.

## Usage

```shell
# Stand up a project repository and a clone of it.
git init --bare project.git
git clone project.git project

# Stand up the store gitalong will use to share local changes between clones.
# In production this lives on GitHub or another shared remote.
git init --bare store.git

# Initialize gitalong in the project clone.
gitalong -C project setup ./store.git \
  --modify-permissions \
  --tracked-extensions .jpg,.png \
  --track-uncommitted \
  --update-gitignore \
  --update-hooks

# Make a few files spread across local, remote, and uncommitted state.
touch project/untracked.txt
touch project/uncommitted.png
touch project/local.png
touch project/current.jpg
touch project/remote.jpg

git -C project add current.jpg && git -C project commit -m "Add current.jpg"
git -C project add remote.jpg  && git -C project commit -m "Add remote.jpg"
git -C project push
git -C project reset --hard HEAD^
git -C project add local.png   && git -C project commit -m "Add local.png"

# Push this clone's view of the world to the store.
# (The post-commit / post-checkout / post-rewrite hooks installed by
# `--update-hooks` do this automatically.)
gitalong -C project update

# What's the situation for each file?
gitalong -C project status untracked.txt uncommitted.png local.png current.jpg remote.jpg

# Claim files for editing. Returns exit 1 when any are blocked.
gitalong -C project claim untracked.txt uncommitted.png local.png current.jpg remote.jpg
```

### Status output

Each line is:

```text
<spread> <filename> <sha> <local-branches> <remote-branches> <host> <author>
```

`<spread>` is an eight-character `+`/`-` bitstring describing where this
commit lives. The bits, in order, are:

| # | Flag                       | Meaning |
|---|----------------------------|---------|
| 1 | `MINE_UNCOMMITTED`         | Uncommitted on this clone |
| 2 | `MINE_ACTIVE_BRANCH`       | On this clone's active branch |
| 3 | `MINE_OTHER_BRANCH`        | On a different local branch |
| 4 | `REMOTE_MATCHING_BRANCH`   | On the remote branch matching the active one |
| 5 | `REMOTE_OTHER_BRANCH`      | On a different remote branch |
| 6 | `THEIR_OTHER_BRANCH`       | On someone else's non-matching branch |
| 7 | `THEIR_MATCHING_BRANCH`    | On someone else's matching branch |
| 8 | `THEIR_UNCOMMITTED`        | Uncommitted on someone else's clone |

## Stores

### Git repository

The simplest backend: any git repository the team can read and push to. Set
`store_url` to the clone URL ending in `.git` and gitalong will clone it into
`<repo>/.gitalong/` on first use, then commit and push `commits.json` updates
from there.

### JSONBin.io

A hosted alternative for teams that don't want a second git repo. Set
`store_url` to the bin URL and pass an access key via `--store-header`:

```shell
gitalong -C project setup https://api.jsonbin.io/v3/b/<BIN_ID> \
  --store-header X-Access-Key=$ACCESS_KEY
```

`$ACCESS_KEY` is expanded from the environment at request time, so the secret
itself doesn't end up in the on-disk config.

## Development

```shell
# Build and test
cargo build
cargo test

# Lint
cargo clippy --all-targets -- -D warnings

# Run the CLI from a checkout
cargo run -- --help
```

Distribution builds use the `vendored` feature so libgit2 is statically
linked and the resulting binary is self-contained:

```shell
cargo build --release --features vendored
```

## License

[MIT](LICENSE)
