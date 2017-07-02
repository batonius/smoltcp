use Error;
use managed::ManagedSlice;
use socket::dispatch::DispatchTable;
use socket::set::{Set as SocketSet, Item as SocketSetItem, Handle as SocketHandle};
use socket::{TcpSocket, UdpSocket, RawSocket, Socket, AsSocket};
use storage::{RingBuffer};
use wire::{IpVersion, IpProtocol, IpRepr, UdpRepr, TcpRepr};
pub use super::tracker::{SocketTracker, TrackedSocket};

/// A container of sockets with packet dispathing.
///
/// The lifetimes `'b` and `'c` are used when storing a `Socket<'b, 'c>`.
#[derive(Debug)]
pub struct Container<'a, 'b: 'a, 'c: 'a + 'b> {
    set: SocketSet<'a, 'b, 'c>,
    dispatch_table: DispatchTable,
    dirty_sockets: RingBuffer<'a, SocketHandle>,
}

impl<'a, 'b: 'a, 'c: 'a + 'b> Container<'a, 'b, 'c> {
    /// Create a new socket container using the provided storage
    pub fn new<SocketsT, DirtySocketsT>(sockets: SocketsT,
                                        dirty_sockets: DirtySocketsT) -> Container<'a, 'b, 'c>
        where SocketsT: Into<ManagedSlice<'a, Option<SocketSetItem<'b, 'c>>>>,
              DirtySocketsT: Into<ManagedSlice<'a, SocketHandle>>,
    {
        Container {
            set: SocketSet::new(sockets),
            dispatch_table: DispatchTable::new(),
            dirty_sockets: RingBuffer::new_default(dirty_sockets),
        }
    }

    /// Add a socket to the set with the reference count 1, and return its handle.
    ///
    /// # Panics
    /// This function panics if the storage is fixed-size (not a `Vec`) and is full.
    pub fn add(&mut self, socket: Socket<'b, 'c>) -> Result<SocketHandle, Error> {
        let handle = self.set.add(socket);
        while self.set.capacity() > self.dirty_sockets.capacity() {
            self.dirty_sockets.expand_storage();
        }
        self.dispatch_table
            .add_socket(self.set.get(handle), handle)?;
        Ok(handle)
    }

    /// Get a tracked socket from the container by its handle, as mutable.
    ///
    /// # Panics
    /// This function may panic if the handle does not belong to this socket set.
    pub fn get_mut<'d, T>(&'d mut self, handle: SocketHandle) -> Option<SocketTracker<'d, 'a, T>>
        where T: TrackedSocket + 'd, Socket<'b, 'c>: AsSocket<T>
    {
        if let Some(socket) = self.set.get_mut(handle).try_as_socket() {
            Some(SocketTracker::new(&mut self.dispatch_table,
                                    &mut self.dirty_sockets,
                                    handle,
                                    socket))
        } else {
            None
        }
    }

    /// Remove a socket from the container, without changing its state.
    ///
    /// # Panics
    /// This function may panic if the handle does not belong to this socket set.
    pub fn remove(&mut self, handle: SocketHandle) -> Socket<'b, 'c> {
        let mut socket = self.set.remove(handle);
        let res = self.dispatch_table.remove_socket(&socket, handle);
        debug_assert!(res.is_ok());
        if socket.is_on_dirty_list() {
            let res = self.dirty_sockets.remove(&handle);
            socket.set_on_dirty_list(false);
            debug_assert!(res.is_ok());
        }
        socket
    }

    pub(crate) fn get_raw_socket<'d>(&'d mut self, ip_version: IpVersion, ip_protocol: IpProtocol)
                                     -> Option<SocketTracker<'d, 'a, RawSocket<'b, 'c>>> {
        if let Some((raw_socket, handle)) =
            self.dispatch_table.get_raw_socket(&mut self.set, ip_version, ip_protocol)
        {
            Some(SocketTracker::new(&mut self.dispatch_table, &mut self.dirty_sockets,
                                    handle, raw_socket))
        } else {
            None
        }
    }

    pub(crate) fn get_udp_socket<'d>(&'d mut self, ip_repr: &IpRepr, udp_repr: &UdpRepr)
                                     -> Option<SocketTracker<'d, 'a, UdpSocket<'b, 'c>>> {
        if let Some((udp_socket, handle)) =
            self.dispatch_table.get_udp_socket(&mut self.set, ip_repr, udp_repr)
        {
            Some(SocketTracker::new(&mut self.dispatch_table, &mut self.dirty_sockets,
                                    handle, udp_socket))
        } else {
            None
        }
    }

    pub(crate) fn get_tcp_socket<'d>(&'d mut self, ip_repr: &IpRepr, tcp_repr: &TcpRepr)
                                     -> Option<SocketTracker<'d, 'a, TcpSocket<'b>>> {
        if let Some((tcp_socket, handle)) =
            self.dispatch_table.get_tcp_socket(&mut self.set, ip_repr, tcp_repr)
        {
            Some(SocketTracker::new(
                &mut self.dispatch_table,
                &mut self.dirty_sockets,
                handle,
                tcp_socket,
            ))
        } else {
            None
        }
    }

    fn next_dirty<'d>(&'d mut self) -> Option<SocketTracker<'d, 'a, Socket<'b, 'c>>> {
        let handle = {
            match self.dirty_sockets.dequeue() {
                Err(_) => return None,
                Ok(handle) => *handle,
            }
        };
        let socket = self.set.get_mut(handle);
        socket.set_on_dirty_list(false);
        Some(SocketTracker::new(&mut self.dispatch_table, &mut self.dirty_sockets, handle, socket))
    }

    pub(crate) fn dirty_iter<'d>(&'d mut self) -> DirtyIter<'d, 'a, 'b, 'c> {
        let capacity = self.dirty_sockets.capacity();
        DirtyIter::new(self, capacity)
    }
}

