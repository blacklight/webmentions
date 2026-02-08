from dataclasses import asdict, dataclass, field
from datetime import datetime, timezone
from enum import Enum


class WebmentionDirection(str, Enum):
    """
    Enum representing the direction of a Webmention
    (incoming or outgoing).
    """

    IN = "incoming"
    OUT = "outgoing"

    @classmethod
    def from_raw(cls, raw: str) -> "WebmentionDirection":
        try:
            return cls(raw.strip().lower())
        except ValueError:
            value = getattr(cls, raw.strip().upper(), None)
            if not value:
                raise ValueError(f"Unknown direction: {raw}")
            return value


class WebmentionStatus(str, Enum):
    """
    Enum representing the status of a Webmention
    (pending, confirmed, or deleted).
    """

    PENDING = "pending"
    CONFIRMED = "confirmed"
    DELETED = "deleted"


class ContentTextFormat(str, Enum):
    """
    Supported content text formats.
    """

    HTML = "html"
    MARKDOWN = "markdown"
    TEXT = "text"


class WebmentionType(str, Enum):
    """
    Enum representing the type of Webmention.

    Note that this list is not exhaustive, and the
    Webmention recommendation itself does not provide
    any static list.

    This is however a lis of commonly supported types
    in Microformats.
    """

    UNKNOWN = "unknown"
    MENTION = "mention"
    REPLY = "reply"
    LIKE = "like"
    REPOST = "repost"
    BOOKMARK = "bookmark"
    RSVP = "rsvp"
    FOLLOW = "follow"

    @classmethod
    def from_raw(cls, raw: str | None) -> "WebmentionType":
        if not raw:
            return cls.UNKNOWN

        normalized = raw.strip().lower()
        aliases = {
            "in-reply-to": cls.REPLY,
            "reply": cls.REPLY,
            "like-of": cls.LIKE,
            "like": cls.LIKE,
            "repost-of": cls.REPOST,
            "repost": cls.REPOST,
            "bookmark-of": cls.BOOKMARK,
            "bookmark": cls.BOOKMARK,
            "rsvp": cls.RSVP,
            "follow-of": cls.FOLLOW,
            "follow": cls.FOLLOW,
            "mention": cls.MENTION,
        }

        return aliases.get(normalized, cls.UNKNOWN)


@dataclass
class Webmention:
    """
    Data class representing a Webmention.
    """

    source: str
    target: str
    direction: WebmentionDirection
    title: str | None = None
    excerpt: str | None = None
    content: str | None = None
    author_name: str | None = None
    author_url: str | None = None
    author_photo: str | None = None
    published: datetime | None = None
    status: WebmentionStatus = WebmentionStatus.CONFIRMED
    mention_type: WebmentionType = WebmentionType.UNKNOWN
    mention_type_raw: str | None = None
    metadata: dict = field(default_factory=dict)
    created_at: datetime | None = None
    updated_at: datetime | None = None

    def to_dict(self) -> dict:
        """
        :return: A dictionary representation of the Webmention
        """

        def _normalize(value):
            if value is None:
                return None
            if isinstance(value, datetime):
                return value.isoformat()
            if isinstance(value, Enum):
                return value.value
            if isinstance(value, list):
                return [_normalize(v) for v in value]
            if isinstance(value, tuple):
                return [_normalize(v) for v in value]
            if isinstance(value, dict):
                return {k: _normalize(v) for k, v in value.items()}
            return value

        return _normalize(asdict(self))

    def __hash__(self):
        """
        :return: A hash value based on the source, target, and direction.
        """
        return hash((self.source, self.target, self.direction))

    @classmethod
    def build(
        cls, data: dict, direction: WebmentionDirection = WebmentionDirection.IN
    ) -> "Webmention":
        assert data.get("source"), "source is required"
        assert data.get("target"), "target is required"
        mention_type: WebmentionType = (
            data.get("mention_type") or WebmentionType.MENTION
        )

        if isinstance(mention_type, str):
            mention_type = WebmentionType.from_raw(mention_type)

        def _parse_dt(value: object) -> datetime | None:
            dt = None
            if value is None:
                return None
            if isinstance(value, datetime):
                return value

            if isinstance(value, str) and value.strip():
                dt = datetime.fromisoformat(value)
            if isinstance(value, (int, float)):
                dt = datetime.fromtimestamp(value, tz=timezone.utc)

            if dt and dt.tzinfo is None:
                dt = dt.replace(tzinfo=timezone.utc)

            return dt

        published = _parse_dt(data.get("published"))
        created_at = _parse_dt(data.get("created_at"))
        updated_at = _parse_dt(data.get("updated_at"))

        return cls(
            **{
                **{k: v for k, v in data.items() if k in cls.__dataclass_fields__},
                "direction": direction,
                "status": data.get("status") or WebmentionStatus.CONFIRMED,
                "mention_type": mention_type,
                "mention_type_raw": mention_type.value,
                "published": published,
                "created_at": created_at,
                "updated_at": updated_at,
            }
        )
