![Webmentions](https://s3.fabiomanganiello.com/fabio/img/webmentions.png)

A general-purpose library to add Webmentions support to your website.

This library implements support for
[Webmentions](https://www.w3.org/TR/2016/WD-webmention-20160112/), a W3C
recommendation for federated mentions, comments and reactions on the Web.

The underlying mechanism of Webmentions is relatively simple:

- Alice publishes an article A1 on her blog A

- Bob reads A1 and mentions it in an article B1 on his blog B

- If both A and B support Webmentions, then:

  - A exposes an HTTP endpoint to receive mention notifications, and that
    endpoint is advertised on A1 as a `rel="webmention"` link in the page
    or a `Link` header in the HTTP response.

  - When Bob saves B1, its blog will scan for links in the page, discover
    which services expose a Webmention endpoint, and send a Webmention to
    A to notify them of the mention.

  - A will receive the notification and, if it accepts comments from B, the
    comment or the reaction will be rendered as a response on A1.

## Installation

```bash
pip install webmentions
```

## Usage

This library provides the bindings to send and process Webmentions, but it's
agnostic about how the Webmentions are stored and rendered, and agnostic about
the Web framework used by your application.

At the very least, you will have to:

- Extend `WebmentionsStorage` to store Webmentions on your database or
  filesystem.

- Create a `WebmentionsHandler` instances wired to that storage.

- Expose an HTTP endpoint that accepts Webmentions and forwards them to the
  handler.

- Render stored Webmentions as HTML.

### Minimal SQLAlchemy + FastAPI example

```python
# storage.py

from dataclasses import asdict
from datetime import datetime, timezone

import sqlalchemy as sa
from sqlalchemy.orm import declarative_base
from webmentions import Webmention, WebmentionDirection, WebmentionsStorage

Base = declarative_base()


class DbWebmention(Base):
  __tablename__ = 'webmentions'

  id = sa.Column(sa.Integer, primary_key=True, autoincrement=True)
  source = sa.Column(sa.String, nullable=False)
  target = sa.Column(sa.String, nullable=False)
  direction = sa.Column(
    sa.Enum('incoming', 'outgoing', name='direction')
  )
  title = sa.Column(sa.String)
  excerpt = sa.Column(sa.String)
  content = sa.Column(sa.String)
  author_name = sa.Column(sa.String)
  author_url = sa.Column(sa.String)
  author_photo = sa.Column(sa.String)
  published = sa.Column(sa.DateTime, nullable=False)
  mention_type = sa.Column(sa.String)
  created_at = sa.Column(sa.DateTime, nullable=False)
  updated_at = sa.Column(sa.DateTime, nullable=False)

  @classmethod
  def from_webmention(cls, webmention: Webmention):
    return cls(
      **{
        **asdict(webmention),
        'direction': webmention.direction.name.value,
        'mention_type': webmention.mention_type.name.value,
        'created_at': webmention.created_at or webmention.published or datetime.now(timezone.utc),
        'updated_at': datetime.now(timezone.utc),
      }
    )


class DbWebmentionsStorage(WebmentionsStorage):
  """
  Implements a simple database storage for Webmentions.
  """

  def __init__(self, engine: str):
    self.engine = sa.create_engine(engine)

  def store_webmention(self, webmention: Webmention):
    with self.engine.begin() as conn:
      conn.execute(
        sa.insert(DbWebmention).values(
          DbWebmention.from_webmention(webmention)
        )
      )

  def delete_webmention(
    self,
    source: str,
    target: str,
    direction: WebmentionDirection,
  ):
    with self.engine.begin() as conn:
      conn.execute(
        sa.delete(DbWebmention).where(
          sa.and_(
            DbWebmention.source == source,
            DbWebmention.target == target,
            DbWebmention.direction == direction.value
          )
        )
      )

  def retrieve_webmentions(
    self,
    resource: str,
    direction: WebmentionDirection,
  ):
    with self.engine.begin() as conn:
      return conn.execute(
        sa.select(DbWebmention).where(
          sa.and_(
            DbWebmention.target == resource,
            DbWebmention.direction == direction.value
          )
        )
      )
```

```python
# app.py

from webmentions import WebmentionsHandler


class MyApp:
  def __init__(self):
    self.webmentions_handler = WebmentionsHandler(
        storage=DbWebmentionsStorage('sqlite:///webmentions.db'),
        base_url="https://example.com",
    )
```

