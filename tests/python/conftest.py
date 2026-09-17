import ipaddress

import pytest


@pytest.fixture(autouse=True)
def _public_dns(monkeypatch):
    """Stub guarded-fetch DNS so test hosts resolve as public addresses.

    IP-literal hosts resolve to themselves (as real DNS would), so URLs
    like ``http://169.254.169.254/x`` still exercise the private-address
    guard in tests that use it. Hostname lookups never hit real DNS.
    """

    def fake_resolve(host):
        try:
            return {ipaddress.ip_address(host)}
        except ValueError:
            return {ipaddress.ip_address("93.184.216.34")}

    monkeypatch.setattr("webmentions.handlers._fetch._resolve_ips", fake_resolve)
