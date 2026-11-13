# Git Along

Git Along is a CLI for Git repositories that seeks to prevent conflicts between files when working with a team. It uses hooks and a store to communicate local changes across all clones of a given remote. In turns this information can be leveraged to prevent modifying files that are already changed elsewhere.

## Install

```shell
# Via Homebrew
brew install douglaslassance/douglaslassance-tap/git-along

# From source via Cargo
cargo install --git https://github.com/douglaslassance/git-along
```

> [!NOTE]
> Binaries for all systems can also be downloaded [here](https://github.com/douglaslassance/git-along/releases).

## Usage

> [!WARNING]
> This following assumes you have [Git](https://git-scm.com/) 2.35 or later installed on your system.

```shell
# Stand up a project repository and a clone of it.
git init --bare project.git
git clone project.git project

# Set up Git Along in the project clone. With no store URL, clones share
# their local changes through hidden refs on the project's own remote.
git -C project along setup \
  --modify-permissions \
  --tracked-extensions .jpg,.png \
  --track-uncommitted \
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
git -C project along update

# What's the situation for each file?
git -C project along status untracked.txt uncommitted.png local.png current.jpg remote.jpg

# Claim files for editing. Returns exit 1 when any are blocked.
git -C project along claim untracked.txt uncommitted.png local.png current.jpg remote.jpg

# Withdraw this clone's records, for instance before deleting the clone.
git -C project along clear

# Wipe the whole store, every clone included. Asks before it does.
git -C project along clear --all
```

### Status output

`status` and `claim` print one line per file, in the order you passed them:

```text
<spread> <filename> <sha> <local-branches> <remote-branches> <host> <author>
```

This form is meant for reading in a terminal. Fields are separated by single
spaces and nothing is quoted, so a filename containing a space cannot be
recovered from it. Scripts should pass `--json` instead.

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

### JSON output

`--json` prints a single array instead, one object per file in the order you
passed them:

```json
[
  {
    "filename": "current.jpg",
    "spread": "-+-+----",
    "flags": ["MINE_ACTIVE_BRANCH", "REMOTE_MATCHING_BRANCH"],
    "commit": {
      "sha": "9f3c1d2e5a7b4c8d9e0f1a2b3c4d5e6f7a8b9c0d",
      "date": "2026-09-20 10:12:03+00:00",
      "author": "Douglas Lassance",
      "changes": ["current.jpg"],
      "branches": { "local": ["main"], "remote": ["main"] }
    }
  }
]
```

`commit` is the store record verbatim, so its fields match `commits.json` and
the git identity (`author`) stays distinct from the OS identity (`user`). It is
`null` when nothing applies, which the text form renders as all dashes.
`claim --json` adds a `blocked` boolean per file and keeps the same exit code.

## Stores

The optional `<STORE_URL>` argument to `git along setup` selects how this clone
publishes its tracked changes for the rest of the team.

### Repository refs

Omit the argument. Each clone pushes its records to its own ref under
`refs/along/v1/` on the repository's remote, with the same credentials as
any other push. The ref points at a commit with an empty tree and the records
in its message, so no file content is ever uploaded. Ordinary fetches never
see the namespace, hosting UIs hide it, and CI does not run on it. The `v1`
is the record format, bumped only if the payload ever changes shape, so that
clones running different versions never misread each other.

A namespace belongs to exactly one repository, so Git Along does not care how
each clone spells its remote. Teammates on SSH and on HTTPS see each other.

> [!NOTE]
> Servers that deny non-fast-forward pushes or restrict ref namespaces
> (Gerrit, for example) need an allowance for `refs/along/*`, or one of
> the stores below.

### Git repository

Pass a repository URL or path. Git Along clones it into
`.git/along/store` on first use, then commits and pushes `commits.json`
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
git -C project along setup https://api.jsonbin.io/v3/b/<BIN_ID> \
  --store-header X-Access-Key=$ACCESS_KEY
```

> [!NOTE]
> `$ACCESS_KEY` is expanded from the environment at request time, so the secret
> itself doesn't end up in the on-disk config.

## Clearing the store

`git along clear` withdraws the records this clone published. Run it before
deleting a clone, otherwise its last records sit in the store and keep
claiming files nobody is working on. In a live clone the effect is temporary:
the next commit or `git along update` republishes whatever is genuinely
unpushed or uncommitted.

`git along clear --all` removes every clone's records. It asks for
confirmation, and `--force` skips the question. Without a terminal to ask on,
it refuses unless `--force` is given, so a hook or a script can never wipe the
team's records by accident.

## Upgrading from Gitalong

Git Along was called Gitalong until 1.0.0. The `gitalong` command is now
`git along`, and `.gitalong.json` is no longer read. Run `git along setup`
again with the same options, commit the new config, and delete
`.gitalong.json` along with the `# Gitalong` block in `.gitignore`.

Every clone then carries over on its own the next time any command runs. Its
hooks are rewritten, and its state and caches move into `.git/along/`.

Each clone also moves its records from `refs/gitalong/v1/` to
`refs/along/v1/` the next time it publishes. Upgraded clones still read the
old refs, but clones on Gitalong cannot see upgraded ones, so upgrade the
whole team together.

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

## Trademark

Git and the Git logo are either registered trademarks or trademarks of Software Freedom Conservancy, Inc., corporate home of the Git Project, in the United States and/or other countries. Git Along is not affiliated with or endorsed by the Git Project.
