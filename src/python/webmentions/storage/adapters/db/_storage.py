from dataclasses import asdict
from datetime import datetime, timezone
from typing import Callable

import sqlalchemy as sa
from sqlalchemy.exc import IntegrityError
from sqlalchemy.orm import Session

from ..._base import (
    Webmention,
    WebmentionDirection,
    WebmentionsStorage,
)
from ._model import DbWebmention


class DbWebmentionsStorage(WebmentionsStorage):
    """
    Implements a simple database storage for Webmentions.

    :param engine: SQLAlchemy engine
    :param model: SQLAlchemy model. It must inherit from DbWebmention
        and be mapped to the database.
    :param session_factory: SQLAlchemy session factory. You can create
        a session factory through e.g.

        .. code-block:: python

            from sqlalchemy.orm import sessionmaker

            session = sessionmaker(bind=engine)

    """

    def __init__(
        self,
        engine: sa.Engine,
        model: type[DbWebmention],
        session_factory: Callable[[], Session],
        *_,
        **__,
    ):
        self.engine = engine
        self.session_factory = session_factory
        self.model = model

    def store_webmention(self, mention: Webmention):
        session = self.session_factory()
        try:
            try:
                session.add(self.model.from_webmention(mention))
                session.commit()
                return
            except IntegrityError:
                session.rollback()

            existing = (
                session.query(self.model)
                .filter(
                    sa.and_(
                        self.model.source == mention.source,
                        self.model.target == mention.target,
                        self.model.direction == mention.direction,
                    )
                )
                .one_or_none()
            )

            if existing is None:
                session.add(self.model.from_webmention(mention))
                session.commit()
                return

            columns = self.model.columns()
            data = {k: v for k, v in asdict(mention).items() if k in columns}
            for key, value in data.items():
                if key in {"id", "created_at", "updated_at"}:
                    continue
                setattr(existing, key, value)

            existing.updated_at = datetime.now(timezone.utc)  # type: ignore
            session.commit()
        finally:
            session.close()

    def delete_webmention(
        self,
        source: str,
        target: str,
        direction: WebmentionDirection,
    ):
        session = self.session_factory()
        try:
            session.query(self.model).filter(
                sa.and_(
                    self.model.source == source,
                    self.model.target == target,
                    self.model.direction == direction,
                )
            ).delete(synchronize_session=False)
            session.commit()
        finally:
            session.close()

    def retrieve_webmentions(
        self,
        resource: str,
        direction: WebmentionDirection,
    ) -> list[Webmention]:
        session = self.session_factory()
        try:
            return [
                wm.to_webmention()
                for wm in session.query(self.model).filter(
                    sa.and_(
                        self.model.target == resource,
                        self.model.direction == direction,
                    )
                )
            ]
        finally:
            session.close()
