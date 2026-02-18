"""Pytest configuration and fixtures for x2ssh e2e tests."""

import socket
import subprocess
import time
from collections.abc import Iterator
from pathlib import Path

import pytest

from socks5_client import Socks5Client
from ssh_server import SshContainer


@pytest.fixture(scope="session")
def project_root() -> Path:
    """Return the project root directory."""
    return Path(__file__).parent.parent


@pytest.fixture(scope="session")
def ssh_container() -> Iterator[SshContainer]:
    """Provide a running SSH container for the test session."""
    container = SshContainer()
    _ = container.start()
    yield container
    container.stop()


@pytest.fixture
def x2ssh_process(ssh_container: SshContainer) -> Iterator[dict[str, object]]:
    """Start x2ssh process and provide proxy address."""
    project_root = Path(__file__).parent.parent

    # Find an available port for SOCKS5 proxy
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    sock.bind(("127.0.0.1", 0))
    proxy_port: int = sock.getsockname()[1]
    sock.close()

    # Build x2ssh command
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

    # Start x2ssh process
    process = subprocess.Popen(
        cmd, cwd=project_root, stdout=subprocess.PIPE, stderr=subprocess.PIPE
    )

    # Wait for proxy to be ready (check port is listening)
    for _ in range(30):  # Wait up to 3 seconds
        try:
            test_sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            test_sock.settimeout(0.1)
            test_sock.connect(("127.0.0.1", proxy_port))
            test_sock.close()
            break
        except (socket.error, ConnectionRefusedError):
            time.sleep(0.1)
    else:
        process.terminate()
        stdout, stderr = process.communicate(timeout=5)
        raise RuntimeError(
            f"x2ssh proxy failed to start. "
            f"STDOUT: {stdout.decode() if stdout else 'N/A'}, "
            f"STDERR: {stderr.decode() if stderr else 'N/A'}"
        )

    yield {"process": process, "proxy_host": "127.0.0.1", "proxy_port": proxy_port}

    # Cleanup
    process.terminate()
    try:
        _ = process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        process.kill()


@pytest.fixture
def socks5_client(x2ssh_process: dict[str, object]) -> Socks5Client:
    """Provide a SOCKS5 client connected to the x2ssh proxy."""
    proxy_host = x2ssh_process["proxy_host"]
    proxy_port = x2ssh_process["proxy_port"]
    assert isinstance(proxy_host, str)
    assert isinstance(proxy_port, int)
    return Socks5Client(proxy_host, proxy_port)


@pytest.fixture
def echo_server_addr(ssh_container: SshContainer) -> tuple[str, int]:
    """Return the echo server address."""
    return ("127.0.0.1", 8080)
