use Error;
use std::collections::{BTreeMap, BTreeSet};
use std::collections::btree_map::Entry as MapEntry;
use socket::{SocketHandle, SocketSet, TcpSocket, UdpSocket, RawSocket, Socket};
use wire::{IpVersion, IpProtocol, IpEndpoint, IpAddress, IpRepr, UdpRepr, TcpRepr};

#[derive(Debug)]
struct TcpLocalEndpoint {
    listen_sockets: BTreeSet<SocketHandle>,
    established_sockets: BTreeMap<IpEndpoint, SocketHandle>,
}

impl TcpLocalEndpoint {
    pub fn new() -> TcpLocalEndpoint {
        TcpLocalEndpoint {
            listen_sockets: BTreeSet::new(),
            established_sockets: BTreeMap::new(),
        }
    }
}

#[derive(Debug)]
pub struct DispatchTable {
    raw: BTreeMap<(IpVersion, IpProtocol), SocketHandle>,
    udp: BTreeMap<IpEndpoint, SocketHandle>,
    tcp: BTreeMap<IpEndpoint, TcpLocalEndpoint>,
}

impl DispatchTable {
    pub fn new() -> DispatchTable {
        DispatchTable {
            raw: BTreeMap::new(),
            tcp: BTreeMap::new(),
            udp: BTreeMap::new(),
        }
    }

    pub fn add_socket(&mut self, socket: &Socket, handle: SocketHandle) -> Result<(), Error> {
        match *socket {
            Socket::Udp(ref udp_socket) => self.add_udp_socket(udp_socket, handle),
            Socket::Tcp(ref tcp_socket) => self.add_tcp_socket(tcp_socket, handle),
            Socket::Raw(ref raw_socket) => self.add_raw_socket(raw_socket, handle),
            _ => unreachable!(),
        }
    }

    pub fn add_udp_socket(
        &mut self,
        udp_socket: &UdpSocket,
        handle: SocketHandle,
    ) -> Result<(), Error> {
        if udp_socket.endpoint() != IpEndpoint::default() {
            match self.udp.entry(udp_socket.endpoint()) {
                MapEntry::Occupied(_) => return Err(Error::AlreadyInUse),
                MapEntry::Vacant(e) => e.insert(handle),
            };
        }
        Ok(())
    }

    pub fn add_raw_socket(
        &mut self,
        raw_socket: &RawSocket,
        handle: SocketHandle,
    ) -> Result<(), Error> {
        let key = (raw_socket.ip_version(), raw_socket.ip_protocol());
        match self.raw.entry(key) {
            MapEntry::Occupied(_) => return Err(Error::AlreadyInUse),
            MapEntry::Vacant(e) => e.insert(handle),
        };
        Ok(())
    }

    pub fn add_tcp_socket(
        &mut self,
        tcp_socket: &TcpSocket,
        handle: SocketHandle,
    ) -> Result<(), Error> {
        if tcp_socket.local_endpoint() == IpEndpoint::default() {
            return Ok(());
        }

        let tcp_endpoint = self.tcp
            .entry(tcp_socket.local_endpoint())
            .or_insert_with(TcpLocalEndpoint::new);

        if tcp_socket.remote_endpoint() == IpEndpoint::default() {
            tcp_endpoint.listen_sockets.insert(handle);
        } else {
            match tcp_endpoint
                .established_sockets
                .entry(tcp_socket.remote_endpoint()) {
                MapEntry::Occupied(_) => return Err(Error::AlreadyInUse),
                MapEntry::Vacant(e) => e.insert(handle),
            };
        }
        Ok(())
    }

    pub fn tcp_socket_established(
        &mut self,
        tcp_socket: &TcpSocket,
        handle: &SocketHandle,
    ) -> Result<(), Error> {
        if tcp_socket.local_endpoint() == IpEndpoint::default() ||
            tcp_socket.remote_endpoint() == IpEndpoint::default()
        {
            return Err(Error::SocketNotFound);
        }

        let mut tcp_endpoint = self.tcp
            .get_mut(&tcp_socket.local_endpoint())
            .ok_or_else(|| Error::SocketNotFound)?;

        if !tcp_endpoint.listen_sockets.remove(handle) {
            return Err(Error::SocketNotFound);
        }

        match tcp_endpoint
            .established_sockets
            .entry(tcp_socket.remote_endpoint()) {
            MapEntry::Occupied(_) => return Err(Error::AlreadyInUse),
            MapEntry::Vacant(e) => e.insert(*handle),
        };

        Ok(())
    }

