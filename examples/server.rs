#[macro_use]
extern crate log;
extern crate env_logger;
extern crate getopts;
extern crate smoltcp;

mod utils;

use std::str;
use std::time::Instant;
use smoltcp::Error;
use smoltcp::wire::{EthernetAddress, IpAddress};
use smoltcp::iface::{ArpCache, SliceArpCache, EthernetInterface};
use smoltcp::socket::{SocketContainer};
use smoltcp::socket::{UdpSocket, UdpSocketBuffer, UdpPacketBuffer};
use smoltcp::socket::{TcpSocket, TcpSocketBuffer};

fn main() {
    utils::setup_logging("");

    let (mut opts, mut free) = utils::create_options();
    utils::add_tap_options(&mut opts, &mut free);
    utils::add_middleware_options(&mut opts, &mut free);

    let mut matches = utils::parse_options(&opts, free);
    let device = utils::parse_tap_options(&mut matches);
    let device = utils::parse_middleware_options(&mut matches, device, /*loopback=*/false);

    let startup_time = Instant::now();

    let arp_cache = SliceArpCache::new(vec![Default::default(); 8]);

    let udp_rx_buffer = UdpSocketBuffer::new(vec![UdpPacketBuffer::new(vec![0; 64])]);
    let udp_tx_buffer = UdpSocketBuffer::new(vec![UdpPacketBuffer::new(vec![0; 128])]);
    let udp_socket = UdpSocket::new(udp_rx_buffer, udp_tx_buffer);

    let tcp1_rx_buffer = TcpSocketBuffer::new(vec![0; 64]);
    let tcp1_tx_buffer = TcpSocketBuffer::new(vec![0; 128]);
    let tcp1_socket = TcpSocket::new(tcp1_rx_buffer, tcp1_tx_buffer);

    let tcp2_rx_buffer = TcpSocketBuffer::new(vec![0; 64]);
    let tcp2_tx_buffer = TcpSocketBuffer::new(vec![0; 128]);
    let tcp2_socket = TcpSocket::new(tcp2_rx_buffer, tcp2_tx_buffer);

    let tcp3_rx_buffer = TcpSocketBuffer::new(vec![0; 65535]);
    let tcp3_tx_buffer = TcpSocketBuffer::new(vec![0; 65535]);
    let tcp3_socket = TcpSocket::new(tcp3_rx_buffer, tcp3_tx_buffer);

    let hardware_addr  = EthernetAddress([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);
    let protocol_addrs = [IpAddress::v4(192, 168, 69, 1)];
    let mut iface      = EthernetInterface::new(
        Box::new(device), Box::new(arp_cache) as Box<ArpCache>,
        hardware_addr, protocol_addrs);

    let mut sockets = SocketContainer::new(vec![]);
    let udp_handle  = sockets.add(udp_socket).unwrap();
    let tcp1_handle = sockets.add(tcp1_socket).unwrap();
    let tcp2_handle = sockets.add(tcp2_socket).unwrap();
    let tcp3_handle = sockets.add(tcp3_socket).unwrap();

    let mut tcp_6970_active = false;
    loop {
        // udp:6969: respond "yo dawg"
        {
            let mut socket = sockets.get_mut::<UdpSocket>(udp_handle).unwrap();
            if socket.endpoint().is_unspecified() {
                socket.bind(6969)
            }

            let client = match socket.recv() {
                Ok((data, endpoint)) => {
                    debug!("udp:6969 recv data: {:?} from {}",
                           str::from_utf8(data.as_ref()).unwrap(), endpoint);
                    Some(endpoint)
                }
                Err(_) => None
            };
            if let Some(endpoint) = client {
                let data = b"yo dawg\n";
                debug!("udp:6969 send data: {:?}",
                       str::from_utf8(data.as_ref()).unwrap());
                socket.send_slice(data, endpoint).unwrap();
            }
        }

        // tcp:6969: respond "yo dawg"
        {
            let mut socket = sockets.get_mut::<TcpSocket>(tcp1_handle).unwrap();
            if !socket.is_open() {
                socket.listen(6969).unwrap();
            }

            if socket.can_send() {
                let data = b"yo dawg\n";
                debug!("tcp:6969 send data: {:?}",
                       str::from_utf8(data.as_ref()).unwrap());
                socket.send_slice(data).unwrap();
                debug!("tcp:6969 close");
                socket.close();
            }
        }

        // tcp:6970: echo with reverse
        {
            let mut socket = sockets.get_mut::<TcpSocket>(tcp2_handle).unwrap();
            if !socket.is_open() {
                socket.listen(6970).unwrap()
            }

            if socket.is_active() && !tcp_6970_active {
                debug!("tcp:6970 connected");
            } else if !socket.is_active() && tcp_6970_active {
                debug!("tcp:6970 disconnected");
            }
            tcp_6970_active = socket.is_active();

            if socket.may_recv() {
                let data = {
                    let mut data = socket.recv(128).unwrap().to_owned();
                    if data.len() > 0 {
                        debug!("tcp:6970 recv data: {:?}",
                               str::from_utf8(data.as_ref()).unwrap_or("(invalid utf8)"));
                        data = data.split(|&b| b == b'\n').collect::<Vec<_>>().concat();
                        data.reverse();
                        data.extend(b"\n");
                    }
                    data
                };
                if socket.can_send() && data.len() > 0 {
                    debug!("tcp:6970 send data: {:?}",
                           str::from_utf8(data.as_ref()).unwrap_or("(invalid utf8)"));
                    socket.send_slice(&data[..]).unwrap();
                }
            } else if socket.may_send() {
                debug!("tcp:6970 close");
                socket.close();
            }
        }

        // tcp:6971: sinkhole
        {
            let mut socket = sockets.get_mut::<TcpSocket>(tcp3_handle).unwrap();
            if !socket.is_open() {
                socket.listen(6971).unwrap()
            }

            if socket.may_recv() {
                if let Ok(data) = socket.recv(65535) {
                    if data.len() > 0 {
                        debug!("tcp:6971 recv {:?} octets", data.len());
                    }
                }
            } else if socket.may_send() {
                socket.close();
            }
        }

        let timestamp = Instant::now().duration_since(startup_time);
        let timestamp_ms = (timestamp.as_secs() * 1000) +
                           (timestamp.subsec_nanos() / 1000000) as u64;
        match iface.poll(&mut sockets, timestamp_ms) {
            Ok(()) | Err(Error::Exhausted) => (),
            Err(e) => debug!("poll error: {}", e)
        }
    }
}
