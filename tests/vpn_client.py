"""VPN container management for x2ssh integration tests."""

import time
from pathlib import Path

import docker


class VpnTestEnv:
    """Manages Docker containers for VPN integration tests."""

    NETWORK_NAME = "x2ssh-vpn-test-net"
    NETWORK_SUBNET = "10.10.0.0/24"
    SERVER_IP = "10.10.0.20"
    CLIENT_IP = "10.10.0.10"

    def __init__(self, project_root: Path):
        self.project_root = project_root
        self.client = docker.from_env()
        self.network: docker.models.networks.Network | None = None
        self.server: docker.models.containers.Container | None = None
        self.vpn_client: docker.models.containers.Container | None = None

    def start(self) -> None:
        """Start all containers and network."""
        self._create_network()
        self._start_server()
        self._start_client()

    def stop(self) -> None:
        """Stop and remove all containers and network."""
        for container in [self.vpn_client, self.server]:
            if container:
                container.remove(force=True, v=True)
        if self.network:
            try:
                self.network.remove()
            except docker.errors.APIError:
                pass

    def _create_network(self) -> None:
        try:
            existing = self.client.networks.get(self.NETWORK_NAME)
            existing.remove()
        except docker.errors.NotFound:
            pass

        ipam_pool = docker.types.IPAMPool(subnet=self.NETWORK_SUBNET)
        ipam_config = docker.types.IPAMConfig(pool_configs=[ipam_pool])
        self.network = self.client.networks.create(
            self.NETWORK_NAME, driver="bridge", ipam=ipam_config
        )

    def _start_server(self) -> None:
        fixtures = self.project_root / "tests" / "fixtures"
        self.server = self.client.containers.run(
            "x2ssh-vpn-server-target:latest",
            detach=True,
            privileged=True,
            volumes={str(fixtures / "keys"): {"bind": "/tmp/keys", "mode": "ro"}},
        )
        if self.network:
            self.network.connect(self.server, ipv4_address=self.SERVER_IP)
        self._wait_log(self.server, "Server listening on")

    def _start_client(self) -> None:
        fixtures = self.project_root / "tests" / "fixtures"
        target = self.project_root / "target" / "release"
        self.vpn_client = self.client.containers.run(
            "x2ssh-vpn-client:latest",
            detach=True,
            privileged=True,
            volumes={
                str(fixtures / "keys"): {"bind": "/tmp/keys", "mode": "ro"},
                str(target / "x2ssh"): {"bind": "/usr/local/bin/x2ssh", "mode": "ro"},
                str(fixtures / "vpn-test-config.toml"): {
                    "bind": "/etc/x2ssh/config.toml",
                    "mode": "ro",
                },
            },
        )
        if self.network:
            self.network.connect(self.vpn_client, ipv4_address=self.CLIENT_IP)

    def _wait_log(
        self,
        container: docker.models.containers.Container,
        pattern: str,
        timeout: float = 30,
    ) -> None:
        deadline = time.time() + timeout
        while time.time() < deadline:
            logs = container.logs().decode()
            if pattern in logs:
                return
            time.sleep(0.5)
        raise TimeoutError(f"Pattern '{pattern}' not found in container logs")

    def exec_client(self, cmd: str) -> tuple[int, str]:
        """Execute command in client container, return (exit_code, output)."""
        if not self.vpn_client:
            raise RuntimeError("Client container not started")
        exit_code, output = self.vpn_client.exec_run(cmd)
        return exit_code, output.decode()

    def exec_server(self, cmd: str) -> tuple[int, str]:
        """Execute command in server container, return (exit_code, output)."""
        if not self.server:
            raise RuntimeError("Server container not started")
        exit_code, output = self.server.exec_run(cmd)
        return exit_code, output.decode()


class VpnSession:
    """Manages a VPN session for testing."""

    def __init__(self, env: VpnTestEnv):
        self.env = env

    def start_vpn(self) -> None:
        """Start x2ssh --vpn in client container (background process)."""
        self.env.exec_client(
            "x2ssh --vpn -i /tmp/keys/id_ed25519 -p 22 root@10.10.0.20 "
            "> /tmp/x2ssh.log 2>&1 &"
        )
        time.sleep(3)

    def stop_vpn(self) -> None:
        """Stop x2ssh process in client container."""
        self.env.exec_client("pkill -x x2ssh || true")
        time.sleep(1)

    def is_vpn_running(self) -> bool:
        """Check if x2ssh process is running."""
        exit_code, _ = self.env.exec_client("pgrep -x x2ssh")
        return exit_code == 0
