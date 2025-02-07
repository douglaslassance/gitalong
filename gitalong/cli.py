import asyncio

import click
import git
import git.exc

from .__info__ import __version__
from .enums import CommitSpread
from .repository import Repository
from .batch import get_files_last_commits, claim_files, update_tracked_commits


def get_status_string(filename: str, commit: dict, spread: int) -> str:
    """Generate a status string for a file and its commit."""
    spread_string = "+" if spread & CommitSpread.MINE_UNCOMMITTED else "-"
    spread_string += "+" if spread & CommitSpread.MINE_ACTIVE_BRANCH else "-"
    spread_string += "+" if spread & CommitSpread.MINE_OTHER_BRANCH else "-"
    spread_string += "+" if spread & CommitSpread.REMOTE_MATCHING_BRANCH else "-"
    spread_string += "+" if spread & CommitSpread.REMOTE_OTHER_BRANCH else "-"
    spread_string += "+" if spread & CommitSpread.THEIR_OTHER_BRANCH else "-"
    spread_string += "+" if spread & CommitSpread.THEIR_MATCHING_BRANCH else "-"
    spread_string += "+" if spread & CommitSpread.THEIR_UNCOMMITTED else "-"
    splits = [
        spread_string,
        filename,
        commit.get("sha", "-"),
        ",".join(commit.get("branches", {}).get("local", ["-"])) or "-",
        ",".join(commit.get("branches", {}).get("remote", ["-"])) or "-",
        commit.get("host", "-"),
        commit.get("author", commit.get("user", "-")) or "-",
    ]
    return " ".join(splits)


def validate_key_value(ctx, param, value):  # pylint: disable=unused-argument
    """Validate that the provided value is a valid key-value."""
    result = {}
    for item in value:
        key, val = item.split("=")
        result[key] = val
    return result


@click.command(help="Prints the requested configuration property value.")
def version():  # pylint: disable=missing-function-docstring
    click.echo(f"gitalong version {__version__}")


@click.command(help="Prints the requested configuration property value.")
@click.argument("prop")
@click.pass_context
def config(ctx, prop):  # pylint: disable=missing-function-docstring
    repository = Repository.from_filename(ctx.obj.get("REPOSITORY", ""))
    repository_config = repository.config if repository else {}
    prop = prop.replace("-", "_")
    if prop in repository_config:
        value = repository_config[prop]
        if isinstance(value, bool):
            value = str(value).lower()
        click.echo(value)


@click.command(
    help=(
        "Update tracked commits with the local changes of this clone. echo a list of "
        "files their permissions changed."
    )
)
@click.pass_context
def update(ctx):
    """Update tracked commits with local changes."""
    repository = Repository.from_filename(ctx.obj.get("REPOSITORY", ""))
    if repository:
        asyncio.run(update_tracked_commits(repository))


@click.command(
    help=(
        "Prints missing commits in this local branch for each filename. "
        "Format: `<spread> <filename> <commit> <local-branches> "
        "<remote-branches> <host> <author>`"
    )
)
@click.argument("filename", nargs=-1)
@click.option(
    "-p",
    "--profile",
    is_flag=True,
    help=(
        "Will generate a profile file in the current working directory."
        "The file can be opened in a profiler like snakeviz."
    ),
)
@click.pass_context
def status(ctx, filename, profile=False):  # pylint: disable=missing-function-docstring
    if profile:
        import cProfile  # pylint: disable=import-outside-toplevel
        import pstats  # pylint: disable=import-outside-toplevel

        with cProfile.Profile() as pr:
            run_status(ctx, filename)
        results = pstats.Stats(pr)
        results.dump_stats("gitalong.prof")
        return
    run_status(ctx, filename)


def run_status(ctx, filename):  # pylint: disable=missing-function-docstring
    absolute_filenames = []
    for filename_ in filename:
        repository = Repository.from_filename(ctx.obj.get("REPOSITORY", filename_))
        absolute_filenames.append(
            repository.get_absolute_path(filename_) if repository else filename_
        )
    file_statuses = []
    last_commits = asyncio.run(get_files_last_commits(absolute_filenames))
    for filename_, last_commit in zip(filename, last_commits):
        file_statuses.append(
            get_status_string(filename_, last_commit, last_commit.commit_spread)
        )
    click.echo("\n".join(file_statuses), err=False)


