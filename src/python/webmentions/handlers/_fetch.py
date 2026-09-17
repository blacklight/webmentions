"""
SSRF-guarded HTTP fetch for Webmention source parsing and delivery.

A Webmention receiver accepts an arbitrary ``source`` URL that it then
fetches, and outgoing delivery follows URLs a remote author's content
points at. A bare ``requests.get`` gives an attacker three things: an
internal-network read primitive (loopback, link-local metadata endpoints,
RFC1918 services reachable from the process), a redirect hop into that
same space even from a safe-looking public URL, and an unbounded body
bufferable in process memory.

``fetch_guarded`` closes all three: every redirect hop's hostname is
resolved and every resolved address must be globally routable, non-http(s)
redirect schemes are refused, and the body is streamed under a hard byte
cap. Policy violations raise ``ValueError`` (terminal rejection, mirroring
the parser's own validation errors); DNS failure raises
``requests.ConnectionError`` and socket errors propagate as
``requests.RequestException`` so callers can treat them as transient.

Private-network deployments that legitimately mention internal hosts can
disable the guard with ``ssrf_protection=False`` on the handler/processors.
"""

import ipaddress
import logging
import socket
from typing import Optional, Set
from urllib.parse import urljoin, urlparse

import requests

logger = logging.getLogger(__name__)

_MAX_REDIRECTS = 5
_CHUNK_SIZE = 65536


def _resolve_ips(hostname: str) -> Set[ipaddress.IPv4Address | ipaddress.IPv6Address]:
    """
    Resolve a hostname to its IP addresses.

    Module-level so tests can stub it. DNS failure raises
    ``requests.ConnectionError`` — transient, worth a retry.
    """
    try:
        infos = socket.getaddrinfo(hostname, None, proto=socket.IPPROTO_TCP)
    except OSError as exc:
        raise requests.ConnectionError(
            f"DNS resolution failed for {hostname}: {exc}"
        ) from exc

    ips = {
        ipaddress.ip_address(info[4][0])
        for info in infos
        if info[0] in (socket.AF_INET, socket.AF_INET6)
    }

    if not ips:
        raise requests.ConnectionError(
            f"DNS resolution returned no addresses for {hostname}"
        )

    return ips


def _is_public_ip(ip) -> bool:
    """
    Whether ``ip`` is a globally routable unicast address.

    ``is_global`` covers loopback, link-local, RFC1918, CGNAT, ULA,
    documentation and reserved ranges; multicast is checked separately.
    """
    return ip.is_global and not ip.is_multicast


def _check_url_host(url: str) -> str:
    """Validate ``url``'s host resolves only to public addresses."""
    host = (urlparse(url).hostname or "").lower()
    if not host:
        raise ValueError("URL has no host")
    ips = _resolve_ips(host)
    if not all(_is_public_ip(ip) for ip in ips):
        raise ValueError(f"host {host} resolves to a non-public address")
    return host


def _read_capped(resp: requests.Response, max_bytes: int) -> bytes:
    """Stream a response body under a hard byte cap."""
    body = bytearray()
    try:
        for chunk in resp.iter_content(chunk_size=_CHUNK_SIZE):
            if not chunk:
                continue
            if isinstance(chunk, str):
                chunk = chunk.encode("utf-8", errors="replace")
            body += chunk
            if len(body) > max_bytes:
                raise ValueError(f"response body exceeds {max_bytes} bytes")
    finally:
        resp.close()
    return bytes(body)


def fetch_guarded(
    url: str,
    *,
    timeout: float,
    user_agent: str,
    max_bytes: int,
    data: Optional[dict] = None,
    max_redirects: int = _MAX_REDIRECTS,
) -> requests.Response:
    """
    Fetch ``url`` with per-hop address validation and a body byte cap.

    Redirects are followed manually so each hop is re-validated: a public
    source cannot 302 into private address space, and a redirect to a
    non-http(s) scheme is refused. The returned response always has its
    body read (into ``_content``) so ``.text``/``.iter_content`` behave as
    callers expect. ``data`` switches to POST; 301/302/303 hops drop the
    body to GET per redirect semantics.
    """
    current = url
    body_data = data
    for _ in range(max_redirects + 1):
        _check_url_host(current)
        req_kwargs = {
            "timeout": timeout,
            "headers": {"User-Agent": user_agent},
            "allow_redirects": False,
            "stream": True,
        }

        method = requests.get
        if body_data is not None:
            method = requests.post
            req_kwargs["data"] = body_data

        resp = method(current, **req_kwargs)

        if resp.is_redirect or resp.is_permanent_redirect:
            location = resp.headers.get("Location")
            resp.close()
            if not location:
                resp._content = b""
                return resp
            nxt = urljoin(current, location)
            if urlparse(nxt).scheme not in ("http", "https"):
                raise ValueError(
                    f"redirect to non-http(s) scheme: {urlparse(nxt).scheme}"
                )
            if resp.status_code in (301, 302, 303):
                body_data = None
            current = nxt
            continue

        # Non-redirect: only success bodies are ever consumed by callers.
        if 200 <= resp.status_code < 300 or 300 <= resp.status_code < 400:
            resp._content = _read_capped(resp, max_bytes)
        else:
            resp.close()
            resp._content = b""
        return resp

    raise ValueError(f"too many redirects fetching {url}")
