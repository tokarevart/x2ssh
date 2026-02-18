"""SSH container management for x2ssh e2e tests."""

from pathlib import Path

from testcontainers.core.container import DockerContainer
from testcontainers.core.waiting_utils import wait_for_logs


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
        project_root = Path(__file__).parent.parent
        fixtures_dir = project_root / "tests" / "fixtures"
        keys_dir = fixtures_dir / "keys"

        self.container = DockerContainer("x2ssh-test-sshd:latest")
        _ = self.container.with_volume_mapping(str(keys_dir), "/tmp/keys", mode="ro")
        _ = self.container.with_exposed_ports(22, 8080)
        _ = self.container.with_bind_ports(8080, 8080)  # Echo server

        _ = self.container.start()

        # Wait for SSH server to be ready
        _ = wait_for_logs(self.container, "Server listening on")

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
        project_root = Path(__file__).parent.parent
        return project_root / "tests" / "fixtures" / "keys" / "id_ed25519"
