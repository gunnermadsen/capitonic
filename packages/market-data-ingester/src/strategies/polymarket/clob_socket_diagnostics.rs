use std::{io, time::Duration};

use socket2::Socket;
use tokio::net::TcpStream;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ClobSocketPressure {
    pub unread_bytes: u64,
    pub receive_buffer_bytes: u64,
}

impl ClobSocketPressure {
    pub fn utilization(self) -> f64 {
        if self.receive_buffer_bytes == 0 {
            0.0
        } else {
            self.unread_bytes as f64 / self.receive_buffer_bytes as f64
        }
    }
}

#[derive(Debug)]
pub(super) struct ClobSocketDiagnostics {
    socket: Socket,
    receive_buffer_bytes: u64,
}

impl ClobSocketDiagnostics {
    pub fn duplicate(stream: &TcpStream) -> io::Result<Self> {
        let descriptor = rustix::io::fcntl_dupfd_cloexec(stream, 0)?;
        let socket = Socket::from(descriptor);
        let receive_buffer_bytes = u64::try_from(socket.recv_buffer_size()?).unwrap_or(u64::MAX);
        Ok(Self {
            socket,
            receive_buffer_bytes,
        })
    }

    pub fn receive_buffer_bytes(&self) -> u64 {
        self.receive_buffer_bytes
    }

    pub fn sample(&self) -> io::Result<ClobSocketPressure> {
        Ok(ClobSocketPressure {
            unread_bytes: rustix::io::ioctl_fionread(&self.socket)?,
            receive_buffer_bytes: self.receive_buffer_bytes,
        })
    }
}

pub(super) fn probe_schedule_delay(
    scheduled_at: tokio::time::Instant,
    observed_at: tokio::time::Instant,
) -> Duration {
    observed_at.saturating_duration_since(scheduled_at)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;

    #[test]
    fn pressure_reports_receive_buffer_utilization() {
        let pressure = ClobSocketPressure {
            unread_bytes: 256,
            receive_buffer_bytes: 1_024,
        };
        assert_eq!(pressure.utilization(), 0.25);
        assert_eq!(
            ClobSocketPressure {
                unread_bytes: 1,
                receive_buffer_bytes: 0,
            }
            .utilization(),
            0.0
        );
    }

    #[test]
    fn schedule_delay_never_underflows() {
        let scheduled_at = tokio::time::Instant::now();
        assert_eq!(
            probe_schedule_delay(scheduled_at, scheduled_at - Duration::from_millis(1)),
            Duration::ZERO
        );
        assert_eq!(
            probe_schedule_delay(scheduled_at, scheduled_at + Duration::from_millis(7)),
            Duration::from_millis(7)
        );
    }

    #[tokio::test]
    async fn duplicated_socket_reports_unread_kernel_bytes_without_consuming_them() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let client = TcpStream::connect(address).await.unwrap();
        let (mut server, _) = listener.accept().await.unwrap();
        let diagnostics = ClobSocketDiagnostics::duplicate(&client).unwrap();

        server.write_all(b"clob").await.unwrap();
        client.readable().await.unwrap();

        let pressure = diagnostics.sample().unwrap();
        assert_eq!(pressure.unread_bytes, 4);
        assert!(pressure.receive_buffer_bytes >= pressure.unread_bytes);
        assert!(pressure.utilization() > 0.0);
    }
}
