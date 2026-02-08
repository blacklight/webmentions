import logging
from typing import Callable

from .._model import Webmention

logger = logging.getLogger(__name__)


def on_mention_callback_wrapper(
    callback: Callable | None,
) -> Callable[[Webmention], None]:
    """
    Wrap a mention callback to catch exceptions and log them.
    """

    def wrapper(mention: Webmention):
        if callback:
            try:
                callback(mention)
            except Exception as e:
                logger.error(
                    "Error on mention processed callback %s: <source=%s target=%s direction=%s>: %s",
                    callback,
                    mention.source,
                    mention.target,
                    mention.direction.value,
                    str(e),
                )

    return wrapper
