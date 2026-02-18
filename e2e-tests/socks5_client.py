"""SOCKS5 client for testing x2ssh proxy."""

import socket
import struct


class Socks5Client:
    """A simple SOCKS5 client for testing."""

    proxy_host: str
    proxy_port: int

    def __init__(self, proxy_host: str, proxy_port: int):
        self.proxy_host = proxy_host
        self.proxy_port = proxy_port

    def connect(self, target_host: str, target_port: int) -> socket.socket:
        """
        Connect to target through SOCKS5 proxy.

        Returns a connected socket that can be used to send/receive data.
        """
        # Create connection to proxy
        sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        sock.settimeout(10)
        sock.connect((self.proxy_host, self.proxy_port))

        # SOCKS5 handshake: version 5, 1 auth method (no auth)
        sock.sendall(bytes([0x05, 0x01, 0x00]))

        # Read response
        response = sock.recv(2)
        if len(response) != 2 or response[0] != 0x05 or response[1] != 0x00:
            sock.close()
            raise ConnectionError("SOCKS5 handshake failed")

        # Build connect request
        # Version (5), Command (1=connect), Reserved (0), Address type
        # Try to parse as IP address first
        try:
            addr = socket.inet_aton(target_host)
            # IPv4
            request = bytes([0x05, 0x01, 0x00, 0x01]) + addr
        except OSError:
            # Domain name
            host_bytes = target_host.encode("utf-8")
            request = bytes([0x05, 0x01, 0x00, 0x03, len(host_bytes)]) + host_bytes

        # Add port
        request += struct.pack(">H", target_port)

        # Send connect request
        sock.sendall(request)

        # Read response (at least 10 bytes for IPv4)
        response = sock.recv(256)
        if len(response) < 10 or response[1] != 0x00:
            sock.close()
            raise ConnectionError(f"SOCKS5 connect failed: {response}")

        return sock
