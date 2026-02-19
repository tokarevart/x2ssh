"""SOCKS5 integration tests for x2ssh."""

import threading

from socks5_client import Socks5Client


def test_socks5_handshake_success(
    socks5_client: Socks5Client, echo_server_addr: tuple[str, int]
) -> None:
    """Test that SOCKS5 handshake completes successfully."""
    echo_host, echo_port = echo_server_addr
    sock = socks5_client.connect(echo_host, echo_port)

    # If we got here, handshake succeeded
    sock.close()


def test_socks5_connect_tcp_forward(
    socks5_client: Socks5Client, echo_server_addr: tuple[str, int]
) -> None:
    """Test data forwarding through SOCKS5 proxy to echo server."""
    echo_host, echo_port = echo_server_addr
    sock = socks5_client.connect(echo_host, echo_port)

    try:
        # Send data
        test_data = b"hello world"
        sock.sendall(test_data)

        # Receive echo
        response = sock.recv(1024)
        assert response == test_data, f"Expected {test_data!r}, got {response!r}"
    finally:
        sock.close()


def test_socks5_multiple_concurrent_connections(
    socks5_client: Socks5Client, echo_server_addr: tuple[str, int]
) -> None:
    """Test multiple concurrent connections through the proxy."""
    echo_host, echo_port = echo_server_addr

    def worker(thread_id: int) -> None:
        sock = socks5_client.connect(echo_host, echo_port)
        try:
            msg = f"message {thread_id}".encode()
            sock.sendall(msg)
            response = sock.recv(1024)
            assert response == msg, (
                f"Thread {thread_id}: expected {msg!r}, got {response!r}"
            )
        finally:
            sock.close()

    # Start 5 concurrent connections
    threads: list[threading.Thread] = []
    for i in range(5):
        t: threading.Thread = threading.Thread(target=worker, args=(i,))
        threads.append(t)
        t.start()

    # Wait for all to complete
    for t in threads:
        t.join()


def test_socks5_large_data_transfer(
    socks5_client: Socks5Client, echo_server_addr: tuple[str, int]
) -> None:
    """Test large data transfer through the proxy."""
    echo_host, echo_port = echo_server_addr
    sock = socks5_client.connect(echo_host, echo_port)

    try:
        # Send 100KB of data
        data = bytes([0xAB] * 100_000)
        sock.sendall(data)

        # Receive all data
        received = bytearray()
        while len(received) < len(data):
            chunk = sock.recv(4096)
            if not chunk:
                break
            received.extend(chunk)

        assert len(received) == len(data), (
            f"Expected {len(data)} bytes, got {len(received)}"
        )
        assert bytes(received) == data, "Data mismatch"
    finally:
        sock.close()
