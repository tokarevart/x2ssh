"""Transport connection tests for x2ssh."""

import subprocess
import time
import socket
from pathlib import Path

import pytest

from socks5_client import Socks5Client
from ssh_server import SshContainer


def test_transport_check_alive_succeeds(ssh_container: SshContainer) -> None:
    """Test that x2ssh can connect and respond to health checks."""
    project_root = Path(__file__).parent.parent.parent

    # Find an available port
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    sock.bind(("127.0.0.1", 0))
    proxy_port: int = sock.getsockname()[1]
    sock.close()

    # Start x2ssh
    cmd = [
        "cargo",
        "run",
        "--",
        "-D",
        f"127.0.0.1:{proxy_port}",
        "-p",
        str(ssh_container.get_port()),
        "-i",
        str(ssh_container.get_key_path()),
        f"root@{ssh_container.host()}",
    ]

    process = subprocess.Popen(
        cmd, cwd=project_root, stdout=subprocess.PIPE, stderr=subprocess.PIPE
    )

    try:
        time.sleep(1)  # Wait for startup

        # Try to connect to the SOCKS5 proxy
        test_sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        test_sock.settimeout(5)
        test_sock.connect(("127.0.0.1", proxy_port))

        # Send SOCKS5 handshake
        test_sock.sendall(bytes([0x05, 0x01, 0x00]))
        response = test_sock.recv(2)

        assert response[0] == 0x05 and response[1] == 0x00, "Health check failed"
        test_sock.close()
    finally:
        process.terminate()
        try:
            _ = process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()


def test_transport_reconnect_after_connection_loss() -> None:
    """Test that x2ssh can reconnect after connection loss.

    Note: This test is complex to implement without the library internals.
    For now, we skip it as it requires testing the binary's internal reconnect logic.
    """
    pytest.skip(
        "Reconnect test requires internal library access - skip for binary testing"
    )


def test_transport_forward_to_echo_server(
    socks5_client: Socks5Client, echo_server_addr: tuple[str, int]
) -> None:
    """Test that data is correctly forwarded to echo server."""
    echo_host, echo_port = echo_server_addr
    sock = socks5_client.connect(echo_host, echo_port)

    try:
        # Send and verify echo
        test_data = b"hello"
        sock.sendall(test_data)
        response = sock.recv(1024)
        assert response == test_data
    finally:
        sock.close()


def test_transport_forward_after_reconnect() -> None:
    """Test forwarding works after reconnect.

    Note: This test is complex to implement without the library internals.
    For now, we skip it as it requires testing the binary's internal reconnect logic.
    """
    pytest.skip(
        "Forward after reconnect test requires internal library access - skip for binary testing"
    )
