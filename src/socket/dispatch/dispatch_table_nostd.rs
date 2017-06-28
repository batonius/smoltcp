use Error;
use socket::{SocketHandle, SocketSet, TcpSocket, UdpSocket, RawSocket, Socket, AsSocket};
use wire::{IpVersion, IpProtocol, IpRepr, UdpRepr, TcpRepr};

pub type WithHandle<'a, T> = Option<(&'a mut T, SocketHandle)>;

#[derive(Debug)]
pub struct DispatchTable {}

impl DispatchTable {
    pub fn new() -> DispatchTable {
        DispatchTable {}
    }

    pub fn add_socket(&mut self, _socket: &Socket, _handle: SocketHandle) -> Result<(), Error> {
        Ok(())
    }

    pub fn add_udp_socket(
        &mut self,
        _udp_socket: &UdpSocket,
        _handle: SocketHandle,
    ) -> Result<(), Error> {
        Ok(())
    }

    pub fn add_raw_socket(
        &mut self,
        _raw_socket: &RawSocket,
        _handle: SocketHandle,
    ) -> Result<(), Error> {
        Ok(())
    }

    pub fn add_tcp_socket(
        &mut self,
        _tcp_socket: &TcpSocket,
        _handle: SocketHandle,
    ) -> Result<(), Error> {
        Ok(())
    }

    pub fn remove_socket(&mut self, _socket: &Socket, _handle: SocketHandle) -> Result<(), Error> {
        Ok(())
    }

    pub fn remove_udp_socket(&mut self, _handle: SocketHandle) -> Result<(), Error> {
        Ok(())
    }

    pub fn remove_raw_socket(&mut self, _handle: SocketHandle) -> Result<(), Error> {
        Ok(())
    }

    pub fn remove_tcp_socket(&mut self, _handle: SocketHandle) -> Result<(), Error> {
        Ok(())
    }

    pub fn get_udp_socket<'a, 'b: 'a, 'c: 'a + 'b, 'd>(
        &self,
        set: &'d mut SocketSet<'a, 'b, 'c>,
        ip_repr: &IpRepr,
        udp_repr: &UdpRepr,
    ) -> WithHandle<'d, UdpSocket<'b, 'c>> {
        for (socket, handle) in set.iter_mut_with_handle() {
            if let Some(udp_socket) = <Socket as AsSocket<UdpSocket>>::try_as_socket(socket) {
                if udp_socket.would_accept(ip_repr, udp_repr) {
                    return Some((udp_socket, handle));
                }
            }
        }
        None
    }

    pub fn get_raw_socket<'a, 'b: 'a, 'c: 'a + 'b, 'd>(
        &self,
        set: &'d mut SocketSet<'a, 'b, 'c>,
        ip_version: IpVersion,
        ip_protocol: IpProtocol,
    ) -> WithHandle<'d, RawSocket<'b, 'c>> {
        for (socket, handle) in set.iter_mut_with_handle() {
            if let Some(raw_socket) = <Socket as AsSocket<RawSocket>>::try_as_socket(socket) {
                if raw_socket.would_accept(ip_version, ip_protocol) {
                    return Some((raw_socket, handle));
                }
            }
        }
        None
    }

    pub fn get_tcp_socket<'a, 'b: 'a, 'c: 'a + 'b, 'd>(
        &self,
        set: &'d mut SocketSet<'a, 'b, 'c>,
        ip_repr: &IpRepr,
        tcp_repr: &TcpRepr,
    ) -> WithHandle<'d, TcpSocket<'b>> {
        for (socket, handle) in set.iter_mut_with_handle() {
            if let Some(tcp_socket) = <Socket as AsSocket<TcpSocket>>::try_as_socket(socket) {
                if tcp_socket.would_accept(ip_repr, tcp_repr) {
                    return Some((tcp_socket, handle));
                }
            }
        }
        None
    }
}