@click.command(
    help=(
        "Make provided files writeable if possible. Return error code 1 if one or more "
        "files cannot be made writeable."
    )
)
@click.argument(
    "filename",
    nargs=-1,
    # help="The path to the file that should be made writable."
)
@click.pass_context
def claim(ctx, filename):  # pylint: disable=missing-function-docstring
    absolute_filenames = []
    for filename_ in filename:
        repository = Repository.from_filename(ctx.obj.get("REPOSITORY", filename_))
        absolute_filenames.append(
            repository.get_absolute_path(filename_) if repository else filename_
        )
    file_statuses = []
    blocking_commits = asyncio.run(claim_files(absolute_filenames))
    for filename_, blocking_commit in zip(filename, blocking_commits):
        file_statuses.append(
            get_status_string(filename_, blocking_commit, blocking_commit.commit_spread)
        )
    click.echo("\n".join(file_statuses), err=False)


@click.command(help="Setup Gitalong in a repository.")
@click.argument("store-url", required=True)
@click.option(
    "-sh",
    "--store-header",
    callback=validate_key_value,
    help="If using JSONBin.io as a store, the headers used to connect the endpoint.",
    required=False,
    multiple=True,
)
@click.option(
    "-mp",
    "--modify-permissions",
    is_flag=True,
    help=(
        "Whether or not Gitalong should affect file permissions of tracked files "
        "to prevent editing of files that are modified elsewhere. This is too "
        "expensive an option for repositories with many files and should be "
        "enabled."
    ),
)
@click.option(
    "-pt",
    "--pull-threshold",
    default=60,
    help=(
        "Time in seconds that need to pass before Gitalong pulls again. Defaults to 60"
        "seconds. This is for optimization sake as pull and fetch operations are "
        "expensive."
    ),
    required=False,
)
@click.option(
    "-tb",
    "--track-binaries",
    is_flag=True,
    help=(
        "Gitalong can track all auto-detected binary files "
        "to prevent conflicts on them. There is a performance cost to this feature so "
        "it's always better if you can specify the extensions you care about tracking "
        "using --tracked-extensions."
    ),
    required=False,
)
@click.option(
    "-tu",
    "--track-uncommitted",
    is_flag=True,
    help=(
        "Track uncommitted changes. Better for collaboration but requires pushing"
        "tracked commits after each file system operation."
    ),
)
@click.option(
    "-te",
    "--tracked-extensions",
    default="",
    help=("A comma-separated list of extensions to track to prevent conflicts."),
)
@click.option(
    "-ug",
    "--update-gitignore",
    is_flag=True,
    help=(".gitignore should be modified in the repository to ignore Gitalong files."),
)
@click.option(
    "-uh",
    "--update-hooks",
    is_flag=True,
    help="Hooks should be updated with Gitalong logic.",
)
@click.pass_context
def setup(  # pylint: disable=too-many-positional-arguments
    ctx,
    store_url,
    store_header,
    modify_permissions,
    pull_threshold,
    track_binaries,
    track_uncommitted,
    tracked_extensions,
    update_gitignore,
    update_hooks,
):
    """Setup Gitalong in a repository."""
    Repository.setup(
        store_url=store_url,
        store_headers=store_header,
        managed_repository=ctx.obj.get("REPOSITORY", ""),
        modify_permissions=modify_permissions,
        pull_threshold=pull_threshold,
        track_binaries=track_binaries,
        track_uncommitted=track_uncommitted,
        tracked_extensions=tracked_extensions.split(","),
        update_gitignore=update_gitignore,
        update_hooks=update_hooks,
    )


class Group(click.Group):  # pylint: disable=missing-class-docstring
    def format_help(self, ctx, formatter):
        return click.Group.format_help(self, ctx, formatter)


@click.group(cls=Group)
@click.version_option(
    prog_name="gitalong",
    version=__version__,
    message="%(prog)s version %(version)s",
)
@click.option(
    "-C",
    "--repository",
    default="",
    help=(
        "The repository to apply operations to. "
        "Defaults to current working directory."
    ),
    required=False,
)
@click.option(
    "-gb",
    "--git-binary",
    default="",
    help="Path to the git binary to use. Defaults to the one available in PATH.",
    required=False,
)
@click.pass_context
def cli(ctx, repository, git_binary):  # pylint: disable=missing-function-docstring
    ctx.ensure_object(dict)
    if repository:
        ctx.obj["REPOSITORY"] = repository
    if git_binary:
        git.refresh(git_binary)


cli.add_command(config)
cli.add_command(update)
cli.add_command(setup)
cli.add_command(status)
cli.add_command(claim)
cli.add_command(version)


def main():
    """This main function will be registered as the console script when installing the
    package.
    """
    cli(obj={})  # pylint: disable=unexpected-keyword-arg,no-value-for-parameter


if __name__ == "__main__":
    main()