// An iterator over dirty sockets with limited iteration count.
// A socket is removed from the `dirty_sockets` list in `next_dirty`, but can be
// added back in the `SocketTracker` destructor if it still has data to send.
// To prevent infinite iteration we limit ourselves to `dirty_sockets.capacity`
// by using `DirtyIter`.
//
// `DirtyIter` can't implement the `Iterator` trait because of non-standard lifetimes
pub struct DirtyIter<'a, 'b: 'a, 'c: 'b, 'd: 'b + 'c> {
    container: &'a mut Container<'b, 'c, 'd>,
    sockets_left: usize,
}

impl <'a, 'b: 'a, 'c: 'b, 'd: 'b + 'c> DirtyIter<'a, 'b, 'c, 'd> {
    pub fn new(container: &'a mut Container<'b, 'c, 'd>, sockets_left: usize)
               -> DirtyIter<'a, 'b, 'c, 'd> {
        DirtyIter {
            container,
            sockets_left,
        }
    }

    pub fn next<'e>(&'e mut self) -> Option<SocketTracker<'e, 'b, Socket<'c, 'd>>> {
        if self.sockets_left == 0 {
            None
        } else {
            self.sockets_left -= 1;
            if let Some(tracker) = self.container.next_dirty() {
                Some(tracker)
            } else {
                None
            }
        }
    }
}

