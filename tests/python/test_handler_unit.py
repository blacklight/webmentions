from unittest.mock import Mock

import pytest

from webmentions import (
    Webmention,
    WebmentionDirection,
    WebmentionsHandler,
    WebmentionStatus,
)


def test_process_incoming_webmention_sets_initial_mention_status(monkeypatch):
    storage = Mock()
    handler = WebmentionsHandler(storage=storage, initial_mention_status=WebmentionStatus.PENDING)

    parsed = Webmention(
        source="https://example.com/source",
        target="https://example.com/target",
        direction=WebmentionDirection.IN,
        status=WebmentionStatus.CONFIRMED,
    )

    monkeypatch.setattr(handler.incoming.parser, "parse", lambda *_: parsed)

    handler.process_incoming_webmention(parsed.source, parsed.target)

    assert storage.store_webmention.call_count == 1
    stored_mention = storage.store_webmention.call_args.args[0]
    assert stored_mention.status == WebmentionStatus.PENDING


def test_process_incoming_webmention_default_initial_status_is_confirmed(monkeypatch):
    storage = Mock()
    handler = WebmentionsHandler(storage=storage)

    parsed = Webmention(
        source="https://example.com/source",
        target="https://example.com/target",
        direction=WebmentionDirection.IN,
        status=WebmentionStatus.PENDING,
    )

    monkeypatch.setattr(handler.incoming.parser, "parse", lambda *_: parsed)

    handler.process_incoming_webmention(parsed.source, parsed.target)

    stored_mention = storage.store_webmention.call_args.args[0]
    assert stored_mention.status == WebmentionStatus.CONFIRMED


@pytest.mark.parametrize(
    ("source", "author_name", "expected_status"),
    [
        ("https://example.com/spam", "Ok Author", WebmentionStatus.DELETED),
        ("https://example.com/ham", "Spam Author", WebmentionStatus.DELETED),
        ("https://example.com/ham", "Ok Author", WebmentionStatus.CONFIRMED),
    ],
)
def test_custom_on_mention_processed_confirms_only_non_spam_incoming(
    monkeypatch,
    source,
    author_name,
    expected_status,
):
    stored_statuses: list[WebmentionStatus] = []

    def _store_webmention(m: Webmention):
        stored_statuses.append(m.status)

    storage = Mock()
    storage.store_webmention.side_effect = _store_webmention

    handler = None

    def on_mention_processed(mention: Webmention):
        # Don't do anything for outgoing mentions
        if mention.direction == WebmentionDirection.OUT:
            return

        # Delete Webmentions coming from notorious spam domains or authors
        if mention.direction == WebmentionDirection.IN:
            if (
                mention.source in ["https://example.com/spam"]
                or mention.author_name in ["Spam Author"]
            ):
                mention.status = WebmentionStatus.DELETED
            # Otherwise, confirm the Webmention
            else:
                mention.status = WebmentionStatus.CONFIRMED

        # Save the modified mention
        handler.storage.store_webmention(mention)

    handler = WebmentionsHandler(
        storage=storage,
        initial_mention_status=WebmentionStatus.PENDING,
        on_mention_processed=on_mention_processed,
    )

    parsed = Webmention(
        source=source,
        target="https://example.com/target",
        direction=WebmentionDirection.IN,
        author_name=author_name,
    )

    monkeypatch.setattr(handler.incoming.parser, "parse", lambda *_: parsed)

    handler.process_incoming_webmention(parsed.source, parsed.target)

    assert storage.store_webmention.call_count == 2
    assert stored_statuses == [WebmentionStatus.PENDING, expected_status]


def test_custom_on_mention_processed_ignores_outgoing_mentions():
    storage = Mock()

    handler = None

    def on_mention_processed(mention: Webmention):
        # Don't do anything for outgoing mentions
        if mention.direction == WebmentionDirection.OUT:
            return

        if mention.direction == WebmentionDirection.IN:
            mention.status = WebmentionStatus.CONFIRMED

        handler.storage.store_webmention(mention)

    handler = WebmentionsHandler(storage=storage, on_mention_processed=on_mention_processed)

    outgoing = Webmention(
        source="https://example.com/source",
        target="https://example.com/target",
        direction=WebmentionDirection.OUT,
    )

    handler.outgoing._on_mention_processed(outgoing)

    storage.store_webmention.assert_not_called()
