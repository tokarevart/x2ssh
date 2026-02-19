FROM alpine:3.19

RUN apk add --no-cache openssh socat

RUN mkdir -p /root/.ssh && chmod 700 /root/.ssh

COPY keys/id_ed25519.pub /root/.ssh/authorized_keys
RUN chmod 600 /root/.ssh/authorized_keys

RUN ssh-keygen -A

RUN echo "PermitRootLogin prohibit-password" >> /etc/ssh/sshd_config && \
    echo "PubkeyAuthentication yes" >> /etc/ssh/sshd_config && \
    echo "PasswordAuthentication no" >> /etc/ssh/sshd_config && \
    sed -i 's/AllowTcpForwarding no/AllowTcpForwarding yes/' /etc/ssh/sshd_config

EXPOSE 22 8080

CMD ["/bin/sh", "-c", "socat TCP-LISTEN:8080,fork EXEC:'cat' & exec /usr/sbin/sshd -D -e"]
