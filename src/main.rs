use clap::Parser;
use fast_socks5::Socks5Command;
use fast_socks5::server::{
    DnsResolveHelper, ErrorContext, Socks5ServerProtocol, SocksServerError, states,
};
use russh::keys::{PrivateKeyWithHashAlg, PublicKey};
use std::net::{Ipv4Addr, SocketAddr, ToSocketAddrs as _};
use std::path::{Path, PathBuf};
use std::pin::pin;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, ToSocketAddrs};

#[derive(clap::Parser, Clone, Debug)]
struct Cli {
    #[clap(short, long)]
    listen_addr: SocketAddr,

    #[clap(long)]
    ssh_host: String,

    #[clap(long, default_value = "22")]
    ssh_port: u16,

    #[clap(long, default_value = "root")]
    ssh_username: String,

    #[clap(long, short = 'k')]
    private_key: PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let cli = Cli::parse();

    tracing::info!("SOCKS5 listen address: {}", cli.listen_addr);
    tracing::info!("SSH host: {}", cli.ssh_host);
    tracing::info!("SSH port: {}", cli.ssh_port);
    tracing::info!("SSH username: {}", cli.ssh_username);
    tracing::info!("SSH key path: {}", cli.private_key.display());

    socks5_server(cli).await?;
    // ping_ssh_server(cli).await?;

    Ok(())
}

async fn ping_ssh_server(cli: Cli) -> anyhow::Result<()> {
    let mut ssh = Session::connect(
        cli.private_key,
        cli.ssh_username,
        (cli.ssh_host, cli.ssh_port),
    )
    .await?;
    tracing::info!("connected");

    let forward_to = (Ipv4Addr::new(127, 0, 0, 1), 4444);
    let client = tokio::io::join(b"ping\n" as &[_], tokio::io::stdout());
    ssh.forward(forward_to, client).await?;
    ssh.close().await?;

    Ok(())
}

async fn socks5_server(cli: Cli) -> anyhow::Result<()> {
    let mut session = Arc::new(
        Session::connect(
            &cli.private_key,
            &cli.ssh_username,
            (cli.ssh_host.clone(), cli.ssh_port),
        )
        .await?,
    );
    tracing::info!("SSH session established");

    let listener = TcpListener::bind(&cli.listen_addr).await?;

    loop {
        match listener.accept().await {
            Ok((socket, client_addr)) => {
                tracing::debug!("accepted connection from {client_addr}");
                if let Err(e) = serve_socks5(session.clone(), socket).await {
                    tracing::error!("{:#}", &e);
                    session = Arc::new(
                        Session::connect(
                            &cli.private_key,
                            &cli.ssh_username,
                            (cli.ssh_host.clone(), cli.ssh_port),
                        )
                        .await?,
                    );
                    tracing::info!("SSH session reconnected");
                }
            }
            Err(err) => {
                tracing::error!("accept error = {:?}", err);
            }
        }
    }

    // session.close().await?;
}

async fn serve_socks5(session: Arc<Session>, socket: tokio::net::TcpStream) -> anyhow::Result<()> {
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
        Socks5Command::UDPAssociate => tracing::warn!("UDP is not supported yet"),
        _ => anyhow::bail!("command not supported"),
    }

    Ok(())
}

pub async fn run_tcp_proxy(
    proto: Socks5ServerProtocol<TcpStream, states::CommandRead>,
    mut socket: impl AsyncRead + AsyncWrite + Unpin,
) -> anyhow::Result<TcpStream> {
    tracing::debug!("Connected to remote destination");

    let mut inner = proto
        .reply_success((Ipv4Addr::new(127, 0, 0, 1), 0).into())
        .await?;

    fast_socks5::server::transfer(&mut inner, &mut socket).await;

    Ok(inner)
}

async fn try_notify<T, P: AsyncRead + AsyncWrite + Unpin>(
    proto: Socks5ServerProtocol<P, states::CommandRead>,
    res: Result<T, fast_socks5::server::SocksServerError>,
) -> anyhow::Result<(T, Socks5ServerProtocol<P, states::CommandRead>)> {
    match res {
        Ok(x) => Ok((x, proto)),
        Err(e) => {
            if let Err(rep_err) = proto.reply_error(&e.to_reply_error()).await {
                tracing::error!("error while reporting an error to the client: {}", rep_err);
            }

            return Err(e.into());
        }
    }
}

struct Client {}

// More SSH event handlers can be defined in this trait
// In this example, we're only using Channel, so these aren't needed.
impl russh::client::Handler for Client {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &PublicKey,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

/// This struct is a convenience wrapper
/// around a russh client
/// that handles the input/output event loop
pub struct Session {
    session: russh::client::Handle<Client>,
}

impl Session {
    async fn connect<P: AsRef<Path>, A: ToSocketAddrs>(
        key_path: P,
        user: impl Into<String>,
        addrs: A,
    ) -> anyhow::Result<Self> {
        let key_pair = russh::keys::load_secret_key(key_path, None)?;

        let config = russh::client::Config {
            ..Default::default()
        };

        let config = Arc::new(config);
        let sh = Client {};

        let mut session = russh::client::connect(config, addrs, sh).await?;

        let auth_res = session
            .authenticate_publickey(
                user,
                PrivateKeyWithHashAlg::new(
                    Arc::new(key_pair),
                    session.best_supported_rsa_hash().await?.flatten(),
                ),
            )
            .await?;

        if !auth_res.success() {
            anyhow::bail!("Authentication (with publickey) failed");
        }

        Ok(Self { session })
    }

    async fn forward(
        &self,
        to: impl ToSocketAddrs,
        client: impl AsyncRead + AsyncWrite + Send + Unpin + 'static,
    ) -> anyhow::Result<()> {
        let to = tokio::net::lookup_host(to).await?.next().unwrap();
        let channel = self
            .session
            .channel_open_direct_tcpip(to.ip().to_string(), to.port() as _, "127.0.0.1", 0)
            .await?;

        let (mut ssh_rx, ssh_tx) = channel.split();

        let (client_rx, client_tx) = tokio::io::split(client);

        let jh = tokio::spawn(async move {
            let mut client = pin!(client_rx);
            let mut buf = Vec::with_capacity(4096);
            loop {
                match client.read_buf(&mut buf).await {
                    Ok(0) => {
                        ssh_tx.close().await?;
                        return anyhow::Ok(());
                    }
                    Ok(_) => {
                        ssh_tx.data(&*buf).await?;
                        buf.clear();
                    }
                    Err(e) => return Err(e.into()),
                }
            }
        });

        let mut client = pin!(client_tx);
        while let Some(msg) = ssh_rx.wait().await {
            match msg {
                russh::ChannelMsg::Data { ref data } => {
                    client.write_all(&data).await?;
                    client.flush().await?;
                }
                x => {
                    dbg!(x);
                }
            }
        }

        jh.abort();

        Ok(())
    }

    async fn close(&mut self) -> anyhow::Result<()> {
        self.session
            .disconnect(russh::Disconnect::ByApplication, "", "en")
            .await?;

        Ok(())
    }
}
