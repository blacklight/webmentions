from dataclasses import dataclass
from enum import Enum

from webmentions import ContentTextFormat


class ContentChangeType(str, Enum):
    """
    Content change types.
    """

    ADDED = "added"
    EDITED = "edited"
    DELETED = "deleted"


@dataclass(frozen=True)
class ContentChange:
    """
    Content change model.
    """

    change_type: ContentChangeType
    path: str
    text: str | None
    text_format: ContentTextFormat | None
