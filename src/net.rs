use std::collections::HashSet;
use std::io;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream, UdpSocket};
use std::thread;
use std::time::{Duration, Instant};

pub fn magic_packet(mac: &[u8; 6]) -> [u8; 102] {
    let mut packet = [0xff; 102];
    for i in 0..16 {
        packet[6 + i * 6..12 + i * 6].copy_from_slice(mac);
    }
    packet
}

/// Sends the packet a few times: it is a single unacknowledged UDP datagram.
pub fn send_magic_packet(mac: &[u8; 6], dest: SocketAddrV4) -> io::Result<()> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
    socket.set_broadcast(true)?;
    let packet = magic_packet(mac);
    for attempt in 0..3 {
        if attempt > 0 {
            thread::sleep(Duration::from_millis(50));
        }
        let sent = socket.send_to(&packet, dest)?;
        if sent != packet.len() {
            return Err(io::Error::other(format!(
                "short send: {sent} of {} bytes",
                packet.len()
            )));
        }
    }
    Ok(())
}

pub fn probe(addr: SocketAddrV4, timeout: Duration) -> io::Result<()> {
    TcpStream::connect_timeout(&SocketAddr::V4(addr), timeout).map(drop)
}

/// Polls `addr` until it accepts a TCP connection or `timeout` elapses.
pub fn wait_until_up(addr: SocketAddrV4, timeout: Duration) -> Result<Duration, String> {
    let start = Instant::now();
    let mut last_error = String::from("no answer");
    loop {
        let remaining = timeout.saturating_sub(start.elapsed());
        if remaining.is_zero() {
            return Err(last_error);
        }
        let attempt = Instant::now();
        match probe(addr, remaining.min(Duration::from_secs(1))) {
            Ok(()) => return Ok(start.elapsed()),
            Err(e) => last_error = e.to_string(),
        }
        // Refused/unreachable errors return at once; don't spin on them.
        let pause = Duration::from_millis(500).saturating_sub(attempt.elapsed());
        thread::sleep(pause.min(timeout.saturating_sub(start.elapsed())));
    }
}

pub fn local_macs() -> HashSet<[u8; 6]> {
    mac_address::MacAddressIterator::new()
        .map(|it| it.map(|m| m.bytes()).filter(|m| *m != [0; 6]).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn packet_is_sync_stream_then_sixteen_macs() {
        let mac = [1, 2, 3, 4, 5, 6];
        let packet = magic_packet(&mac);
        assert_eq!(&packet[..6], &[0xff; 6]);
        assert!(packet[6..].chunks(6).all(|c| c == mac));
    }

    #[test]
    fn sends_packets_over_loopback() {
        let receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
        receiver
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let SocketAddr::V4(addr) = receiver.local_addr().unwrap() else {
            unreachable!()
        };
        let mac = [0x02, 0x11, 0x22, 0x33, 0x44, 0x55];
        send_magic_packet(&mac, addr).unwrap();
        let mut buf = [0u8; 200];
        let (len, _) = receiver.recv_from(&mut buf).unwrap();
        assert_eq!(&buf[..len], &magic_packet(&mac));
    }

    #[test]
    fn wait_reports_listening_port_and_times_out_on_closed_one() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let SocketAddr::V4(open) = listener.local_addr().unwrap() else {
            unreachable!()
        };
        assert!(wait_until_up(open, Duration::from_secs(2)).is_ok());
        drop(listener);
        let start = Instant::now();
        assert!(wait_until_up(open, Duration::from_millis(700)).is_err());
        assert!(start.elapsed() < Duration::from_secs(2));
    }
}
