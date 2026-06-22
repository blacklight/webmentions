import logging
from pathlib import Path
from typing import Any, Callable, Collection

from jinja2 import Template
from markupsafe import Markup

from ..render import WebmentionsRenderer
from ..storage import WebmentionsStorage
from .._model import (
    ContentTextFormat,
    Webmention,
    WebmentionDirection,
    WebmentionStatus,
)
from ._constants import (
    DEFAULT_HTTP_TIMEOUT,
    DEFAULT_MAX_DISCOVERY_RESPONSE_BYTES,
    DEFAULT_USER_AGENT,
)
from ._incoming import IncomingWebmentionsProcessor
from ._outgoing import OutgoingWebmentionsProcessor

logger = logging.getLogger(__name__)


class WebmentionsHandler:
    """
    Webmentions handler.

    :param storage: The Webmentions storage backend
    :param base_url: The base URL of the server, used to validate target URLs
    :param base_urls: A list of base URLs, used to validate target URLs when
        the server responds on multiple domains (e.g., split-domain setups).
        If both ``base_url`` and ``base_urls`` are provided, they are merged.
    :param http_timeout: The HTTP timeout for fetching source URLs
    :param max_discovery_response_bytes: Maximum response body size to read when
        discovering outgoing Webmention endpoints from target HTML.
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
        base_urls: list[str] | None = None,
        http_timeout: float = DEFAULT_HTTP_TIMEOUT,
        max_discovery_response_bytes: int = DEFAULT_MAX_DISCOVERY_RESPONSE_BYTES,
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
            base_urls=base_urls,
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
            max_discovery_response_bytes=max_discovery_response_bytes,
            on_mention_processed=on_mention_processed,
            on_mention_deleted=on_mention_deleted,
            **kwargs,
        )
        self.renderer = WebmentionsRenderer()

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

    def render_webmention(
        self,
        webmention: Webmention,
        template: str | Path | Template | None = None,
    ) -> Markup:
        """
        Render a Webmention as a ``Markup`` object that can be imported as safe
        HTML snippets in your Jinja2 templates.

        :param webmention: The Webmention to render
        :param template: The template to use. It can be a path, a Jinja2
            Template object or a template string. If not provided, the default
            template ``webmentions/templates/webmention.html`` will be used.
        :return: The rendered templates
        """
        return self.renderer.render_webmention(webmention, template=template)

    def render_webmentions(
        self,
        webmentions: Collection[Webmention],
        template: str | Path | Template | None = None,
    ) -> Markup:
        """
        Render a list of Webmentions as a ``Markup`` object that can be imported
        as safe HTML snippets in your Jinja2 templates.

        :param webmentions: The Webmentions to render
        :param template: The template to use. It can be a path, a Jinja2
            Template object or a template string. If not provided, the default
            template ``webmentions/templates/webmention.html`` will be used.
        :return: The rendered templates
        """
        return self.renderer.render_webmentions(webmentions, template=template)
