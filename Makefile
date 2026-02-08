PYTHON ?= python

all:
	$(PYTHON) -m build

.PHONY: docs
docs:
	mkdir -p docs/api
	$(PYTHON) -m sphinx.ext.apidoc -o docs/api -f -e src/python/webmentions
	$(PYTHON) -m sphinx -b html docs docs/_build/html
