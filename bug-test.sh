docker kill $(docker ps -a -q) 2>&1>/dev/null
docker rm $(docker ps -a -q) 2>&1>/dev/null
docker network rm x2ssh-debug-net 2>&1>/dev/null

# Create network
docker network create --subnet=10.10.0.0/24 x2ssh-debug-net > /dev/null

# Start server ON the network with IP at creation time
SERVER=$(docker run -d --privileged \
  -v $(pwd)/tests/fixtures/keys:/tmp/keys:ro \
  --network x2ssh-debug-net \
  --ip 10.10.0.20 \
  x2ssh-vpn-server-target:latest)
echo "server started"

# Start client ON the network with IP at creation time
CLIENT=$(docker run -d --privileged \
  -v $(pwd)/tests/fixtures/keys:/tmp/keys:ro \
  -v $(pwd)/target/release/x2ssh:/usr/local/bin/x2ssh:ro \
  -v $(pwd)/tests/fixtures/vpn-test-config.toml:/etc/x2ssh/config.toml:ro \
  --network x2ssh-debug-net \
  --ip 10.10.0.10 \
  x2ssh-vpn-client:latest)
echo "client started"

# Wait for SSH
sleep 1

# Start VPN
docker exec $CLIENT sh -c 'RUST_LOG=info x2ssh --vpn --config /etc/x2ssh/config.toml -i /tmp/keys/id_ed25519 -p 22 root@10.10.0.20 > /tmp/x2ssh.log 2>&1 &'
echo "VPN started"

# Wait for VPN to establish
sleep 1

# Verify
docker exec $SERVER ip link show | grep tun
docker exec $CLIENT ping -c 2 10.8.0.1
