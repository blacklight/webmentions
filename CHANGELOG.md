# Changelog

## Unreleased

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
