use std::net::Ipv4Addr;
use std::net::ToSocketAddrs;
use std::sync::Arc;

use fast_socks5::Socks5Command;
use fast_socks5::server::DnsResolveHelper;
use fast_socks5::server::ErrorContext;
use fast_socks5::server::Socks5ServerProtocol;
use fast_socks5::server::SocksServerError;
use fast_socks5::server::states;
use tokio::io::AsyncRead;
use tokio::io::AsyncWrite;
use tokio::net::TcpStream;
use tracing::debug;
use tracing::error;
use tracing::warn;

use crate::transport::Transport;

pub async fn serve(session: Arc<Transport>, socket: TcpStream) -> anyhow::Result<()> {
    let (proto, cmd, target_addr) = Socks5ServerProtocol::accept_no_auth(socket)
        .await?
        .read_command()
        .await?
        .resolve_dns()
        .await?;

    let (addr, proto) = try_notify(
        proto,
        target_addr
            .to_socket_addrs()
            .err_when("converting to socket addr")
            .and_then(|mut addrs| addrs.next().ok_or(SocksServerError::Bug("no socket addrs"))),
    )
    .await?;

    match cmd {
        Socks5Command::TCPConnect => {
            let (s0, s1) = tokio::io::duplex(4096);

            tokio::select! {
                Err(e) = session.forward(addr, s0) => return Err(e),
                Err(e) = run_tcp_proxy(proto, s1) => return Err(e),
                else => {}
            }
        }
        Socks5Command::UDPAssociate => warn!("UDP is not supported yet"),
        _ => anyhow::bail!("command not supported"),
    }

    Ok(())
}

async fn run_tcp_proxy(
    proto: Socks5ServerProtocol<TcpStream, states::CommandRead>,
    mut socket: impl AsyncRead + AsyncWrite + Unpin,
) -> anyhow::Result<TcpStream> {
    debug!("Connected to remote destination");

    let mut inner = proto
        .reply_success((Ipv4Addr::new(127, 0, 0, 1), 0).into())
        .await?;

    fast_socks5::server::transfer(&mut inner, &mut socket).await;

    Ok(inner)
}

async fn try_notify<T, P: AsyncRead + AsyncWrite + Unpin>(
    proto: Socks5ServerProtocol<P, states::CommandRead>,
    res: Result<T, SocksServerError>,
) -> anyhow::Result<(T, Socks5ServerProtocol<P, states::CommandRead>)> {
    match res {
        Ok(x) => Ok((x, proto)),
        Err(e) => {
            if let Err(rep_err) = proto.reply_error(&e.to_reply_error()).await {
                error!("error while reporting an error to the client: {}", rep_err);
            }
            Err(e.into())
        }
    }
}