    pub fn remove_socket(&mut self, socket: &Socket, handle: &SocketHandle) -> Result<(), Error> {
        match *socket {
            Socket::Udp(ref udp_socket) => self.remove_udp_socket(udp_socket, handle),
            Socket::Tcp(ref tcp_socket) => self.remove_tcp_scoket(tcp_socket, handle),
            Socket::Raw(ref raw_socket) => self.remove_raw_socket(raw_socket, handle),
            _ => unreachable!(),
        }
    }

    pub fn remove_udp_socket(
        &mut self,
        udp_socket: &UdpSocket,
        handle: &SocketHandle,
    ) -> Result<(), Error> {
        match self.udp.entry(udp_socket.endpoint()) {
            MapEntry::Occupied(o) => {
                if *o.get() == *handle {
                    o.remove();
                    Ok(())
                } else {
                    Err(Error::SocketNotFound)
                }
            }
            MapEntry::Vacant(_) => Err(Error::SocketNotFound),
        }
    }

    pub fn remove_raw_socket(
        &mut self,
        raw_socket: &RawSocket,
        handle: &SocketHandle,
    ) -> Result<(), Error> {
        let key = (raw_socket.ip_version(), raw_socket.ip_protocol());
        match self.raw.entry(key) {
            MapEntry::Occupied(o) => {
                if *o.get() == *handle {
                    o.remove();
                    Ok(())
                } else {
                    Err(Error::SocketNotFound)
                }
            }
            MapEntry::Vacant(_) => Err(Error::SocketNotFound),
        }
    }

    pub fn remove_tcp_scoket(
        &mut self,
        tcp_socket: &TcpSocket,
        handle: &SocketHandle,
    ) -> Result<(), Error> {
        if tcp_socket.local_endpoint() == IpEndpoint::default() {
            return Err(Error::SocketNotFound);
        }

        let mut tcp_endpoint = self.tcp
            .get_mut(&tcp_socket.local_endpoint())
            .ok_or_else(|| Error::SocketNotFound)?;

        if tcp_socket.remote_endpoint() == IpEndpoint::default() {
            if tcp_endpoint.listen_sockets.remove(handle) {
                Ok(())
            } else {
                Err(Error::SocketNotFound)
            }
        } else {
            match tcp_endpoint
                .established_sockets
                .entry(tcp_socket.remote_endpoint()) {
                MapEntry::Occupied(o) => {
                    if *o.get() == *handle {
                        o.remove();
                        Ok(())
                    } else {
                        Err(Error::SocketNotFound)
                    }
                }
                MapEntry::Vacant(_) => Err(Error::SocketNotFound),
            }
        }
    }

    pub fn get_raw_sockets<'a, 'b: 'a, 'c: 'a + 'b, 'd>(
        &self,
        set: &'d mut SocketSet<'a, 'b, 'c>,
        ip_version: IpVersion,
        ip_protocol: IpProtocol,
    ) -> Iter<'d, 'b, 'c> {
        let key = (ip_version, ip_protocol);
        Iter::new(self.raw.get(&key).map(move |handle| set.get_mut(*handle)))
    }

    pub fn get_udp_sockets<'a, 'b: 'a, 'c: 'a + 'b, 'd>(
        &self,
        set: &'d mut SocketSet<'a, 'b, 'c>,
        ip_repr: &IpRepr,
        udp_repr: &UdpRepr,
    ) -> Iter<'d, 'b, 'c> {
        Iter::new(
            DispatchTable::get_l3_socket(
                &self.udp,
                IpEndpoint::new(ip_repr.dst_addr(), udp_repr.dst_port),
            ).map(move |handle| set.get_mut(*handle)),
        )
    }

    pub fn get_tcp_sockets<'a, 'b: 'a, 'c: 'a + 'b, 'd>(
        &self,
        set: &'d mut SocketSet<'a, 'b, 'c>,
        ip_repr: &IpRepr,
        tcp_repr: &TcpRepr,
    ) -> Iter<'d, 'b, 'c> {
        Iter::new(
            DispatchTable::get_l3_socket(
                &self.tcp,
                IpEndpoint::new(ip_repr.dst_addr(), tcp_repr.dst_port),
            ).and_then(|tcp_endpoint| {
                tcp_endpoint
                    .established_sockets
                    .get(&IpEndpoint::new(ip_repr.src_addr(), tcp_repr.src_port))
                    .or_else(|| tcp_endpoint.listen_sockets.iter().next())
            })
                .map(move |handle| set.get_mut(*handle)),
        )
    }

    fn get_l3_socket<T>(tree: &BTreeMap<IpEndpoint, T>, endpoint: IpEndpoint) -> Option<&T> {
        use std::collections::Bound::Included;
        let mut unspecified_endpoint = endpoint;
        unspecified_endpoint.addr = IpAddress::Unspecified;
        let mut range = tree.range((Included(unspecified_endpoint), Included(endpoint)));
        match range.next_back() {
            None => None,
            Some((&IpEndpoint { ref addr, .. }, h))
                if *addr == endpoint.addr || addr.is_unspecified() => Some(h),
            _ => {
                match range.next() {
                    Some((e, h)) if e.is_unspecified() => Some(h),
                    _ => None,
                }
            }
        }
    }
}

