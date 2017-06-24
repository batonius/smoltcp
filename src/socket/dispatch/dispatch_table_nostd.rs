use Error;
use socket::{SocketSet, Socket, SocketSetIterMut, SocketHandle};
use wire::{IpVersion, IpProtocol, IpAddress};

pub type Iter<'a, 'b: 'a, 'c: 'a + 'b> = SocketSetIterMut<'a, 'b, 'c>;

#[derive(Debug)]
pub struct DispatchTable {}

impl DispatchTable {
    pub fn new() -> DispatchTable {
        DispatchTable {}
    }

    pub fn add_socket(&mut self, _socket: &Socket, _handle: SocketHandle) -> Result<(), Error> {
        Ok(())
    }

    pub fn remove_socket(&mut self, _socket: &Socket) -> Result<(), Error> {
        Ok(())
    }

    pub fn get_raw_sockets<'a, 'b: 'a, 'c: 'a + 'b, 'd>(
        &self,
        set: &'d mut SocketSet<'a, 'b, 'c>,
        _ip_version: IpVersion,
        _ip_protocol: IpProtocol,
    ) -> Iter<'d, 'b, 'c> {
        set.iter_mut()
    }

    pub fn get_tcp_sockets<'a, 'b: 'a, 'c: 'a + 'b, 'd>(
        &self,
        set: &'d mut SocketSet<'a, 'b, 'c>,
        _src_ip: IpAddress,
        _src_port: u16,
    ) -> Iter<'d, 'b, 'c> {
        set.iter_mut()
    }

    pub fn get_udp_sockets<'a, 'b: 'a, 'c: 'a + 'b, 'd>(
        &self,
        set: &'d mut SocketSet<'a, 'b, 'c>,
        _src_ip: IpAddress,
        _src_port: u16,
    ) -> Iter<'d, 'b, 'c> {
        set.iter_mut()
    }
}
