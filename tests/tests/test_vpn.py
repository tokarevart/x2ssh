"""VPN integration tests for x2ssh."""

import time

from vpn_client import VpnSession, VpnTestEnv

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
# Phase 3 Tests: VPN Tunnel
# =============================================================================


def test_vpn_tunnel_establishment(vpn_session: VpnSession) -> None:
    """Verify TUN interfaces exist on both client and server after VPN starts."""
    code, output = vpn_session.env.exec_client("ip link show tun-x2ssh")
    assert code == 0, f"Client TUN interface not found: {output}"
    assert "tun-x2ssh" in output, f"Client TUN not named correctly: {output}"

    code, output = vpn_session.env.exec_server("ip link show | grep -E 'tun[0-9]'")
    assert code == 0, f"Server TUN interface not found: {output}"


def test_vpn_tcp_through_tunnel(vpn_session: VpnSession) -> None:
    """Test TCP traffic through VPN tunnel."""
    server_tun_ip = VpnTestEnv.SERVER_TUN_IP
    code, output = vpn_session.env.exec_client(
        f"echo 'vpn_test' | nc -w3 {server_tun_ip} 8080"
    )
    assert code == 0, f"TCP through VPN failed: {output}"
    assert "vpn_test" in output, f"Expected 'vpn_test' in output, got: {output}"


def test_vpn_udp_through_tunnel(vpn_session: VpnSession) -> None:
    """Test UDP traffic through VPN tunnel."""
    server_tun_ip = VpnTestEnv.SERVER_TUN_IP
    code, output = vpn_session.env.exec_client(
        f"echo 'udp_test' | nc -u -w3 {server_tun_ip} 8081"
    )
    assert code == 0, f"UDP through VPN failed: {output}"
    assert "udp_test" in output, f"Expected 'udp_test' in output, got: {output}"


def test_vpn_ping_through_tunnel(vpn_session: VpnSession) -> None:
    """Test ICMP traffic through VPN tunnel."""
    server_tun_ip = VpnTestEnv.SERVER_TUN_IP
    code, output = vpn_session.env.exec_client(f"ping -c 2 -W 3 {server_tun_ip}")
    assert code == 0, f"Ping through VPN failed: {output}"
    assert "2 packets transmitted, 2 received" in output or "2 received" in output, (
        f"Expected successful ping, got: {output}"
    )


def test_vpn_post_up_hooks_executed(vpn_session: VpnSession) -> None:
    """Verify PostUp hooks set up iptables rules."""
    code, output = vpn_session.env.exec_server(
        "iptables -t nat -L POSTROUTING -n | grep MASQUERADE"
    )
    assert code == 0, f"PostUp iptables rule not found: {output}"
    assert "MASQUERADE" in output, f"MASQUERADE rule not set: {output}"

    code, output = vpn_session.env.exec_server("cat /proc/sys/net/ipv4/ip_forward")
    assert code == 0, f"Could not read ip_forward: {output}"
    assert output.strip() == "1", f"IP forwarding not enabled: {output}"


def test_vpn_default_route_via_tun(vpn_session: VpnSession) -> None:
    """Verify default route points to TUN interface."""
    code, output = vpn_session.env.exec_client("ip route show default")
    assert code == 0, f"Could not get default route: {output}"
    assert "tun-x2ssh" in output or "10.8.0.1" in output, (
        f"Default route not via TUN: {output}"
    )


def test_vpn_pre_down_cleanup(vpn_session: VpnSession) -> None:
    """Verify PreDown hooks execute on disconnect."""
    assert vpn_session.is_vpn_running(), "VPN should be running before cleanup test"

    code, _ = vpn_session.env.exec_server(
        "iptables -t nat -L POSTROUTING -n | grep MASQUERADE"
    )
    assert code == 0, "MASQUERADE rule should exist before disconnect"

    vpn_session.stop_vpn()

    time.sleep(2)

    code, output = vpn_session.env.exec_server(
        "iptables -t nat -L POSTROUTING -n | grep MASQUERADE"
    )
    assert code != 0 or "MASQUERADE" not in output, (
        f"PreDown should have removed MASQUERADE rule, but found: {output}"
    )