pub struct Iter<'a, 'b: 'a, 'c: 'a + 'b> {
    socket: Option<&'a mut Socket<'b, 'c>>,
}

impl<'a, 'b: 'a, 'c: 'a + 'b> Iter<'a, 'b, 'c> {
    pub fn new(socket: Option<&'a mut Socket<'b, 'c>>) -> Iter<'a, 'b, 'c> {
        Iter { socket }
    }
}

impl<'a, 'b: 'a, 'c: 'a + 'b> Iterator for Iter<'a, 'b, 'c> {
    type Item = &'a mut Socket<'b, 'c>;

    fn next(&mut self) -> Option<&'a mut Socket<'b, 'c>> {
        self.socket.take()
    }
}

#[cfg(test)]
mod test {
    use socket::*;
    use wire::*;
    use super::*;

    #[test]
    fn dispater() {
        let mut sockets = SocketSet::new(vec![]);

        let tcp_rx_buffer = TcpSocketBuffer::new(vec![0; 64]);
        let tcp_tx_buffer = TcpSocketBuffer::new(vec![0; 128]);
        let tcp_socket = TcpSocket::new(tcp_rx_buffer, tcp_tx_buffer);

        let tcp_rx_buffer2 = TcpSocketBuffer::new(vec![0; 64]);
        let tcp_tx_buffer2 = TcpSocketBuffer::new(vec![0; 128]);
        let tcp_socket2 = TcpSocket::new(tcp_rx_buffer2, tcp_tx_buffer2);

        let tcp_handle = sockets.add(tcp_socket);
        let tcp_handle2 = sockets.add(tcp_socket2);

        let ep_u = IpEndpoint::new(IpAddress::Unspecified, 12345u16);
        let ep_1 = IpEndpoint::new(IpAddress::Ipv4(Ipv4Address::new(192, 168, 1, 1)), 12345u16);
        let ep_2 = IpEndpoint::new(IpAddress::Ipv4(Ipv4Address::new(192, 168, 1, 2)), 12345u16);
        let ep_3 = IpEndpoint::new(IpAddress::Ipv4(Ipv4Address::new(192, 168, 1, 3)), 12345u16);

        let mut dispatcher = DispatchTable::new();
        {
            let tcp_socket: &mut TcpSocket = sockets.get_mut(tcp_handle).as_socket();
            tcp_socket.set_debug_id(1);
            assert_eq!(tcp_socket.listen(ep_u), Ok(()));
        }
        assert_eq!(
            dispatcher.add_socket(&sockets.get(tcp_handle), tcp_handle),
            Ok(())
        );
        {
            let tcp_socket2: &mut TcpSocket = sockets.get_mut(tcp_handle2).as_socket();
            tcp_socket2.set_debug_id(2);
            assert_eq!(tcp_socket2.listen(ep_2), Ok(()));
        }
        assert_eq!(
            dispatcher.add_socket(&sockets.get(tcp_handle2), tcp_handle2),
            Ok(())
        );

        // assert_eq!(
        //     dispatcher
        //         .get_tcp_sockets(&mut sockets, ep_1.addr, ep_1.port)
        //         .map(|s| s.debug_id())
        //         .collect::<Vec<_>>(),
        //     vec![1]
        // );
        // assert_eq!(
        //     dispatcher
        //         .get_tcp_sockets(&mut sockets, ep_2.addr, ep_2.port)
        //         .map(|s| s.debug_id())
        //         .collect::<Vec<_>>(),
        //     vec![2]
        // );
        // assert_eq!(
        //     dispatcher
        //         .get_tcp_sockets(&mut sockets, ep_3.addr, ep_3.port)
        //         .map(|s| s.debug_id())
        //         .collect::<Vec<_>>(),
        //     vec![1]
        // );
    }
}
