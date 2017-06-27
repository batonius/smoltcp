use Error;

use managed::ManagedSlice;
use super::set::{Set as SocketSet, Item as SocketSetItem, Handle as SocketHandle};
use super::dispatch::{DispatchTable, Iter as DispatchTableIterMut};
use socket::{TcpSocket, UdpSocket, RawSocket, Socket, AsSocket};
use std::ops::{Deref, DerefMut};
use wire::{IpVersion, IpProtocol};
use std::cell::RefCell;

#[derive(Debug)]
pub struct Container<'a, 'b: 'a, 'c: 'a + 'b> {
    set: SocketSet<'a, 'b, 'c>,
    dispatch_table: RefCell<DispatchTable>,
}

impl<'a, 'b: 'a, 'c: 'a + 'b> Container<'a, 'b, 'c> {
    pub fn new<SocketsT>(sockets: SocketsT) -> Container<'a, 'b, 'c>
    where
        SocketsT: Into<ManagedSlice<'a, Option<SocketSetItem<'b, 'c>>>>,
    {
        Container {
            set: SocketSet::new(sockets),
            dispatch_table: RefCell::new(DispatchTable::new()),
        }
    }

    pub fn add(&mut self, socket: Socket<'b, 'c>) -> Result<SocketHandle, Error> {
        let handle = self.set.add(socket);
        self.dispatch_table
            .borrow_mut()
            .add_socket(self.set.get(handle), handle)?;
        Ok(handle)
    }

    pub fn get_raw_mut<'d>(&'d mut self, handle: SocketHandle) -> Option<RawTracker<'d, 'b, 'c>> {
        if let Some(raw_socket) = self.set.get_mut(handle).try_as_socket() {
            Some(RawTracker {
                dispatch_table: &self.dispatch_table,
                raw_socket,
                handle,
            })
        } else {
            None
        }
    }

    pub fn remove(&mut self, handle: SocketHandle) -> Socket<'b, 'c> {
        let socket = self.set.remove(handle);
        let _ = self.dispatch_table
            .borrow_mut()
            .remove_socket(&socket, &handle);
        socket
    }

    pub fn get_raw_sockets<'d>(
        &'d mut self,
        ip_version: IpVersion,
        ip_protocol: IpProtocol,
    ) -> Iter<'d, RawSocket<'b, 'c>> {
        Iter {
            inner: self.dispatch_table
                .borrow()
                .get_raw_sockets(&mut self.set, ip_version, ip_protocol),
            dispatch_table: &self.dispatch_table,
        }
    }
}

// Waiting for impl Trait
pub struct Iter<'a, T: 'a> {
    inner: DispatchTableIterMut<'a, T>,
    dispatch_table: &'a RefCell<DispatchTable>,
}

impl<'a, T: 'a> Iterator for Iter<'a, T>
where
    T: TrackedSocket<'a>,
{
    type Item = <T as TrackedSocket<'a>>::Tracker;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(move |(socket, handle)| {
            <T as TrackedSocket<'a>>::new_tracker(socket, self.dispatch_table, handle)
        })
    }
}

trait TrackedSocket<'a> {
    type Tracker;

    fn new_tracker(&'a mut Self, &'a RefCell<DispatchTable>, SocketHandle) -> Self::Tracker;
}

pub struct RawTracker<'a, 'b: 'a, 'c: 'a + 'b> {
    handle: SocketHandle,
    raw_socket: &'a mut RawSocket<'b, 'c>,
    dispatch_table: &'a RefCell<DispatchTable>,
}

impl<'a, 'b: 'a, 'c: 'a + 'b> TrackedSocket<'a> for RawSocket<'b, 'c> {
    type Tracker = RawTracker<'a, 'b, 'c>;
    fn new_tracker(
        raw_socket: &'a mut Self,
        dispatch_table: &'a RefCell<DispatchTable>,
        handle: SocketHandle,
    ) -> Self::Tracker {
        RawTracker {
            raw_socket,
            dispatch_table,
            handle,
        }
    }
}

impl<'a, 'b: 'a, 'c: 'a + 'b> Deref for RawTracker<'a, 'b, 'c> {
    type Target = RawSocket<'b, 'c>;

    fn deref(&self) -> &Self::Target {
        self.raw_socket
    }
}

impl<'a, 'b: 'a, 'c: 'a + 'b> DerefMut for RawTracker<'a, 'b, 'c> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.raw_socket
    }
}
