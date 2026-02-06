from dataclasses import asdict
from datetime import datetime, timezone

import sqlalchemy as sa

from ...._model import (
    Webmention,
    WebmentionDirection,
    WebmentionType,
    WebmentionStatus,
)


class DbWebmention:
    """
    SQLAlchemy base model for Webmentions.

    This model should be inherited by an actual SQLAlchemy model.
    """

    id = sa.Column(sa.Integer, primary_key=True, autoincrement=True)
    source = sa.Column(sa.String, nullable=False)
    target = sa.Column(sa.String, nullable=False)
    direction = sa.Column(
        sa.Enum(WebmentionDirection, name="webmention_direction"), nullable=False
    )
    title = sa.Column(sa.String)
    excerpt = sa.Column(sa.String)
    content = sa.Column(sa.String)
    author_name = sa.Column(sa.String)
    author_url = sa.Column(sa.String)
    author_photo = sa.Column(sa.String)
    published = sa.Column(sa.DateTime, nullable=False)
    status = sa.Column(
        sa.Enum(WebmentionStatus, name="webmention_status"),
        nullable=False,
        default=WebmentionStatus.PENDING,
    )
    mention_type = sa.Column(
        sa.Enum(WebmentionType, name="webmention_type"), nullable=False
    )
    meta = sa.Column(sa.JSON, nullable=False, default={})
    created_at = sa.Column(sa.DateTime, nullable=False)
    updated_at = sa.Column(sa.DateTime, nullable=False)

    # Unique constraint on <source, target, direction>
    __table_args__ = (
        sa.UniqueConstraint(
            "source", "target", "direction", name="uix_source_target_direction"
        ),
    )

    def __init__(self, *_, **kwargs):
        for key, value in kwargs.items():
            setattr(self, key, value)

    @classmethod
    def columns(cls) -> set[str]:
        return {c.name for c in cls.__table__.columns}  # type: ignore

    @classmethod
    def from_webmention(cls, webmention: Webmention) -> "DbWebmention":
        created_at = webmention.created_at
        if created_at is None:
            created_at = webmention.published
        if created_at is None:
            created_at = datetime.now(timezone.utc)

        columns = cls.columns()
        data = {k: v for k, v in asdict(webmention).items() if k in columns}
        return cls(
            **{
                **data,
                "meta": webmention.metadata or {},
                "created_at": created_at,
                "updated_at": datetime.now(timezone.utc),
            }
        )

    # noinspection PyTypeChecker
    def to_webmention(self) -> Webmention:
        return Webmention(
            source=self.source,  # type: ignore
            target=self.target,  # type: ignore
            direction=WebmentionDirection(self.direction),  # type: ignore
            title=self.title,  # type: ignore
            excerpt=self.excerpt,  # type: ignore
            content=self.content,  # type: ignore
            author_name=self.author_name,  # type: ignore
            author_url=self.author_url,  # type: ignore
            author_photo=self.author_photo,  # type: ignore
            published=self.published,  # type: ignore
            mention_type=(
                WebmentionType.from_raw(self.mention_type)  # type: ignore
                if self.mention_type  # type: ignore
                else WebmentionType.MENTION
            ),
            metadata=dict(self.meta),  # type: ignore
            created_at=self.created_at,  # type: ignore
            updated_at=self.updated_at,  # type: ignore
        )
