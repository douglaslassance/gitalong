# Gitalong

Gitalong is a CLI for Git repositories that seeks to prevent conflicts between files when working with a team. It uses hooks and a store to communicate local changes across all clones of a given remote. In turns this information can be leveraged to prevent modifying files that are already changed elsewhere.

## Install

```shell
# Via Homebrew
brew install douglaslassance/tap/gitalong

# From source via Cargo
cargo install gitalong
```

> [!NOTE]
> Binaries for all systems can also be downloaded [here](https://github.com/douglaslassance/gitalong/releases).

## Usage

> [!WARNING]
> This following assumes you have [Git](https://git-scm.com/) 2.35 or later installed on your system.

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

| #   | Flag                     | Meaning                                      |
| --- | ------------------------ | -------------------------------------------- |
| 1   | `MINE_UNCOMMITTED`       | Uncommitted on this clone                    |
| 2   | `MINE_ACTIVE_BRANCH`     | On this clone's active branch                |
| 3   | `MINE_OTHER_BRANCH`      | On a different local branch                  |
| 4   | `REMOTE_MATCHING_BRANCH` | On the remote branch matching the active one |
| 5   | `REMOTE_OTHER_BRANCH`    | On a different remote branch                 |
| 6   | `THEIR_OTHER_BRANCH`     | On someone else's non-matching branch        |
| 7   | `THEIR_MATCHING_BRANCH`  | On someone else's matching branch            |
| 8   | `THEIR_UNCOMMITTED`      | Uncommitted on someone else's clone          |

## Stores

The `<STORE_URL>` argument to `gitalong setup` selects how this clone
publishes its tracked changes for the rest of the team.

### Git repository

Pass a repository URL or path. Gitalong clones it into
`<repo>/.gitalong/` on first use, then commits and pushes `commits.json`
updates from there.

> [!WARNING]
> Low infrastructure hassle but operations are slow.

### JSONBin.io

Pass the bin URL (`https://api.jsonbin.io/v3/b/<id>`) and an access key via
`--store-header`:

```shell
gitalong -C project setup https://api.jsonbin.io/v3/b/<BIN_ID> \
  --store-header X-Access-Key=$ACCESS_KEY
```

> [!NOTE]
> `$ACCESS_KEY` is expanded from the environment at request time, so the secret
> itself doesn't end up in the on-disk config.

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
