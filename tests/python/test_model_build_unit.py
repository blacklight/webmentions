from __future__ import annotations

from datetime import datetime, timezone

import pytest

from webmentions._model import Webmention, WebmentionDirection


def test_build_with_missing_source():
    with pytest.raises(ValueError):
        Webmention.build(
            {
                "target": "https://example.com/t",
            },
            direction=WebmentionDirection.IN,
        )


def test_build_with_missing_target():
    with pytest.raises(ValueError):
        Webmention.build(
            {
                "source": "https://example.com/s",
            },
            direction=WebmentionDirection.IN,
        )


def test_build_parses_datetime_iso_with_tzinfo():
    m = Webmention.build(
        {
            "source": "https://example.com/s",
            "target": "https://example.com/t",
            "published": "2026-02-07T01:02:03+00:00",
        },
        direction=WebmentionDirection.IN,
    )

    assert m.published is not None
    assert m.published.isoformat() == "2026-02-07T01:02:03+00:00"


def test_build_parses_datetime_iso_without_tzinfo_assumes_utc():
    m = Webmention.build(
        {
            "source": "https://example.com/s",
            "target": "https://example.com/t",
            "published": "2026-02-07T01:02:03",
        },
        direction=WebmentionDirection.IN,
    )

    assert m.published is not None
    assert m.published.tzinfo == timezone.utc
    assert m.published.isoformat() == "2026-02-07T01:02:03+00:00"


def test_build_datetime_passthrough_preserves_tzinfo():
    published = datetime(2026, 2, 7, 1, 2, 3, tzinfo=timezone.utc)
    m = Webmention.build(
        {
            "source": "https://example.com/s",
            "target": "https://example.com/t",
            "published": published,
        },
        direction=WebmentionDirection.IN,
    )

    assert m.published is published


@pytest.mark.parametrize("value", [0, 0.0, 1, 1.5])
def test_build_parses_datetime_timestamp_numeric_assumes_utc(value):
    m = Webmention.build(
        {
            "source": "https://example.com/s",
            "target": "https://example.com/t",
            "published": value,
        },
        direction=WebmentionDirection.IN,
    )

    assert m.published is not None
    assert m.published.tzinfo == timezone.utc
    assert m.published.timestamp() == pytest.approx(float(value))


@pytest.mark.parametrize("field", ["published", "created_at", "updated_at"])
def test_build_blank_datetime_string_is_treated_as_none(field):
    m = Webmention.build(
        {
            "source": "https://example.com/s",
            "target": "https://example.com/t",
            field: "   ",
        },
        direction=WebmentionDirection.IN,
    )

    assert getattr(m, field) is None


@pytest.mark.parametrize("field", ["published", "created_at", "updated_at"])
def test_build_none_datetime_is_treated_as_none(field):
    m = Webmention.build(
        {
            "source": "https://example.com/s",
            "target": "https://example.com/t",
            field: None,
        },
        direction=WebmentionDirection.IN,
    )

    assert getattr(m, field) is None


@pytest.mark.parametrize("field", ["published", "created_at", "updated_at"])
def test_build_invalid_iso_datetime_raises_value_error(field):
    with pytest.raises(ValueError):
        Webmention.build(
            {
                "source": "https://example.com/s",
                "target": "https://example.com/t",
                field: "not-a-datetime",
            },
            direction=WebmentionDirection.IN,
        )


@pytest.mark.parametrize(
    ("raw", "expected"),
    [
        ("incoming", WebmentionDirection.IN),
        (" outgoing ", WebmentionDirection.OUT),
        ("INCOMING", WebmentionDirection.IN),
        ("OuTgOiNg", WebmentionDirection.OUT),
    ],
)
def test_webmention_direction_from_raw_parses_value(raw, expected):
    assert WebmentionDirection.from_raw(raw) is expected


@pytest.mark.parametrize(
    ("raw", "expected"),
    [
        ("IN", WebmentionDirection.IN),
        (" out ", WebmentionDirection.OUT),
        ("in", WebmentionDirection.IN),
        ("OUT", WebmentionDirection.OUT),
    ],
)
def test_webmention_direction_from_raw_parses_name(raw, expected):
    assert WebmentionDirection.from_raw(raw) is expected


def test_webmention_direction_from_raw_unknown_raises_value_error():
    with pytest.raises(ValueError, match=r"Unknown direction"):
        WebmentionDirection.from_raw("sideways")
