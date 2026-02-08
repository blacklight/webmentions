import os
import sys

project = "webmentions"

sys.path.insert(0, os.path.abspath("../src/python"))

extensions = [
    "sphinx.ext.autodoc",
    "sphinx.ext.autosummary",
    "myst_parser",
    "sphinx.ext.napoleon",
    "sphinx.ext.viewcode",
]

autosummary_generate = True

autodoc_mock_imports = [
    "fastapi",
    "flask",
    "sqlalchemy",
    "watchdog",
    "watchdog.events",
    "watchdog.observers",
]

templates_path = ["_templates"]
exclude_patterns = ["_build"]

source_suffix = {
    ".rst": "restructuredtext",
    ".md": "markdown",
}

suppress_warnings = [
    "myst.header",
]

html_theme = "sphinx_rtd_theme"

root_doc = "index"
