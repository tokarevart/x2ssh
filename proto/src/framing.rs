use tokio::io::AsyncRead;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWrite;
use tokio::io::AsyncWriteExt;

pub async fn read_framed<R: AsyncRead + Unpin>(reader: &mut R) -> anyhow::Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;

    let mut packet = vec![0u8; len];
    reader.read_exact(&mut packet).await?;
    Ok(packet)
}

pub async fn write_framed<W: AsyncWrite + Unpin>(
    writer: &mut W,
    packet: &[u8],
) -> anyhow::Result<()> {
    let len = (packet.len() as u32).to_be_bytes();
    writer.write_all(&len).await?;
    writer.write_all(packet).await?;
    writer.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_round_trip() {
        let packet = b"Hello, World!";
        let mut buf = Vec::new();

        write_framed(&mut buf, packet).await.unwrap();

        let received = read_framed(&mut buf.as_slice()).await.unwrap();
        assert_eq!(received, packet);
    }

    #[tokio::test]
    async fn test_empty_packet() {
        let packet: &[u8] = b"";
        let mut buf = Vec::new();

        write_framed(&mut buf, packet).await.unwrap();

        let received = read_framed(&mut buf.as_slice()).await.unwrap();
        assert!(received.is_empty());
    }

    #[tokio::test]
    async fn test_large_packet() {
        let packet = vec![0u8; 65536]; // 64KB
        let mut buf = Vec::new();

        write_framed(&mut buf, &packet).await.unwrap();

        let received = read_framed(&mut buf.as_slice()).await.unwrap();
        assert_eq!(received.len(), 65536);
        assert_eq!(received, packet);
    }

    #[tokio::test]
    async fn test_multiple_packets() {
        let packets = vec![b"first".to_vec(), b"second".to_vec(), b"third".to_vec()];
        let mut buf = Vec::new();

        for packet in &packets {
            write_framed(&mut buf, packet).await.unwrap();
        }

        let mut cursor = buf.as_slice();
        for expected in &packets {
            let received = read_framed(&mut cursor).await.unwrap();
            assert_eq!(&received, expected);
        }
    }
}
