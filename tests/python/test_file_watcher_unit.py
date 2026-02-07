import os
from unittest.mock import Mock

import pytest

from webmentions import ContentTextFormat
from webmentions.storage.adapters.file._model import ContentChangeType
from webmentions.storage.adapters.file._watcher import FileSystemWatcher


class _FakeObserver:
    def __init__(self):
        self.daemon = False
        self.scheduled = []
        self.started = False
        self.stopped = False
        self.joined = []

    def schedule(self, handler, root_dir, recursive=False):
        self.scheduled.append((handler, root_dir, recursive))

    def start(self):
        self.started = True

    def stop(self):
        self.stopped = True

    def join(self, timeout=None):
        self.joined.append(timeout)


class _FakeThread:
    def __init__(self, *, target, name=None, daemon=None):
        self.target = target
        self.name = name
        self.daemon = daemon
        self.started = False

    def start(self):
        self.started = True


def test_start_noop_if_root_dir_missing(monkeypatch, tmp_path):
    on_change = Mock()
    watcher = FileSystemWatcher(str(tmp_path), on_change)

    monkeypatch.setattr("webmentions.storage.adapters.file._watcher.os.path.isdir", lambda _: False)
    watcher.start()

    assert watcher._watch_observer is None
    assert watcher._watch_thread is None


def test_start_wires_observer_and_starts_thread(monkeypatch, tmp_path):
    on_change = Mock()
    watcher = FileSystemWatcher(str(tmp_path), on_change)

    monkeypatch.setattr("webmentions.storage.adapters.file._watcher.os.path.isdir", lambda _: True)
    monkeypatch.setattr("webmentions.storage.adapters.file._watcher.Observer", _FakeObserver)
    monkeypatch.setattr("webmentions.storage.adapters.file._watcher.threading.Thread", _FakeThread)

    watcher.start()

    assert watcher._watch_observer is not None
    assert watcher._watch_observer.started is True
    assert watcher._watch_thread is not None
    assert watcher._watch_thread.started is True

    handler, root_dir, recursive = watcher._watch_observer.scheduled[0]
    assert root_dir == str(tmp_path)
    assert recursive is True

    class _Event:
        def __init__(self, src_path=None, dest_path=None, is_directory=False):
            self.src_path = src_path
            self.dest_path = dest_path
            self.is_directory = is_directory

    watcher._watch_queue.queue.clear()

    handler.on_created(_Event(src_path=str(tmp_path / "a.md")))
    handler.on_modified(_Event(src_path=str(tmp_path / "b.md")))
    handler.on_deleted(_Event(src_path=str(tmp_path / "c.md")))
    handler.on_moved(_Event(src_path=str(tmp_path / "d.md"), dest_path=str(tmp_path / "e.md")))

    q = list(watcher._watch_queue.queue)
    assert ("created", os.path.abspath(str(tmp_path / "a.md"))) in q
    assert ("modified", os.path.abspath(str(tmp_path / "b.md"))) in q
    assert ("deleted", os.path.abspath(str(tmp_path / "c.md"))) in q
    assert ("deleted", os.path.abspath(str(tmp_path / "d.md"))) in q
    assert ("created", os.path.abspath(str(tmp_path / "e.md"))) in q


def test_stop_is_idempotent_and_stops_observer(monkeypatch, tmp_path):
    on_change = Mock()
    watcher = FileSystemWatcher(str(tmp_path), on_change)

    monkeypatch.setattr("webmentions.storage.adapters.file._watcher.os.path.isdir", lambda _: True)
    monkeypatch.setattr("webmentions.storage.adapters.file._watcher.Observer", _FakeObserver)
    monkeypatch.setattr("webmentions.storage.adapters.file._watcher.threading.Thread", _FakeThread)

    watcher.start()
    observer = watcher._watch_observer

    watcher.stop()
    assert observer.stopped is True
    assert observer.joined == [5]
    assert watcher._watch_observer is None
    assert watcher._watch_thread is None

    watcher.stop()


def test_enqueue_fs_event_filters_empty_outside_root_and_extension(monkeypatch, tmp_path):
    on_change = Mock()
    watcher = FileSystemWatcher(str(tmp_path), on_change, extensions=(".md",))

    monkeypatch.setattr("webmentions.storage.adapters.file._watcher.os.path.abspath", lambda p: p)

    watcher._enqueue_fs_event("modified", "")
    assert watcher._watch_queue.qsize() == 0

    watcher._enqueue_fs_event("modified", "/outside/file.md")
    assert watcher._watch_queue.qsize() == 0

    watcher._enqueue_fs_event("modified", str(tmp_path / "file.txt"))
    assert watcher._watch_queue.qsize() == 0

    watcher._enqueue_fs_event("modified", str(tmp_path / "file.md"))
    assert watcher._watch_queue.get_nowait() == ("modified", str(tmp_path / "file.md"))


