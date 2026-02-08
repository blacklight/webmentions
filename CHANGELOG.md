# Changelog

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
- End-to-end integration tests for `DbWebmentionStorage`
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
