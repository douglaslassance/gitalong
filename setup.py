"""Setup for gitalong.
"""

import os

from setuptools import setup

dirname = os.path.dirname(__file__)
info = {}
with open(
    os.path.join(dirname, "gitalong", "__info__.py"), mode="r", encoding="utf-8"
) as f:
    exec(f.read(), info)  # pylint: disable=W0122

# Get the long description from the README file.
with open(os.path.join(dirname, "README.md"), encoding="utf-8") as fle:
    long_description = fle.read()

setup(
    name="gitalong",
    version=info.get("__version__", ""),
    description=("Git without conflicts."),
    long_description=long_description,
    long_description_content_type="text/markdown",
    url="https://github.com/douglaslassance/gitalong-python",
    author=info.get("__author__", ""),
    author_email=info.get("__email__", ""),
    license=info.get("__license__", ""),
    packages=["gitalong", "gitalong.stores"],
    install_requires=[
        "click~=8.1",
        "dictdiffer~=0.9",
        "gitdb~=4.0",
        "GitPython~=3.1",
        "requests~=2.31",
    ],
    extras_require={
        "ci": [
            "black~=24.10",
            "pep8-naming~=0.13",
            "Pillow~=10.4",
            "pylint~=3.0",
            "pytest-cov~=4.1",
            "pytest-html~=4.1",
            "pytest-profiling~=1.7",
            "responses~=0.24",
            "setuptools~=69.0",
            "sphinx-markdown-tables~=0.0",
            "sphinx-rtd-theme~=2.0",
            "sphinxcontrib-apidoc~=0.5",
            "Sphinx~=7.2",
            "twine~=4.0",
            "wheel~=0.42",
        ],
    },
    entry_points={
        "console_scripts": [
            "gitalong = gitalong.cli:main",
        ]
    },
    include_package_data=True,
    python_require="~=3.7",
)