#[cfg(test)]
mod test {
    use socket::*;
    use wire::*;
    #[test]
    fn dispatcher() {
        let eps = [IpEndpoint::new(IpAddress::Unspecified, 12345u16),
                   IpEndpoint::new(IpAddress::Ipv4(Ipv4Address::new(192, 168, 1, 1)), 12345u16),
                   IpEndpoint::new(IpAddress::Ipv4(Ipv4Address::new(192, 168, 1, 2)), 12345u16),
                   IpEndpoint::new(IpAddress::Ipv4(Ipv4Address::new(192, 168, 1, 3)), 12345u16),
                   IpEndpoint::new(IpAddress::Ipv4(Ipv4Address::new(192, 168, 1, 4)), 12345u16),
                   IpEndpoint::new(IpAddress::Ipv4(Ipv4Address::new(192, 168, 1, 5)), 12345u16)];

        let mut sockets = SocketContainer::new(vec![], vec![]);

        let udp_rx_buffer = UdpSocketBuffer::new(vec![UdpPacketBuffer::new(vec![0; 64])]);
        let udp_tx_buffer = UdpSocketBuffer::new(vec![UdpPacketBuffer::new(vec![0; 128])]);
        let udp_socket = UdpSocket::new(udp_rx_buffer, udp_tx_buffer);

        let tcp_rx_buffer = TcpSocketBuffer::new(vec![0; 64]);
        let tcp_tx_buffer = TcpSocketBuffer::new(vec![0; 128]);
        let tcp_socket = TcpSocket::new(tcp_rx_buffer, tcp_tx_buffer);

        let tcp_rx_buffer2 = TcpSocketBuffer::new(vec![0; 64]);
        let tcp_tx_buffer2 = TcpSocketBuffer::new(vec![0; 128]);
        let tcp_socket2 = TcpSocket::new(tcp_rx_buffer2, tcp_tx_buffer2);

        let tcp_rx_buffer3 = TcpSocketBuffer::new(vec![0; 64]);
        let tcp_tx_buffer3 = TcpSocketBuffer::new(vec![0; 128]);
        let tcp_socket3 = TcpSocket::new(tcp_rx_buffer3, tcp_tx_buffer3);

        let udp_handle = sockets.add(udp_socket).unwrap();
        let tcp_handle = sockets.add(tcp_socket).unwrap();
        let tcp_handle2 = sockets.add(tcp_socket2).unwrap();
        let tcp_handle3 = sockets.add(tcp_socket3).unwrap();

        {
            let mut tcp_socket = sockets.get_mut::<TcpSocket>(tcp_handle).unwrap();
            tcp_socket.set_debug_id(101);
            tcp_socket.listen(eps[0]).unwrap();
        }
        {
            let mut tcp_socket2 = sockets.get_mut::<TcpSocket>(tcp_handle2).unwrap();
            tcp_socket2.set_debug_id(102);
            tcp_socket2.listen(eps[2]).unwrap();
        }
        {
            let mut tcp_socket3 = sockets.get_mut::<TcpSocket>(tcp_handle3).unwrap();
            tcp_socket3.set_debug_id(103);
            tcp_socket3.listen(eps[4]).unwrap();
        }
        {
            let mut udp_socket = sockets.get_mut::<UdpSocket>(udp_handle).unwrap();
            udp_socket.set_debug_id(201);
            udp_socket.bind(eps[0]);
        }

        let tcp_payload = vec![];
        let udp_payload = vec![];

        let mut tcp_repr = TcpRepr {
            src_port: 9999u16,
            dst_port: 12345u16,
            control: TcpControl::Syn,
            push: false,
            seq_number: TcpSeqNumber(0),
            ack_number: None,
            window_len: 0u16,
            max_seg_size: None,
            payload: &tcp_payload,
        };

        let mut ipv4_repr = Ipv4Repr {
            src_addr: Ipv4Address::new(192,168, 1, 100),
            dst_addr: Ipv4Address::new(192,168, 1, 1),
            protocol: IpProtocol::Tcp,
            payload_len: 0,
        };

        let udp_repr = UdpRepr {
            src_port: 9999u16,
            dst_port: 12345u16,
            payload: &udp_payload,
        };

        {
            let tracked_socket = sockets.get_udp_socket(&IpRepr::Ipv4(ipv4_repr), &udp_repr)
                .unwrap();
            assert_eq!(tracked_socket.debug_id(), 201);
        }

        for &(ep_index, expected_debug_id) in &[(1, 101), (2, 102), (3, 101), (4, 103), (5, 101)] {
            ipv4_repr.dst_addr = match eps[ep_index].addr {
                IpAddress::Ipv4(a) => a,
                _ => unreachable!(),
            };
            tcp_repr.dst_port = eps[ep_index].port;
            let tracked_socket = sockets.get_tcp_socket(&IpRepr::Ipv4(ipv4_repr), &tcp_repr)
                .unwrap();
            assert_eq!(tracked_socket.debug_id(), expected_debug_id);
        }

        sockets.remove(udp_handle);

        assert!(sockets.get_udp_socket(&IpRepr::Ipv4(ipv4_repr), &udp_repr).is_none());

        ipv4_repr.dst_addr = Ipv4Address::new(192, 168, 1, 2);

        assert_eq!(sockets.get_tcp_socket(&IpRepr::Ipv4(ipv4_repr), &tcp_repr).unwrap().debug_id(),
                   102);

        sockets.remove(tcp_handle2);

        assert_eq!(sockets.get_tcp_socket(&IpRepr::Ipv4(ipv4_repr), &tcp_repr).unwrap().debug_id(),
                   101);
    }
}
