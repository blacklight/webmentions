import logging
import time
from pathlib import Path
from threading import RLock
from typing import Callable

from ....handlers import WebmentionsHandler
from ._model import ContentChange, ContentChangeType
from ._watcher import FileSystemWatcher

logger = logging.getLogger(__name__)


class FileSystemMonitor:
    """
    Encapsulates :class:`.FilesystemWatcher` to watch a directory for
    changes to files and maps file system events to outgoing Webmention
    requests.

    :param handler: The Webmentions handler to use to dispatch
        outgoing Webmentions.
    :param root_dir: The root directory to watch.
    :param file_to_url_mapper: A function that maps file paths to URLs
        to be used in outgoing Webmentions.
    :param extensions: A tuple of file extensions to watch. Default: all
        text, HTML, and Markdown files.
    :param throttle_seconds: The minimum number of seconds between
        processing changes, to prevent too many consecutive calls when a
        file is written multiple times. Default: 2.
    """

    def __init__(
        self,
        handler: WebmentionsHandler,
        root_dir: str,
        file_to_url_mapper: Callable[[str], str],
        *,
        extensions: tuple[str, ...] = (".md", ".markdown", ".txt", ".html", ".htm"),
        throttle_seconds: float = 2.0,
    ):
        self._webmentions_handler = handler
        self._source_url_from_content_path = file_to_url_mapper
        self._watcher: FileSystemWatcher | None = None
        self._root_dir = Path(root_dir).expanduser().resolve()
        self.extensions = tuple(e.lower() for e in extensions)
        self.throttle_seconds = throttle_seconds
        self._watcher_lock = RLock()

    def _on_filesystem_change(self, change: ContentChange):
        if not self._webmentions_handler:
            return

        source_url = self._source_url_from_content_path(change.path)
        t_start = time.monotonic()

        if change.change_type == ContentChangeType.DELETED:
            logger.info(
                "Content deleted, processing outgoing Webmentions: %s",
                source_url,
            )
            self._webmentions_handler.process_outgoing_webmentions(
                source_url,
                text="",
                text_format=change.text_format,
            )
        else:
            logger.info(
                "Content changed, processing outgoing Webmentions: %s",
                source_url,
            )
            self._webmentions_handler.process_outgoing_webmentions(
                source_url,
                text=change.text,
                text_format=change.text_format,
            )

        logger.info(
            "Processed outgoing Webmentions for %s in %.2f seconds",
            source_url,
            time.monotonic() - t_start,
        )

    def start(self) -> None:
        """
        Starts the filesystem watcher.
        """
        with self._watcher_lock:
            if self._watcher is not None:
                return

            self._watcher = FileSystemWatcher(
                root_dir=str(self._root_dir),
                on_change=self._on_filesystem_change,
                extensions=self.extensions,
                throttle_seconds=self.throttle_seconds,
            )

            self._watcher.start()

    def stop(self) -> None:
        """
        Stops the filesystem watcher.
        """
        with self._watcher_lock:
            if self._watcher is None:
                return

            self._watcher.stop()
            self._watcher = None

    def __enter__(self):
        """
        Starts the filesystem watcher.
        """
        self.start()
        return self

    def __exit__(self, *_, **__):
        """
        Stops the filesystem watcher.
        """
        self.stop()
