import sqlalchemy as sa
from sqlalchemy.orm import declarative_base
from sqlalchemy.orm import sessionmaker

from ._model import DbWebmention
from ._storage import DbWebmentionsStorage


def init_db_storage(
    engine: str | sa.Engine,
    *args,
    table_name: str = "webmentions",
    **kwargs,
) -> DbWebmentionsStorage:
    """
    A helper function that initializes a database storage for Webmentions.

    Use this if you want to use a dedicated SQLAlchemy engine for your
    Webmentions storage.

    Otherwise, extend ``DbWebmention`` with your table name, register it
    to your engine, and initialize a ``DbWebmentionsStorage``.

    :param engine: SQLAlchemy engine, as a string (creates a new engine)
        or an existing engine
    :param table_name: Name of the table to use to store Webmentions
    :param args: Positional arguments to pass to ``sa.create_engine``
    :param kwargs: Keyword arguments to pass to ``sa.create_engine``
    """

    Base = declarative_base()

    class DefaultDbWebmention(Base, DbWebmention):
        """
        Dynamically generated base class.
        """

        __tablename__ = table_name

    if isinstance(engine, str):
        engine = sa.create_engine(engine, *args, **kwargs)

    Base.metadata.create_all(engine)
    return DbWebmentionsStorage(
        engine=engine,
        session_factory=sessionmaker(bind=engine),
        model=DefaultDbWebmention,
    )