def test_flush_debounced_throttles_and_dispatches(monkeypatch, tmp_path):
    events = []

    def _on_change(change):
        events.append(change)

    watcher = FileSystemWatcher(str(tmp_path), _on_change, throttle_seconds=2.0)

    path = str(tmp_path / "a.md")
    watcher._pending_paths.add(path)
    watcher._last_event_at[path] = 10.0
    watcher._last_event_type[path] = "modified"

    watcher._last_processed_at = 11.5
    monkeypatch.setattr("webmentions.storage.adapters.file._watcher.time.monotonic", lambda: 12.0)
    watcher._flush_debounced()
    assert events == []

    monkeypatch.setattr("webmentions.storage.adapters.file._watcher.time.monotonic", lambda: 20.0)

    def _build_change(event_type, abs_path):
        assert event_type == "modified"
        assert abs_path == path
        return Mock()

    monkeypatch.setattr(watcher, "_build_change", _build_change)
    watcher._flush_debounced()

    assert len(events) == 1
    assert path not in watcher._pending_paths
    assert path not in watcher._last_event_at
    assert path not in watcher._last_event_type


def test_flush_debounced_swallows_on_change_exceptions(monkeypatch, tmp_path):
    def _on_change(_):
        raise RuntimeError("boom")

    watcher = FileSystemWatcher(str(tmp_path), _on_change, throttle_seconds=1.0)

    path = str(tmp_path / "a.md")
    watcher._pending_paths.add(path)
    watcher._last_event_at[path] = 0.0
    watcher._last_event_type[path] = "created"

    monkeypatch.setattr("webmentions.storage.adapters.file._watcher.time.monotonic", lambda: 10.0)
    monkeypatch.setattr(watcher, "_build_change", lambda *_: Mock())

    watcher._flush_debounced()


def test_build_change_deleted_for_deleted_event_or_missing_file(monkeypatch, tmp_path):
    on_change = Mock()
    watcher = FileSystemWatcher(str(tmp_path), on_change)

    path = str(tmp_path / "missing.md")

    monkeypatch.setattr("webmentions.storage.adapters.file._watcher.os.path.isfile", lambda _: False)

    change1 = watcher._build_change("deleted", path)
    assert change1 is not None
    assert change1.change_type == ContentChangeType.DELETED
    assert change1.text is None
    assert change1.text_format is None

    change2 = watcher._build_change("modified", path)
    assert change2 is not None
    assert change2.change_type == ContentChangeType.DELETED


@pytest.mark.parametrize(
    ("event_type", "expected"),
    [
        ("created", ContentChangeType.ADDED),
        ("modified", ContentChangeType.EDITED),
    ],
)
def test_build_change_reads_text_and_guesses_format(monkeypatch, tmp_path, event_type, expected):
    on_change = Mock()
    watcher = FileSystemWatcher(str(tmp_path), on_change)

    path = tmp_path / "a.md"
    path.write_text("hello", encoding="utf-8")

    monkeypatch.setattr("webmentions.storage.adapters.file._watcher.os.path.isfile", lambda p: str(p) == str(path))

    change = watcher._build_change(event_type, str(path))
    assert change is not None
    assert change.change_type == expected
    assert change.text == "hello"
    assert change.text_format == ContentTextFormat.MARKDOWN


def test_build_change_returns_none_if_format_unknown(monkeypatch, tmp_path):
    on_change = Mock()
    watcher = FileSystemWatcher(str(tmp_path), on_change)

    path = tmp_path / "a.unknown"
    path.write_text("hello", encoding="utf-8")

    monkeypatch.setattr("webmentions.storage.adapters.file._watcher.os.path.isfile", lambda p: str(p) == str(path))

    assert watcher._build_change("modified", str(path)) is None


def test_build_change_sets_text_none_on_read_error(monkeypatch, tmp_path):
    on_change = Mock()
    watcher = FileSystemWatcher(str(tmp_path), on_change)

    path = tmp_path / "a.md"
    path.write_text("hello", encoding="utf-8")

    monkeypatch.setattr("webmentions.storage.adapters.file._watcher.os.path.isfile", lambda p: str(p) == str(path))

    def _open(*_, **__):
        raise OSError("nope")

    monkeypatch.setattr("builtins.open", _open)

    change = watcher._build_change("modified", str(path))
    assert change is not None
    assert change.text is None
    assert change.text_format == ContentTextFormat.MARKDOWN


def test_guess_text_format():
    assert FileSystemWatcher._guess_text_format("/x/a.htm") == ContentTextFormat.HTML
    assert FileSystemWatcher._guess_text_format("/x/a.html") == ContentTextFormat.HTML
    assert FileSystemWatcher._guess_text_format("/x/a.md") == ContentTextFormat.MARKDOWN
    assert FileSystemWatcher._guess_text_format("/x/a.markdown") == ContentTextFormat.MARKDOWN
    assert FileSystemWatcher._guess_text_format("/x/a.txt") == ContentTextFormat.TEXT
    assert FileSystemWatcher._guess_text_format("/x/a.text") == ContentTextFormat.TEXT
    assert FileSystemWatcher._guess_text_format("/x/a.bin") is None
