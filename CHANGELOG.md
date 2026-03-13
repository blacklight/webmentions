# Changelog

## 0.1.21

### Fixed
- **parser:** Improved author parsing for string values: treat plain strings as
  the author **name**, and only treat strings starting with `http://` or `ht

tps://` as the author **URL**. Added tests to cover both cases.
## 0.1.20

### Added
- **docs**: Added badges and TOC to the README

## 0.1.19

### Changed
- Minor style tweaks

## 0.1.18

### Changed
- Removed `max-height` from expanded interactions

## 0.1.17

### Changed
- Webmention renderer now collapses overly long interaction content/excerpts
- (>1000 chars) with a “show more / show less” toggle, including new CSS for
- the collapsible behavior.

## 0.1.16

### Changed
- Prefer `html` over `value` when parsing mf2 `content` for webmentions.

### Fixed
- - Strip trailing URLs from mention titles derived from mf2 `name`, `og:title`, `twitter:title`, and `<title>`.
- - Sanitize rendered mention `content`/`excerpt` via an allowlist-based HTML sanitizer (including safe `href` scheme checks) to prevent unsafe markup in templates.

## 0.1.15

### Fixed

- Sort mentions by creation timestamp (descending) on the templates

## 0.1.14

### Added

- Summary header in the Webmentions container template showing counters for
  replies (💬), reposts (🔁), likes (⭐), and mentions (📣).
  `render_webmentions()` now computes counts from `Webmention.mention_type` and
  passes them to the template.

## 0.1.13

### Fixed

- Fixed CSS selectors for child classes not matching after the `wm-` prefix
  rename (e.g. `.wm-mention .mention-author-photo` → `.wm-mention .wm-mention-author-photo`).
  This caused all Webmention styling (author photos, names, layout) to break.

## 0.1.12

### Changed

- Namespaced all CSS classes in Webmention templates with a `wm-` prefix
  (`.mention` → `.wm-mention`, `.mentions` → `.wm-mentions`, etc.) to avoid
  collisions with external `.mention` classes (e.g. Mastodon's ActivityPub HTML).

## 0.1.11

### Added

- Added Tornado web framework adapter with async handlers using `run_in_executor`
  to avoid blocking the IOLoop. Includes `bind_webmentions()` with the same API
  as Flask/FastAPI adapters, `make_webmention_link_header_handler()` for Link
  header injection, unit and E2E tests, API docs, example server, and `[tornado]`
  install extra. Closes [#5](https://git.platypush.tech/blacklight/webmentions/issues/5).

### Fixed

- Don't display the Webmentions container `<div>` when there are no mentions to
  render.

### Changed

- Replaced usage of `assert` statements with proper validation in `Webmention.build`
  and renderer code (Codacy guidelines).

## 0.1.10

### Added

- `WebmentionsHandler.render_webmentions` now returns a single `Markup` object
  rather than a list of `Markup` objects, which makes rendering simpler and more
  consistent.

- Added general `<style>` tag for `webmentions/templates/webmentions.html`,
  customizable through CSS variables.

## 0.1.9

### Added

- Added `WebmentionsHandler.render_webmentions` as a helper method to render
  Webmentions into HTML.

### Changed

- Default `throttle_seconds` value for `FileSystemMonitor` has been increased
  from 2 to 10 seconds (to prevent flooding of events when a file is updated
  a few times in a short time).

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
