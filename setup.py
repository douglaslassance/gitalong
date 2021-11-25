"""Setup for gitarmony.
"""

import os

from setuptools import setup

dirname = os.path.dirname(__file__)
info = {}
with open(os.path.join(dirname, "gitarmony", "__info__.py"), mode="r") as f:
    exec(f.read(), info)  # pylint: disable=W0122

# Get the long description from the README file.
with open(os.path.join(dirname, "README.md"), encoding="utf-8") as fle:
    long_description = fle.read()

setup(
    name="gitarmony",
    version=info.get("__version__", ""),
    description="An API to perform gitarmony operation on Git repositories.",
    long_description=long_description,
    long_description_content_type="text/markdown",
    url="https://github.com/douglaslassance/gitarmony-python",
    author=info.get("__author__", ""),
    author_email=info.get("__email__", ""),
    license=info.get("__license__", ""),
    packages=["gitarmony"],
    install_requires=["GitPython~=3.1", "configparser~=5.1", "dictdiffer~=0.9"],
    extras_require={
        "ci": [
            "flake8-print~=3.1",
            "flake8~=3.8",
            "pep8-naming~=0.11",
            "pytest-cov~=2.10.1",
            "pytest-html~=2.1.1",
            "pytest-pep8~=1.0.6",
            "pytest~=6.1.1",
            "requests-mock~=1.8",
            "sphinx-markdown-tables~=0.0",
            "sphinx-rtd-theme~=0.5",
            "sphinxcontrib-apidoc~=0.3",
            "Sphinx~=3.2",
        ],
    },
    include_package_data=True,
)
