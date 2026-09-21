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

# Initialize gitalong in the project clone. With no store URL, clones share
# their local changes through hidden refs on the project's own remote.
gitalong -C project setup \
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

# Withdraw this clone's records, for instance before deleting the clone.
gitalong -C project clear

# Wipe the whole store, every clone included. Asks before it does.
gitalong -C project clear --all
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

The optional `<STORE_URL>` argument to `gitalong setup` selects how this clone
publishes its tracked changes for the rest of the team.

### Repository refs

Omit the argument. Each clone pushes its records to its own ref under
`refs/gitalong/v1/` on the repository's remote, with the same credentials as
any other push. The ref points at a commit with an empty tree and the records
in its message, so no file content is ever uploaded. Ordinary fetches never
see the namespace, hosting UIs hide it, and CI does not run on it. The `v1`
is the record format, bumped only if the payload ever changes shape, so that
clones running different versions never misread each other.

A namespace belongs to exactly one repository, so gitalong does not care how
each clone spells its remote. Teammates on SSH and on HTTPS see each other.

> [!NOTE]
> Servers that deny non-fast-forward pushes or restrict ref namespaces
> (Gerrit, for example) need an allowance for `refs/gitalong/*`, or one of
> the stores below.

### Git repository

Pass a repository URL or path. Gitalong clones it into
`<repo>/.gitalong/` on first use, then commits and pushes `commits.json`
updates from there.

> [!WARNING]
> Low infrastructure hassle but operations are slow.

> [!IMPORTANT]
> One store can serve several repositories, so records are tagged with the
> remote they belong to and read back by exact match. Every clone of a
> repository must therefore spell its remote the same way: a clone on
> `git@github.com:you/project.git` and one on
> `https://github.com/you/project.git` will not see each other. The refs
> store has no such requirement.

### JSONBin.io

Pass the bin URL (`https://api.jsonbin.io/v3/b/<id>`) and an access key via
`--store-header`. Any other HTTP endpoint that answers `GET` with
`{"record": [...]}` and accepts the array on `PUT` works the same way.

```shell
gitalong -C project setup https://api.jsonbin.io/v3/b/<BIN_ID> \
  --store-header X-Access-Key=$ACCESS_KEY
```

> [!NOTE]
> `$ACCESS_KEY` is expanded from the environment at request time, so the secret
> itself doesn't end up in the on-disk config.

## Clearing the store

`gitalong clear` withdraws the records this clone published. Run it before
deleting a clone, otherwise its last records sit in the store and keep
claiming files nobody is working on. In a live clone the effect is temporary:
the next commit or `gitalong update` republishes whatever is genuinely
unpushed or uncommitted.

`gitalong clear --all` removes every clone's records. It asks for
confirmation, and `--force` skips the question. Without a terminal to ask on,
it refuses unless `--force` is given, so a hook or a script can never wipe the
team's records by accident.

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
