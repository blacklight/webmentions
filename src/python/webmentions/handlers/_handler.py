import logging
from typing import Any, Callable

from ..storage import WebmentionsStorage
from .._model import (
    ContentTextFormat,
    Webmention,
    WebmentionDirection,
    WebmentionStatus,
)
from ._constants import DEFAULT_HTTP_TIMEOUT, DEFAULT_USER_AGENT
from ._incoming import IncomingWebmentionsProcessor
from ._outgoing import OutgoingWebmentionsProcessor

logger = logging.getLogger(__name__)


class WebmentionsHandler:
    """
    Webmentions handler.

    :param storage: The Webmentions storage backend
    :param base_url: The base URL of the server, used to validate target URLs
    :param http_timeout: The HTTP timeout for fetching source URLs
    :param user_agent: The User-Agent header to use when fetching source URLs
    :param on_mention_processed: A callback to call when a Webmention is processed
    :param on_mention_deleted: A callback to call when a Webmention is deleted
    :param initial_mention_status: The initial status of Webmentions (see
        :class:`WebmentionStatus`). If not specified, defaults to
        :attr:`WebmentionStatus.CONFIRMED`. If you set this to
        :attr:`WebmentionStatus.PENDING` then you will need to manually mark
        mentions as confirmed on your storage, or create your on
        ``on_mention_processed`` that performs custom filtering or moderation
        and calls ``handler.storage.store_webmention(webmention)`` with the right
        status after processing.
    """

    def __init__(
        self,
        storage: WebmentionsStorage,
        *,
        base_url: str | None = None,
        http_timeout: float = DEFAULT_HTTP_TIMEOUT,
        user_agent: str = DEFAULT_USER_AGENT,
        on_mention_processed: Callable[[Webmention], None] | None = None,
        on_mention_deleted: Callable[[Webmention], None] | None = None,
        initial_mention_status: WebmentionStatus = WebmentionStatus.CONFIRMED,
        **kwargs,
    ):
        self.storage = storage
        self.incoming = IncomingWebmentionsProcessor(
            storage=storage,
            base_url=base_url,
            http_timeout=http_timeout,
            user_agent=user_agent,
            on_mention_processed=on_mention_processed,
            on_mention_deleted=on_mention_deleted,
            init_mention_status=initial_mention_status,
            **kwargs,
        )
        self.outgoing = OutgoingWebmentionsProcessor(
            storage=storage,
            user_agent=user_agent,
            http_timeout=http_timeout,
            on_mention_processed=on_mention_processed,
            on_mention_deleted=on_mention_deleted,
            **kwargs,
        )

    def process_incoming_webmention(
        self, source_url: str | None, target_url: str | None
    ) -> Any:
        """
        Process an incoming Webmention.

        :param source_url: The source URL of the Webmention
        :param target_url: The target URL of the Webmention
        """
        return self.incoming.process_incoming_webmention(source_url, target_url)

    def process_outgoing_webmentions(
        self,
        source_url: str,
        *,
        text: str | None = None,
        text_format: ContentTextFormat | None = None,
    ) -> Any:
        """
        Process an outgoing Webmention.

        :param source_url: The source URL of the Webmention. Ignored if text is
            provided.
        :param text: The text of the Webmention. If not provided, the source URL
            will be fetched.
        :param text_format: The text format of the Webmention. If not provided,
            it will be inferred from the source URL or text.
        """
        return self.outgoing.process_outgoing_webmentions(
            source_url, text=text, text_format=text_format
        )

    def retrieve_stored_webmentions(
        self, resource: str, direction: WebmentionDirection
    ) -> list[Webmention]:
        """
        Retrieve stored Webmentions for a given URL.

        :param resource: The resource URL
        :param direction: The direction of the Webmentions (inbound or outbound)
        :return: A list of Webmentions
        """
        return self.storage.retrieve_webmentions(resource, direction=direction)
