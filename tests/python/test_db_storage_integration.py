from datetime import datetime, timezone

import pytest

from webmentions import (
    Webmention,
    WebmentionDirection,
    WebmentionsStorage,
    WebmentionType,
)
from webmentions.storage.adapters.db import init_db_storage


@pytest.fixture
def db_storage(tmp_path) -> WebmentionsStorage:
    db_path = tmp_path / "webmentions.sqlite"
    return init_db_storage(engine=f"sqlite:///{db_path}")


@pytest.mark.integration
def test_db_storage_roundtrip(db_storage):
    published = datetime.now(timezone.utc)
    webmention = Webmention(
        source="https://example.com/source",
        target="https://example.com/target",
        direction=WebmentionDirection.IN,
        mention_type=WebmentionType.MENTION,
        author_name="John Doe",
        author_url="https://example.com/johndoe",
        author_photo="https://example.com/johndoe/photo.jpg",
        published=published,
    )

    db_storage.store_webmention(webmention)
    results = db_storage.retrieve_webmentions(
        resource="https://example.com/target",
        direction=WebmentionDirection.IN,
    )

    assert len(results) == 1

    stored = results[0]
    assert stored.source == webmention.source
    assert stored.target == webmention.target
    assert stored.direction == webmention.direction
    assert stored.mention_type == webmention.mention_type
    assert stored.author_name == webmention.author_name
    assert stored.author_url == webmention.author_url
    assert stored.author_photo == webmention.author_photo

    assert stored.published is not None
    assert stored.published.replace(tzinfo=timezone.utc) == published

    assert stored.created_at is not None
    assert stored.updated_at is not None


@pytest.mark.integration
def test_db_storage_store_is_idempotent_on_key_and_updates_fields(db_storage):
    published = datetime.now(timezone.utc)
    base = Webmention(
        source="https://example.com/source",
        target="https://example.com/target",
        direction=WebmentionDirection.IN,
        mention_type=WebmentionType.MENTION,
        author_name="John Doe",
        author_url="https://example.com/johndoe",
        author_photo="https://example.com/johndoe/photo.jpg",
        published=published,
    )

    db_storage.store_webmention(base)
    results1 = db_storage.retrieve_webmentions(
        resource="https://example.com/target",
        direction=WebmentionDirection.IN,
    )
    assert len(results1) == 1
    created_at_1 = results1[0].created_at
    updated_at_1 = results1[0].updated_at
    assert created_at_1 is not None
    assert updated_at_1 is not None

    updated = Webmention(
        source=base.source,
        target=base.target,
        direction=base.direction,
        mention_type=base.mention_type,
        author_name="Jane Doe",
        author_url="https://example.com/janedoe",
        author_photo=base.author_photo,
        published=published,
        title="New title",
        excerpt="New excerpt",
        content="New content",
    )

    db_storage.store_webmention(updated)
    results2 = db_storage.retrieve_webmentions(
        resource="https://example.com/target",
        direction=WebmentionDirection.IN,
    )
    assert len(results2) == 1

    stored = results2[0]
    assert stored.source == base.source
    assert stored.target == base.target
    assert stored.direction == base.direction
    assert stored.mention_type == base.mention_type
    assert stored.author_name == "Jane Doe"
    assert stored.author_url == "https://example.com/janedoe"
    assert stored.title == "New title"
    assert stored.excerpt == "New excerpt"
    assert stored.content == "New content"

    assert stored.created_at is not None
    assert stored.updated_at is not None
    assert stored.created_at.replace(tzinfo=timezone.utc) == created_at_1.replace(
        tzinfo=timezone.utc
    )
    assert stored.updated_at.replace(tzinfo=timezone.utc) >= updated_at_1.replace(
        tzinfo=timezone.utc
    )


@pytest.mark.integration
def test_db_storage_delete(db_storage):
    published = datetime.now(timezone.utc)
    webmention = Webmention(
        source="https://example.com/source",
        target="https://example.com/target",
        direction=WebmentionDirection.IN,
        mention_type=WebmentionType.MENTION,
        published=published,
    )

    db_storage.store_webmention(webmention)
    assert (
        len(
            db_storage.retrieve_webmentions(
                resource=webmention.target,
                direction=webmention.direction,
            )
        )
        == 1
    )

    db_storage.delete_webmention(
        source=webmention.source,
        target=webmention.target,
        direction=webmention.direction,
    )

    results = db_storage.retrieve_webmentions(
        resource=webmention.target,
        direction=webmention.direction,
    )
    assert results == []

    db_storage.delete_webmention(
        source=webmention.source,
        target=webmention.target,
        direction=webmention.direction,
    )
