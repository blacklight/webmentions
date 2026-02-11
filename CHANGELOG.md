# Changelog

## 0.1.8

### Added

- Added example Jinja template for rendering Webmentions and an example
  semantic-compliant microformats2 HTML page.

## 0.1.7

### Fixed

- Fix behaviour of `DbWebmentionsStorage.retrieve_webmentions` - it should only
  include records having `status=WebmentionStatus.CONFIRMED`.

## 0.1.6

### Changed
- **File monitor:** A more consistent start/stop API, plus a context manager.
- **Storage adapters:** Don't expose `FileSystemWatcher` in `webmentions.storage.adapters.file`.

## 0.1.5

### Changed
- **Parser:** Enhanced Microformats2 support.

### Documentation / CI
- **CI:** Added black and flake8 pre-commit hooks.
- **README:** Added a _Microformats support_ section.
- **Docs:** Added a link to the repo (and its mirror) in the docs index.

### Tests
- **Parser:** Extended tests to cover all supported Microformats features.

## 0.1.4

### Added
- **API:** Added `GET /webmentions` endpoint to `bind_webmentions`.
- **Handler:** Ignore self-references (webmentions where `src == dest`).
- **Documentation:** Added auto-generated Sphinx documentation under `docs/`.

### Changed
- **API:** Changed default endpoint from `/webmention` to `/webmentions`.
- **Handler:** Removed `exclude_netlocs` parameters.

### Documentation / CI
- **README:** Documented the `GET /webmentions` endpoint/binding.
- **Docs:** Added link to hosted docs at `docs.webmentions.work`.
- **CI:** Added workflow to rebuild and publish docs on new version.

### Tests
- Added tests for the default `GET` endpoint.
- Added tests for `WebmentionDirection.from_raw`.

## 0.1.3

### Added

- Webmention handler callbacks for mentions sent/received/deleted
- `initial_mention_status` option for incoming mentions

### Fixed

- Wrap Webmention callbacks in `try/except` to prevent callback errors from crashing the app

### Testing

- Expanded unit test coverage across handler, outgoing processor, adapters, parser, file watcher, and model
- Improved E2E test performance

### CI

- Added GitHub Actions coverage reporting

### Documentation

- Project URL metadata and README updates (badges)

## 0.1.2

### Added

- File-system monitoring storage adapter for serving static Web server files
- `Link` header support in FastAPI and Flask server adapters

### Fixed

- Ensure timestamps are updated when a Webmention is marked as sent
- Fix database retrieval by selecting the correct column dynamically

### Testing

- End-to-end tests covering Web server integration, file monitoring, and DB storage
- Restore compatibility with Python <= 3.10 in the test suite

### Documentation

- README updates covering `bind_webmentions` and general documentation completion

## 0.1.1

### Added

- SQLAlchemy-backed storage layer via generic `DbWebmentionsStorage` plus helpers
- End-to-end integration tests for `DbWebmentionsStorage`
- Server adapters for FastAPI and Flask
- Generic key-value metadata field on the `Webmention` model

### Documentation

- Documentation for `WebmentionsHandler` parameters
- README improvements, including banner image and usage examples for Flask/FastAPI
- Example applications for SQLAlchemy storage, FastAPI, and Flask

## 0.1.0

First version. Supports:

- Receiving Webmention
- Sending Webmention
