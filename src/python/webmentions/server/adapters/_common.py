def webmention_link_header_value(endpoint: str) -> str:
    return f"<{endpoint}>; rel=\"webmention\""


def append_link_header(existing: str | None, to_add: str) -> str:
    if not existing:
        return to_add
    if to_add in existing:
        return existing
    return f"{existing}, {to_add}"
