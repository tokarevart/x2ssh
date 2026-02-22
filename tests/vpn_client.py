"""VPN container management for x2ssh integration tests."""

import subprocess
import time
from pathlib import Path

import docker
import docker.errors as docker_errors
import docker.models.containers


class VpnTestEnv:
    """Manages Docker containers for VPN integration tests using docker compose."""

    COMPOSE_PROJECT = "x2ssh-vpn-test"
    SERVER_IP = "10.10.0.20"
    CLIENT_IP = "10.10.0.10"
    SERVER_TUN_IP = "10.8.0.1"
    CLIENT_TUN_IP = "10.8.0.2"

    def __init__(self, project_root: Path):
        self.project_root = project_root
        self.compose_file = (
            project_root / "tests" / "fixtures" / "docker-compose.vpn.yaml"
        )
        self.docker_client = docker.from_env()
        self.server: docker.models.containers.Container | None = None
        self.vpn_client: docker.models.containers.Container | None = None

    def start(self) -> None:
        """Start all containers using docker compose."""
        subprocess.run(
            [
                "docker",
                "compose",
                "-f",
                str(self.compose_file),
                "-p",
                self.COMPOSE_PROJECT,
                "up",
                "-d",
                "--build",
            ],
            check=True,
            capture_output=True,
        )
        self._wait_containers_ready()
        self._get_container_refs()

    def stop(self) -> None:
        """Stop and remove all containers using docker compose."""
        subprocess.run(
            [
                "docker",
                "compose",
                "-f",
                str(self.compose_file),
                "-p",
                self.COMPOSE_PROJECT,
                "down",
                "-v",
            ],
            check=True,
            capture_output=True,
        )
        self.server = None
        self.vpn_client = None

    def _wait_containers_ready(self, timeout: float = 30.0) -> None:
        """Wait for server container to be ready."""
        deadline = time.time() + timeout
        while time.time() < deadline:
            try:
                result = subprocess.run(
                    [
                        "docker",
                        "compose",
                        "-f",
                        str(self.compose_file),
                        "-p",
                        self.COMPOSE_PROJECT,
                        "logs",
                        "vpn-server",
                    ],
                    capture_output=True,
                    text=True,
                    check=True,
                )
                if "Server listening on" in result.stdout:
                    return
            except subprocess.CalledProcessError:
                pass
            time.sleep(0.5)
        raise TimeoutError("VPN server container not ready in time")

    def _get_container_refs(self) -> None:
        """Get container references for exec operations."""
        server_name = f"{self.COMPOSE_PROJECT}-vpn-server-1"
        client_name = f"{self.COMPOSE_PROJECT}-vpn-client-1"
        try:
            self.server = self.docker_client.containers.get(server_name)
        except docker_errors.NotFound:
            pass
        try:
            self.vpn_client = self.docker_client.containers.get(client_name)
        except docker_errors.NotFound:
            pass

    def exec_client(self, cmd: str) -> tuple[int, str]:
        """Execute command in client container, return (exit_code, output)."""
        if not self.vpn_client:
            raise RuntimeError("Client container not started")
        if any(c in cmd for c in "|&;<>()$`\n"):
            cmd = f'sh -c "{cmd}"'
        exit_code, output = self.vpn_client.exec_run(cmd)
        return exit_code, output.decode()

    def exec_server(self, cmd: str) -> tuple[int, str]:
        """Execute command in server container, return (exit_code, output)."""
        if not self.server:
            raise RuntimeError("Server container not started")
        if any(c in cmd for c in "|&;<>()$`\n"):
            cmd = f'sh -c "{cmd}"'
        exit_code, output = self.server.exec_run(cmd)
        return exit_code, output.decode()


class VpnSession:
    """Manages a VPN session for testing."""

    def __init__(self, env: VpnTestEnv):
        self.env = env

    def start_vpn(self, timeout: float = 30.0) -> None:
        """Start x2ssh --vpn in client container (background process)."""
        self.env.exec_client("pkill -INT -x x2ssh || true")
        self.env.exec_client(
            "RUST_LOG=info x2ssh --vpn --config /etc/x2ssh/config.toml "
            "-i /tmp/keys/id_ed25519 "
            "-p 22 root@10.10.0.20 "
            "> /tmp/x2ssh.log 2>&1 &"
        )

        deadline = time.time() + timeout
        while time.time() < deadline:
            if self.is_vpn_running() and self._is_tunnel_ready():
                return
            time.sleep(0.5)
        raise TimeoutError("VPN tunnel failed to establish")

    def stop_vpn(self) -> None:
        """Stop x2ssh process in client container."""
        self.env.exec_client("pkill -INT -x x2ssh || true")
        time.sleep(2)

    def is_vpn_running(self) -> bool:
        """Check if x2ssh process is running."""
        exit_code, _ = self.env.exec_client("pgrep -x x2ssh")
        return exit_code == 0

    def _is_tunnel_ready(self) -> bool:
        """Check if TUN interface exists on client."""
        exit_code, _ = self.env.exec_client("ip link show tun-x2ssh")
        return exit_code == 0

    def get_vpn_logs(self) -> str:
        """Get x2ssh logs from client container."""
        _, output = self.env.exec_client("cat /tmp/x2ssh.log 2>/dev/null || echo ''")
        return output
