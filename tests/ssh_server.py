"""SSH container management for x2ssh integration tests."""

from pathlib import Path

from testcontainers.core.container import DockerContainer
from testcontainers.core.wait_strategies import LogMessageWaitStrategy


class SshContainer:
    """Manages a Docker container with SSH server for testing."""

    container: DockerContainer | None
    port: int | None

    def __init__(self) -> None:
        self.container = None
        self.port = None

    def start(self) -> "SshContainer":
        """Start the SSH container and return the host port."""
        # Get the path to the test fixtures
        e2e_dir = Path(__file__).parent
        keys_dir = e2e_dir / "fixtures" / "keys"

        self.container = DockerContainer("x2ssh-test-sshd:latest")
        _ = self.container.with_volume_mapping(str(keys_dir), "/tmp/keys", mode="ro")
        _ = self.container.with_exposed_ports(22, 8080)
        _ = self.container.with_bind_ports(8080, 8080)  # Echo server

        # Wait for SSH server to be ready before starting
        self.container.waiting_for(LogMessageWaitStrategy("Server listening on"))
        _ = self.container.start()

        # Get the mapped port
        self.port = self.container.get_exposed_port(22)

        return self

    def stop(self) -> None:
        """Stop and remove the container."""
        if self.container:
            self.container.stop()

    def __enter__(self) -> "SshContainer":
        return self.start()

    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc_val: BaseException | None,
        exc_tb: object | None,
    ) -> None:
        self.stop()

    def host(self):
        """Return the host address."""
        return "127.0.0.1"

    def get_port(self):
        """Return the mapped SSH port."""
        return self.port

    def get_echo_port(self):
        """Return the echo server port (always 8080)."""
        return 8080

    def get_key_path(self):
        """Return the path to the SSH private key."""
        e2e_dir = Path(__file__).parent
        return e2e_dir / "fixtures" / "keys" / "id_ed25519"
