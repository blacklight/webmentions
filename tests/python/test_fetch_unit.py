import pytest
import requests

from webmentions.handlers._fetch import fetch_guarded


class _FakeResponse:
    def __init__(
        self,
        *,
        status_code: int = 200,
        text: str = "",
        body: bytes | None = None,
        headers: dict | None = None,
        url: str = "https://example.com/x",
    ):
        self.status_code = status_code
        self.text = text
        self.body = body
        self.headers = headers or {}
        self.url = url
        self.encoding = "utf-8"
        self.closed = False
        self._content = b""
        has_location = any(k.lower() == "location" for k in self.headers)
        self.is_redirect = has_location and status_code in (301, 302, 303, 307, 308)
        self.is_permanent_redirect = has_location and status_code in (301, 308)

    def raise_for_status(self):
        if self.status_code >= 400:
            raise requests.HTTPError(f"{self.status_code}", response=self)

    def iter_content(self, chunk_size=65536, decode_unicode=False):
        body = self.body if self.body is not None else self.text.encode(self.encoding)
        for i in range(0, len(body), chunk_size):
            yield body[i : i + chunk_size]

    def close(self):
        self.closed = True


def _fetch(url, **kwargs):
    kwargs.setdefault("timeout", 5.0)
    kwargs.setdefault("user_agent", "test-agent")
    kwargs.setdefault("max_bytes", 1024)
    return fetch_guarded(url, **kwargs)


@pytest.mark.parametrize(
    "url",
    [
        "http://169.254.169.254/latest/meta-data",
        "http://127.0.0.1:6379/",
        "http://10.0.0.5/internal",
        "http://192.168.1.1/admin",
        "http://[::1]/x",
    ],
)
def test_private_addresses_blocked_before_connect(url, monkeypatch):
    fetches = []
    monkeypatch.setattr(requests, "get", lambda *a, **k: fetches.append(a))

    with pytest.raises(ValueError, match="non-public address"):
        _fetch(url)

    assert fetches == []


def test_redirect_to_private_address_blocked_at_hop(monkeypatch):
    fetches = []

    def _get(url, **kwargs):
        fetches.append(url)
        return _FakeResponse(
            status_code=302,
            headers={"Location": "http://169.254.169.254/x"},
        )

    monkeypatch.setattr(requests, "get", _get)

    with pytest.raises(ValueError, match="non-public address"):
        _fetch("https://blog.example/post")

    assert fetches == ["https://blog.example/post"]


def test_validated_redirect_is_followed(monkeypatch):
    calls = []

    def _get(url, **kwargs):
        calls.append(url)
        if "blog.example" in url:
            return _FakeResponse(
                status_code=302, headers={"Location": "https://cdn.example/post"}
            )
        return _FakeResponse(text="<html>body</html>", url=url)

    monkeypatch.setattr(requests, "get", _get)

    resp = _fetch("https://blog.example/post")

    assert calls == ["https://blog.example/post", "https://cdn.example/post"]
    assert resp.status_code == 200
    assert resp.text == "<html>body</html>"


def test_response_body_is_capped(monkeypatch):
    monkeypatch.setattr(
        requests,
        "get",
        lambda *a, **k: _FakeResponse(body=b"x" * 4096),
    )

    with pytest.raises(ValueError, match="exceeds 1024 bytes"):
        _fetch("https://blog.example/big")


def test_non_http_redirect_scheme_refused(monkeypatch):
    monkeypatch.setattr(
        requests,
        "get",
        lambda *a, **k: _FakeResponse(
            status_code=302, headers={"Location": "file:///etc/passwd"}
        ),
    )

    with pytest.raises(ValueError, match="non-http"):
        _fetch("https://blog.example/post")


def test_too_many_redirects_rejected(monkeypatch):
    monkeypatch.setattr(
        requests,
        "get",
        lambda *a, **k: _FakeResponse(
            status_code=302, headers={"Location": "https://blog.example/next"}
        ),
    )

    with pytest.raises(ValueError, match="too many redirects"):
        _fetch("https://blog.example/post", max_redirects=3)


def test_post_redirect_drops_body_to_get(monkeypatch):
    calls = []

    def _post(url, data=None, **kwargs):
        calls.append(("post", url, data))
        return _FakeResponse(
            status_code=302, headers={"Location": "https://target.example/wm2"}
        )

    def _get(url, **kwargs):
        calls.append(("get", url, None))
        return _FakeResponse(status_code=202)

    monkeypatch.setattr(requests, "post", _post)
    monkeypatch.setattr(requests, "get", _get)

    resp = _fetch("https://target.example/wm", data={"source": "s", "target": "t"})

    assert resp.status_code == 202
    assert calls == [
        ("post", "https://target.example/wm", {"source": "s", "target": "t"}),
        ("get", "https://target.example/wm2", None),
    ]


def test_dns_failure_raises_connection_error(monkeypatch):
    def _boom(*a, **k):
        raise requests.ConnectionError("name resolution failed")

    monkeypatch.setattr("webmentions.handlers._fetch._resolve_ips", _boom)

    with pytest.raises(requests.ConnectionError):
        _fetch("https://blog.example/post")


def test_error_status_skips_body_read(monkeypatch):
    resp_holder = {}

    def _get(url, **kwargs):
        resp = _FakeResponse(status_code=500, body=b"x" * 999999)
        resp_holder["resp"] = resp
        return resp

    monkeypatch.setattr(requests, "get", _get)

    resp = _fetch("https://blog.example/post")

    assert resp.status_code == 500
    assert resp_holder["resp"].closed
