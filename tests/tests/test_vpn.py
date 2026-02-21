"""VPN integration tests for x2ssh."""

import pytest

from vpn_client import VpnTestEnv


# =============================================================================
# Phase 2 Tests: Container Setup (enabled)
# =============================================================================


def test_vpn_client_container_has_required_tools(vpn_env: VpnTestEnv) -> None:
    """Verify client container has iproute2, iptables, nc, ping, ssh."""
    code, _ = vpn_env.exec_client("which ip iptables nc ping ssh")
    assert code == 0, "Client container missing required tools"


def test_vpn_server_container_has_sshd(vpn_env: VpnTestEnv) -> None:
    """Verify server container has sshd running."""
    code, _ = vpn_env.exec_server("pgrep sshd")
    assert code == 0, "Server container sshd not running"


def test_vpn_server_tcp_echo_service(vpn_env: VpnTestEnv) -> None:
    """Verify TCP echo service responds on server port 8080."""
    code, output = vpn_env.exec_client("echo hello | nc -w2 10.10.0.20 8080")
    assert code == 0, f"TCP echo failed with code {code}"
    assert "hello" in output, f"Expected 'hello' in output, got: {output}"


def test_vpn_client_can_ssh_to_server(vpn_env: VpnTestEnv) -> None:
    """Verify client can SSH to server."""
    code, output = vpn_env.exec_client(
        "ssh -i /tmp/keys/id_ed25519 "
        "-o StrictHostKeyChecking=no "
        "-o BatchMode=yes "
        "root@10.10.0.20 'echo ssh_ok'"
    )
    assert code == 0, f"SSH failed: {output}"
    assert "ssh_ok" in output, f"Expected 'ssh_ok' in output, got: {output}"


def test_vpn_client_has_tun_device_access(vpn_env: VpnTestEnv) -> None:
    """Verify client container can access /dev/net/tun for TUN creation."""
    code, output = vpn_env.exec_client("ls -la /dev/net/tun")
    assert code == 0, f"TUN device not accessible: {output}"
    assert "/dev/net/tun" in output, f"TUN device not found: {output}"


def test_vpn_server_has_tun_device_access(vpn_env: VpnTestEnv) -> None:
    """Verify server container can access /dev/net/tun for TUN creation."""
    code, output = vpn_env.exec_server("ls -la /dev/net/tun")
    assert code == 0, f"TUN device not accessible: {output}"
    assert "/dev/net/tun" in output, f"TUN device not found: {output}"


def test_vpn_x2ssh_binary_exists(vpn_env: VpnTestEnv) -> None:
    """Verify x2ssh binary is mounted in client container."""
    code, _ = vpn_env.exec_client("test -x /usr/local/bin/x2ssh")
    assert code == 0, "x2ssh binary not found or not executable"


# =============================================================================
# Phase 3 Tests: VPN Tunnel (disabled until Phase 3 is complete)
# =============================================================================


@pytest.mark.skip(reason="Phase 3 - requires VPN tunnel implementation")
def test_vpn_tunnel_establishment(vpn_session) -> None:
    """Verify TUN interfaces exist on both client and server after VPN starts."""
    pass


@pytest.mark.skip(reason="Phase 3 - requires VPN tunnel implementation")
def test_vpn_tcp_through_tunnel(vpn_session) -> None:
    """Test TCP traffic through VPN tunnel."""
    pass


@pytest.mark.skip(reason="Phase 3 - requires VPN tunnel implementation")
def test_vpn_udp_through_tunnel(vpn_session) -> None:
    """Test UDP traffic through VPN tunnel."""
    pass


@pytest.mark.skip(reason="Phase 3 - requires VPN tunnel implementation")
def test_vpn_ping_through_tunnel(vpn_session) -> None:
    """Test ICMP traffic through VPN tunnel."""
    pass


@pytest.mark.skip(reason="Phase 3 - requires PostUp/PreDown implementation")
def test_vpn_post_up_hooks_executed(vpn_session) -> None:
    """Verify PostUp hooks set up iptables rules."""
    pass


@pytest.mark.skip(reason="Phase 3 - requires PostUp/PreDown implementation")
def test_vpn_pre_down_cleanup(vpn_session) -> None:
    """Verify PreDown hooks execute on disconnect."""
    pass


@pytest.mark.skip(reason="Phase 3 - requires routing implementation")
def test_vpn_default_route_via_tun(vpn_session) -> None:
    """Verify default route points to TUN interface."""
    pass
