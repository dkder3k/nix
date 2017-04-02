//! Socket interface functions
//!
//! [Further reading](http://man7.org/linux/man-pages/man7/socket.7.html)
use {Error, Errno, Result};
use features;
use libc::{self, c_void, c_int, socklen_t, size_t, pid_t, uid_t, gid_t};
use std::{mem, ptr, slice};
use std::os::unix::io::RawFd;
use sys::time::TimeVal;
use sys::uio::IoVec;

mod addr;
mod ffi;
mod multicast;
pub mod sockopt;

/*
 *
 * ===== Re-exports =====
 *
 */

pub use self::addr::{
    AddressFamily,
    SockAddr,
    InetAddr,
    UnixAddr,
    IpAddr,
    Ipv4Addr,
    Ipv6Addr,
};
#[cfg(any(target_os = "linux", target_os = "android"))]
pub use ::sys::socket::addr::netlink::NetlinkAddr;

pub use libc::{
    in_addr,
    in6_addr,
    sockaddr,
    sockaddr_in,
    sockaddr_in6,
    sockaddr_un,
    sa_family_t,
};

pub use self::multicast::{
    ip_mreq,
    ipv6_mreq,
};

pub use libc::sockaddr_storage;

/// These constants are used to specify the communication semantics
/// when creating a socket with [`socket()`](fn.socket.html)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(i32)]
pub enum SockType {
    /// Provides sequenced, reliable, two-way, connection-
    /// based byte streams.  An out-of-band data transmission
    /// mechanism may be supported.
    Stream = libc::SOCK_STREAM,
    /// Supports datagrams (connectionless, unreliable
    /// messages of a fixed maximum length).
    Datagram = libc::SOCK_DGRAM,
    /// Provides a sequenced, reliable, two-way connection-
    /// based data transmission path for datagrams of fixed
    /// maximum length; a consumer is required to read an
    /// entire packet with each input system call.
    SeqPacket = libc::SOCK_SEQPACKET,
    /// Provides raw network protocol access.
    Raw = libc::SOCK_RAW,
    /// Provides a reliable datagram layer that does not
    /// guarantee ordering.
    Rdm = libc::SOCK_RDM,
}

/// Constants used in [`socket`](fn.socket.html) and [`socketpair`](fn.socketpair.html)
/// to specify the protocol to use.
#[repr(i32)]
pub enum SockProtocol {
    /// TCP protocol ([ip(7)](http://man7.org/linux/man-pages/man7/ip.7.html))
    Tcp = libc::IPPROTO_TCP,
    /// UDP protocol ([ip(7)](http://man7.org/linux/man-pages/man7/ip.7.html))
    Udp = libc::IPPROTO_UDP,
    /// Allows applications and other KEXTs to be notified when certain kernel events occur
    /// ([ref](https://developer.apple.com/library/content/documentation/Darwin/Conceptual/NKEConceptual/control/control.html))
    #[cfg(any(target_os = "ios", target_os = "macos"))]
    KextEvent = libc::SYSPROTO_EVENT,
    /// Allows applications to configure and control a KEXT
    /// ([ref](https://developer.apple.com/library/content/documentation/Darwin/Conceptual/NKEConceptual/control/control.html))
    #[cfg(any(target_os = "ios", target_os = "macos"))]
    KextControl = libc::SYSPROTO_CONTROL,
}

libc_bitflags!{
    /// Additional socket options
    pub struct SockFlag: c_int {
        /// Set non-blocking mode on the new socket
        #[cfg(any(target_os = "android",
                  target_os = "dragonfly",
                  target_os = "freebsd",
                  target_os = "linux",
                  target_os = "netbsd",
                  target_os = "openbsd"))]
        SOCK_NONBLOCK;
        /// Set close-on-exec on the new descriptor
        #[cfg(any(target_os = "android",
                  target_os = "dragonfly",
                  target_os = "freebsd",
                  target_os = "linux",
                  target_os = "netbsd",
                  target_os = "openbsd"))]
        SOCK_CLOEXEC;
        /// Return `EPIPE` instead of raising `SIGPIPE`
        #[cfg(target_os = "netbsd")]
        SOCK_NOSIGPIPE;
        /// For domains `AF_INET(6)`, only allow `connect(2)`, `sendto(2)`, or `sendmsg(2)`
        /// to the DNS port (typically 53)
        #[cfg(target_os = "openbsd")]
        SOCK_DNS;
    }
}

libc_bitflags!{
    /// Flags for send/recv and their relatives
    pub struct MsgFlags: libc::c_int {
        /// Sends or requests out-of-band data on sockets that support this notion
        /// (e.g., of type [`Stream`](enum.SockType.html)); the underlying protocol must also
        /// support out-of-band data.
        MSG_OOB;
        /// Peeks at an incoming message. The data is treated as unread and the next
        /// [`recv()`](fn.recv.html)
        /// or similar function shall still return this data.
        MSG_PEEK;
        /// Enables nonblocking operation; if the operation would block,
        /// `EAGAIN` or `EWOULDBLOCK` is returned.  This provides similar
        /// behavior to setting the `O_NONBLOCK` flag
        /// (via the [`fcntl`](../../fcntl/fn.fcntl.html)
        /// `F_SETFL` operation), but differs in that `MSG_DONTWAIT` is a per-
        /// call option, whereas `O_NONBLOCK` is a setting on the open file
        /// description (see [open(2)](http://man7.org/linux/man-pages/man2/open.2.html)),
        /// which will affect all threads in
        /// the calling process and as well as other processes that hold
        /// file descriptors referring to the same open file description.
        MSG_DONTWAIT;
        /// Receive flags: Control Data was discarded (buffer too small)
        MSG_CTRUNC;
        /// For raw ([`Packet`](addr/enum.AddressFamily.html)), Internet datagram
        /// (since Linux 2.4.27/2.6.8),
        /// netlink (since Linux 2.6.22) and UNIX datagram (since Linux 3.4)
        /// sockets: return the real length of the packet or datagram, even
        /// when it was longer than the passed buffer. Not implemented for UNIX
        /// domain ([unix(7)](https://linux.die.net/man/7/unix)) sockets.
        ///
        /// For use with Internet stream sockets, see [tcp(7)](https://linux.die.net/man/7/tcp).
        MSG_TRUNC;
        /// Terminates a record (when this notion is supported, as for
        /// sockets of type [`SeqPacket`](enum.SockType.html)).
        MSG_EOR;
        /// This flag specifies that queued errors should be received from
        /// the socket error queue. (For more details, see
        /// [recvfrom(2)](https://linux.die.net/man/2/recvfrom))
        #[cfg(any(target_os = "linux", target_os = "android"))]
        MSG_ERRQUEUE;
        /// Set the `close-on-exec` flag for the file descriptor received via a UNIX domain
        /// file descriptor using the `SCM_RIGHTS` operation (described in
        /// [unix(7)](https://linux.die.net/man/7/unix)).
        /// This flag is useful for the same reasons as the `O_CLOEXEC` flag of
        /// [open(2)](https://linux.die.net/man/2/open).
        ///
        /// Only used in [`recvmsg`](fn.recvmsg.html) function.
        #[cfg(any(target_os = "linux", target_os = "android"))]
        MSG_CMSG_CLOEXEC;
    }
}

/// Copy the in-memory representation of src into the byte slice dst,
/// updating the slice to point to the remainder of dst only. Unsafe
/// because it exposes all bytes in src, which may be UB if some of them
/// are uninitialized (including padding).
unsafe fn copy_bytes<'a, 'b, T: ?Sized>(src: &T, dst: &'a mut &'b mut [u8]) {
    let srclen = mem::size_of_val(src);
    let mut tmpdst = &mut [][..];
    mem::swap(&mut tmpdst, dst);
    let (target, mut remainder) = tmpdst.split_at_mut(srclen);
    // Safe because the mutable borrow of dst guarantees that src does not alias it.
    ptr::copy_nonoverlapping(src as *const T as *const u8, target.as_mut_ptr(), srclen);
    mem::swap(dst, &mut remainder);
}


use self::ffi::{cmsghdr, msghdr, type_of_cmsg_data, type_of_msg_iovlen, type_of_cmsg_len};

/// A structure used to make room in a cmsghdr passed to recvmsg. The
/// size and alignment match that of a cmsghdr followed by a T, but the
/// fields are not accessible, as the actual types will change on a call
/// to recvmsg.
///
/// To make room for multiple messages, nest the type parameter with
/// tuples, e.g.
/// `let cmsg: CmsgSpace<([RawFd; 3], CmsgSpace<[RawFd; 2]>)> = CmsgSpace::new();`
pub struct CmsgSpace<T> {
    _hdr: cmsghdr,
    _data: T,
}

impl<T> CmsgSpace<T> {
    /// Create a CmsgSpace<T>. The structure is used only for space, so
    /// the fields are uninitialized.
    pub fn new() -> Self {
        // Safe because the fields themselves aren't accessible.
        unsafe { mem::uninitialized() }
    }
}

pub struct RecvMsg<'a> {
    // The number of bytes received.
    pub bytes: usize,
    cmsg_buffer: &'a [u8],
    pub address: Option<SockAddr>,
    pub flags: MsgFlags,
}

impl<'a> RecvMsg<'a> {
    /// Iterate over the valid control messages pointed to by this
    /// msghdr.
    pub fn cmsgs(&self) -> CmsgIterator {
        CmsgIterator {
            buf: self.cmsg_buffer,
            next: 0
        }
    }
}

pub struct CmsgIterator<'a> {
    buf: &'a [u8],
    next: usize,
}

impl<'a> Iterator for CmsgIterator<'a> {
    type Item = ControlMessage<'a>;

    // The implementation loosely follows CMSG_FIRSTHDR / CMSG_NXTHDR,
    // although we handle the invariants in slightly different places to
    // get a better iterator interface.
    fn next(&mut self) -> Option<ControlMessage<'a>> {
        let sizeof_cmsghdr = mem::size_of::<cmsghdr>();
        if self.buf.len() < sizeof_cmsghdr {
            return None;
        }
        let cmsg: &'a cmsghdr = unsafe { &*(self.buf.as_ptr() as *const cmsghdr) };

        // This check is only in the glibc implementation of CMSG_NXTHDR
        // (although it claims the kernel header checks this), but such
        // a structure is clearly invalid, either way.
        let cmsg_len = cmsg.cmsg_len as usize;
        if cmsg_len < sizeof_cmsghdr {
            return None;
        }
        let len = cmsg_len - sizeof_cmsghdr;
        let aligned_cmsg_len = if self.next == 0 {
            // CMSG_FIRSTHDR
            cmsg_len
        } else {
            // CMSG_NXTHDR
            cmsg_align(cmsg_len)
        };

        // Advance our internal pointer.
        if aligned_cmsg_len > self.buf.len() {
            return None;
        }
        self.buf = &self.buf[aligned_cmsg_len..];
        self.next += 1;

        match (cmsg.cmsg_level, cmsg.cmsg_type) {
            (libc::SOL_SOCKET, libc::SCM_RIGHTS) => unsafe {
                Some(ControlMessage::ScmRights(
                    slice::from_raw_parts(
                        &cmsg.cmsg_data as *const _ as *const _, 1)))
            },
            (libc::SOL_SOCKET, libc::SCM_TIMESTAMP) => unsafe {
                Some(ControlMessage::ScmTimestamp(
                    &*(&cmsg.cmsg_data as *const _ as *const _)))
            },
            (_, _) => unsafe {
                Some(ControlMessage::Unknown(UnknownCmsg(
                    &cmsg,
                    slice::from_raw_parts(
                        &cmsg.cmsg_data as *const _ as *const _,
                        len))))
            }
        }
    }
}

/// A type-safe wrapper around a single control message. More types may
/// be added to this enum; do not exhaustively pattern-match it.
/// [Further reading](http://man7.org/linux/man-pages/man3/cmsg.3.html)
pub enum ControlMessage<'a> {
    /// A message of type `SCM_RIGHTS`, containing an array of file
    /// descriptors passed between processes.
    ///
    /// See the description in the "Ancillary messages" section of the
    /// [unix(7) man page](http://man7.org/linux/man-pages/man7/unix.7.html).
    ScmRights(&'a [RawFd]),
    /// A message of type `SCM_TIMESTAMP`, containing the time the
    /// packet was received by the kernel.
    ///
    /// See the kernel's explanation in "SO_TIMESTAMP" of
    /// [networking/timestamping](https://www.kernel.org/doc/Documentation/networking/timestamping.txt).
    ///
    /// # Examples
    ///
    // Disable this test on FreeBSD i386
    // https://bugs.freebsd.org/bugzilla/show_bug.cgi?id=222039
    #[cfg_attr(not(all(target_os = "freebsd", target_arch = "x86")), doc = " ```")]
    #[cfg_attr(all(target_os = "freebsd", target_arch = "x86"), doc = " ```no_run")]
    /// use nix::sys::socket::*;
    /// use nix::sys::uio::IoVec;
    /// use nix::sys::time::*;
    /// use std::time::*;
    ///
    /// // Set up
    /// let message1 = "Ohayō!".as_bytes();
    /// let message2 = "Jā ne".as_bytes();
    /// let in_socket = socket(AddressFamily::Inet, SockType::Datagram, SockFlag::empty(), None).unwrap();
    /// setsockopt(in_socket, sockopt::ReceiveTimestamp, &true).unwrap();
    /// bind(in_socket, &SockAddr::new_inet(InetAddr::new(IpAddr::new_v4(127, 0, 0, 1), 0))).unwrap();
    /// let address = if let Ok(address) = getsockname(in_socket) { address } else { unreachable!() };
    ///
    /// // Send both
    /// assert!(Ok(message1.len()) == sendmsg(in_socket, &[IoVec::from_slice(message1)], &[], MsgFlags::empty(), Some(&address)));
    /// let time = SystemTime::now();
    /// std::thread::sleep(Duration::from_millis(250));
    /// assert!(Ok(message2.len()) == sendmsg(in_socket, &[IoVec::from_slice(message2)], &[], MsgFlags::empty(), Some(&address)));
    /// let delay = time.elapsed().unwrap();
    ///
    /// // Receive the first
    /// let mut buffer1 = vec![0u8; message1.len() + message2.len()];
    /// let mut time1: CmsgSpace<TimeVal> = CmsgSpace::new();
    /// let received1 = recvmsg(in_socket, &[IoVec::from_mut_slice(&mut buffer1)], Some(&mut time1), MsgFlags::empty()).unwrap();
    /// let mut time1 = if let Some(ControlMessage::ScmTimestamp(&time1)) = received1.cmsgs().next() { time1 } else { panic!("Unexpected or no control message") };
    ///
    /// // Receive the second
    /// let mut buffer2 = vec![0u8; message1.len() + message2.len()];
    /// let mut time2: CmsgSpace<TimeVal> = CmsgSpace::new();
    /// let received2 = recvmsg(in_socket, &[IoVec::from_mut_slice(&mut buffer2)], Some(&mut time2), MsgFlags::empty()).unwrap();
    /// let mut time2 = if let Some(ControlMessage::ScmTimestamp(&time2)) = received2.cmsgs().next() { time2 } else { panic!("Unexpected or no control message") };
    ///
    /// // Swap if needed; UDP is unordered
    /// match (received1.bytes, received2.bytes, message1.len(), message2.len()) {
    ///     (l1, l2, m1, m2) if l1 == m1 && l2 == m2 => {},
    ///     (l2, l1, m1, m2) if l1 == m1 && l2 == m2 => {
    ///         std::mem::swap(&mut time1, &mut time2);
    ///         std::mem::swap(&mut buffer1, &mut buffer2);
    ///     },
    ///     _ => panic!("Wrong packets"),
    /// };
    ///
    /// // Compare results
    /// println!("{:?} @ {:?}, {:?} @ {:?}, {:?}", buffer1, time1, buffer2, time2, delay);
    /// assert!(message1 == &buffer1[0..(message1.len())], "{:?} == {:?}", message1, buffer1);
    /// assert!(message2 == &buffer2[0..(message2.len())], "{:?} == {:?}", message2, buffer2);
    /// let time = time2 - time1;
    /// let time = Duration::new(time.num_seconds() as u64, time.num_nanoseconds() as u32);
    /// let difference = if delay < time { time - delay } else { delay - time };
    /// assert!(difference.subsec_nanos() < 5_000_000, "{}ns < 5ms", difference.subsec_nanos());
    /// assert!(difference.as_secs() == 0);
    ///
    /// // Close socket
    /// nix::unistd::close(in_socket).unwrap();
    /// ```
    ScmTimestamp(&'a TimeVal),
    #[doc(hidden)]
    Unknown(UnknownCmsg<'a>),
}

// An opaque structure used to prevent cmsghdr from being a public type
#[doc(hidden)]
pub struct UnknownCmsg<'a>(&'a cmsghdr, &'a [u8]);

fn cmsg_align(len: usize) -> usize {
    let align_bytes = mem::size_of::<type_of_cmsg_data>() - 1;
    (len + align_bytes) & !align_bytes
}

impl<'a> ControlMessage<'a> {
    /// The value of CMSG_SPACE on this message.
    fn space(&self) -> usize {
        cmsg_align(self.len())
    }

    /// The value of CMSG_LEN on this message.
    fn len(&self) -> usize {
        cmsg_align(mem::size_of::<cmsghdr>()) + match *self {
            ControlMessage::ScmRights(fds) => {
                mem::size_of_val(fds)
            },
            ControlMessage::ScmTimestamp(t) => {
                mem::size_of_val(t)
            },
            ControlMessage::Unknown(UnknownCmsg(_, bytes)) => {
                mem::size_of_val(bytes)
            }
        }
    }

    // Unsafe: start and end of buffer must be size_t-aligned (that is,
    // cmsg_align'd). Updates the provided slice; panics if the buffer
    // is too small.
    unsafe fn encode_into<'b>(&self, buf: &mut &'b mut [u8]) {
        match *self {
            ControlMessage::ScmRights(fds) => {
                let cmsg = cmsghdr {
                    cmsg_len: self.len() as type_of_cmsg_len,
                    cmsg_level: libc::SOL_SOCKET,
                    cmsg_type: libc::SCM_RIGHTS,
                    cmsg_data: [],
                };
                copy_bytes(&cmsg, buf);

                let padlen = cmsg_align(mem::size_of_val(&cmsg)) -
                    mem::size_of_val(&cmsg);

                let mut tmpbuf = &mut [][..];
                mem::swap(&mut tmpbuf, buf);
                let (_padding, mut remainder) = tmpbuf.split_at_mut(padlen);
                mem::swap(buf, &mut remainder);

                copy_bytes(fds, buf);
            },
            ControlMessage::ScmTimestamp(t) => {
                let cmsg = cmsghdr {
                    cmsg_len: self.len() as type_of_cmsg_len,
                    cmsg_level: libc::SOL_SOCKET,
                    cmsg_type: libc::SCM_TIMESTAMP,
                    cmsg_data: [],
                };
                copy_bytes(&cmsg, buf);

                let padlen = cmsg_align(mem::size_of_val(&cmsg)) -
                    mem::size_of_val(&cmsg);

                let mut tmpbuf = &mut [][..];
                mem::swap(&mut tmpbuf, buf);
                let (_padding, mut remainder) = tmpbuf.split_at_mut(padlen);
                mem::swap(buf, &mut remainder);

                copy_bytes(t, buf);
            },
            ControlMessage::Unknown(UnknownCmsg(orig_cmsg, bytes)) => {
                copy_bytes(orig_cmsg, buf);
                copy_bytes(bytes, buf);
            }
        }
    }
}


/// Send data in scatter-gather vectors to a socket, possibly accompanied
/// by ancillary data. Optionally direct the message at the given address,
/// as with sendto.
///
/// Allocates if cmsgs is nonempty.
pub fn sendmsg<'a>(fd: RawFd, iov: &[IoVec<&'a [u8]>], cmsgs: &[ControlMessage<'a>], flags: MsgFlags, addr: Option<&'a SockAddr>) -> Result<usize> {
    let mut len = 0;
    let mut capacity = 0;
    for cmsg in cmsgs {
        len += cmsg.len();
        capacity += cmsg.space();
    }
    // Note that the resulting vector claims to have length == capacity,
    // so it's presently uninitialized.
    let mut cmsg_buffer = unsafe {
        let mut vec = Vec::<u8>::with_capacity(len);
        vec.set_len(len);
        vec
    };
    {
        let mut ptr = &mut cmsg_buffer[..];
        for cmsg in cmsgs {
            unsafe { cmsg.encode_into(&mut ptr) };
        }
    }

    let (name, namelen) = match addr {
        Some(addr) => { let (x, y) = unsafe { addr.as_ffi_pair() }; (x as *const _, y) }
        None => (0 as *const _, 0),
    };

    let cmsg_ptr = if capacity > 0 {
        cmsg_buffer.as_ptr() as *const c_void
    } else {
        ptr::null()
    };

    let mhdr = msghdr {
        msg_name: name as *const c_void,
        msg_namelen: namelen,
        msg_iov: iov.as_ptr(),
        msg_iovlen: iov.len() as type_of_msg_iovlen,
        msg_control: cmsg_ptr,
        msg_controllen: capacity as type_of_cmsg_len,
        msg_flags: 0,
    };
    let ret = unsafe { ffi::sendmsg(fd, &mhdr, flags.bits()) };

    Errno::result(ret).map(|r| r as usize)
}

/// Receive message in scatter-gather vectors from a socket, and
/// optionally receive ancillary data into the provided buffer.
/// If no ancillary data is desired, use () as the type parameter.
pub fn recvmsg<'a, T>(fd: RawFd, iov: &[IoVec<&mut [u8]>], cmsg_buffer: Option<&'a mut CmsgSpace<T>>, flags: MsgFlags) -> Result<RecvMsg<'a>> {
    let mut address: sockaddr_storage = unsafe { mem::uninitialized() };
    let (msg_control, msg_controllen) = match cmsg_buffer {
        Some(cmsg_buffer) => (cmsg_buffer as *mut _, mem::size_of_val(cmsg_buffer)),
        None => (0 as *mut _, 0),
    };
    let mut mhdr = msghdr {
        msg_name: &mut address as *const _ as *const c_void,
        msg_namelen: mem::size_of::<sockaddr_storage>() as socklen_t,
        msg_iov: iov.as_ptr() as *const IoVec<&[u8]>, // safe cast to add const-ness
        msg_iovlen: iov.len() as type_of_msg_iovlen,
        msg_control: msg_control as *const c_void,
        msg_controllen: msg_controllen as type_of_cmsg_len,
        msg_flags: 0,
    };
    let ret = unsafe { ffi::recvmsg(fd, &mut mhdr, flags.bits()) };

    Ok(unsafe { RecvMsg {
        bytes: try!(Errno::result(ret)) as usize,
        cmsg_buffer: slice::from_raw_parts(mhdr.msg_control as *const u8,
                                           mhdr.msg_controllen as usize),
        address: sockaddr_storage_to_addr(&address,
                                          mhdr.msg_namelen as usize).ok(),
        flags: MsgFlags::from_bits_truncate(mhdr.msg_flags),
    } })
}


/// Create an endpoint for communication
///
/// The `protocol` specifies a particular protocol to be used with the
/// socket.  Normally only a single protocol exists to support a
/// particular socket type within a given protocol family, in which case
/// protocol can be specified as `None`.  However, it is possible that many
/// protocols may exist, in which case a particular protocol must be
/// specified in this manner.
///
/// [Further reading](http://man7.org/linux/man-pages/man2/socket.2.html)
pub fn socket<T: Into<Option<SockProtocol>>>(domain: AddressFamily, ty: SockType, flags: SockFlag, protocol: T) -> Result<RawFd> {
    let mut ty = ty as c_int;
    let protocol = match protocol.into() {
        None => 0,
        Some(p) => p as c_int,
    };
    let feat_atomic = features::socket_atomic_cloexec();

    if feat_atomic {
        ty = ty | flags.bits();
    }

    // TODO: Check the kernel version
    let res = try!(Errno::result(unsafe { libc::socket(domain as c_int, ty, protocol) }));

    #[cfg(any(target_os = "android",
              target_os = "dragonfly",
              target_os = "freebsd",
              target_os = "linux",
              target_os = "netbsd",
              target_os = "openbsd"))]
    {
        use fcntl::{fcntl, FD_CLOEXEC, O_NONBLOCK};
        use fcntl::FcntlArg::{F_SETFD, F_SETFL};

        if !feat_atomic {
            if flags.contains(SOCK_CLOEXEC) {
                try!(fcntl(res, F_SETFD(FD_CLOEXEC)));
            }

            if flags.contains(SOCK_NONBLOCK) {
                try!(fcntl(res, F_SETFL(O_NONBLOCK)));
            }
        }
    }

    Ok(res)
}

/// Create a pair of connected sockets
///
/// [Further reading](http://man7.org/linux/man-pages/man2/socketpair.2.html)
pub fn socketpair<T: Into<Option<SockProtocol>>>(domain: AddressFamily, ty: SockType, protocol: T,
                  flags: SockFlag) -> Result<(RawFd, RawFd)> {
    let mut ty = ty as c_int;
    let protocol = match protocol.into() {
        None => 0,
        Some(p) => p as c_int,
    };
    let feat_atomic = features::socket_atomic_cloexec();

    if feat_atomic {
        ty = ty | flags.bits();
    }
    let mut fds = [-1, -1];
    let res = unsafe {
        libc::socketpair(domain as c_int, ty, protocol, fds.as_mut_ptr())
    };
    try!(Errno::result(res));

    #[cfg(any(target_os = "android",
              target_os = "dragonfly",
              target_os = "freebsd",
              target_os = "linux",
              target_os = "netbsd",
              target_os = "openbsd"))]
    {
        use fcntl::{fcntl, FD_CLOEXEC, O_NONBLOCK};
        use fcntl::FcntlArg::{F_SETFD, F_SETFL};

        if !feat_atomic {
            if flags.contains(SOCK_CLOEXEC) {
                try!(fcntl(fds[0], F_SETFD(FD_CLOEXEC)));
                try!(fcntl(fds[1], F_SETFD(FD_CLOEXEC)));
            }

            if flags.contains(SOCK_NONBLOCK) {
                try!(fcntl(fds[0], F_SETFL(O_NONBLOCK)));
                try!(fcntl(fds[1], F_SETFL(O_NONBLOCK)));
            }
        }
    }
    Ok((fds[0], fds[1]))
}

/// Listen for connections on a socket
///
/// [Further reading](http://man7.org/linux/man-pages/man2/listen.2.html)
pub fn listen(sockfd: RawFd, backlog: usize) -> Result<()> {
    let res = unsafe { libc::listen(sockfd, backlog as c_int) };

    Errno::result(res).map(drop)
}

/// Bind a name to a socket
///
/// [Further reading](http://man7.org/linux/man-pages/man2/bind.2.html)
#[cfg(not(all(target_os="android", target_pointer_width="64")))]
pub fn bind(fd: RawFd, addr: &SockAddr) -> Result<()> {
    let res = unsafe {
        let (ptr, len) = addr.as_ffi_pair();
        libc::bind(fd, ptr, len)
    };

    Errno::result(res).map(drop)
}

/// Bind a name to a socket
///
/// [Further reading](http://man7.org/linux/man-pages/man2/bind.2.html)
// Android has some weirdness. Its 64-bit bind takes a c_int instead of a
// socklen_t
#[cfg(all(target_os="android", target_pointer_width="64"))]
pub fn bind(fd: RawFd, addr: &SockAddr) -> Result<()> {
    let res = unsafe {
        let (ptr, len) = addr.as_ffi_pair();
        libc::bind(fd, ptr, len as c_int)
    };

    Errno::result(res).map(drop)
}

/// Accept a connection on a socket
///
/// [Further reading](http://man7.org/linux/man-pages/man2/accept.2.html)
pub fn accept(sockfd: RawFd) -> Result<RawFd> {
    let res = unsafe { libc::accept(sockfd, ptr::null_mut(), ptr::null_mut()) };

    Errno::result(res)
}

/// Accept a connection on a socket
///
/// [Further reading](http://man7.org/linux/man-pages/man2/accept.2.html)
pub fn accept4(sockfd: RawFd, flags: SockFlag) -> Result<RawFd> {
    accept4_polyfill(sockfd, flags)
}

#[inline]
fn accept4_polyfill(sockfd: RawFd, flags: SockFlag) -> Result<RawFd> {
    let res = try!(Errno::result(unsafe { libc::accept(sockfd, ptr::null_mut(), ptr::null_mut()) }));

    #[cfg(any(target_os = "android",
              target_os = "dragonfly",
              target_os = "freebsd",
              target_os = "linux",
              target_os = "netbsd",
              target_os = "openbsd"))]
    {
        use fcntl::{fcntl, FD_CLOEXEC, O_NONBLOCK};
        use fcntl::FcntlArg::{F_SETFD, F_SETFL};

        if flags.contains(SOCK_CLOEXEC) {
            try!(fcntl(res, F_SETFD(FD_CLOEXEC)));
        }

        if flags.contains(SOCK_NONBLOCK) {
            try!(fcntl(res, F_SETFL(O_NONBLOCK)));
        }
    }

    // Disable unused variable warning on some platforms
    #[cfg(not(any(target_os = "android",
                  target_os = "dragonfly",
                  target_os = "freebsd",
                  target_os = "linux",
                  target_os = "netbsd",
                  target_os = "openbsd")))]
    {
        let _ = flags;
    }


    Ok(res)
}

/// Initiate a connection on a socket
///
/// [Further reading](http://man7.org/linux/man-pages/man2/connect.2.html)
pub fn connect(fd: RawFd, addr: &SockAddr) -> Result<()> {
    let res = unsafe {
        let (ptr, len) = addr.as_ffi_pair();
        libc::connect(fd, ptr, len)
    };

    Errno::result(res).map(drop)
}

/// Receive data from a connection-oriented socket. Returns the number of
/// bytes read
///
/// [Further reading](http://man7.org/linux/man-pages/man2/recv.2.html)
pub fn recv(sockfd: RawFd, buf: &mut [u8], flags: MsgFlags) -> Result<usize> {
    unsafe {
        let ret = ffi::recv(
            sockfd,
            buf.as_ptr() as *mut c_void,
            buf.len() as size_t,
            flags.bits());

        Errno::result(ret).map(|r| r as usize)
    }
}

/// Receive data from a connectionless or connection-oriented socket. Returns
/// the number of bytes read and the socket address of the sender.
///
/// [Further reading](http://man7.org/linux/man-pages/man2/recvmsg.2.html)
pub fn recvfrom(sockfd: RawFd, buf: &mut [u8]) -> Result<(usize, SockAddr)> {
    unsafe {
        let addr: sockaddr_storage = mem::zeroed();
        let mut len = mem::size_of::<sockaddr_storage>() as socklen_t;

        let ret = try!(Errno::result(ffi::recvfrom(
            sockfd,
            buf.as_ptr() as *mut c_void,
            buf.len() as size_t,
            0,
            mem::transmute(&addr),
            &mut len as *mut socklen_t)));

        sockaddr_storage_to_addr(&addr, len as usize)
            .map(|addr| (ret as usize, addr))
    }
}

pub fn sendto(fd: RawFd, buf: &[u8], addr: &SockAddr, flags: MsgFlags) -> Result<usize> {
    let ret = unsafe {
        let (ptr, len) = addr.as_ffi_pair();
        libc::sendto(fd, buf.as_ptr() as *const c_void, buf.len() as size_t, flags.bits(), ptr, len)
    };

    Errno::result(ret).map(|r| r as usize)
}

/// Send data to a connection-oriented socket. Returns the number of bytes read
///
/// [Further reading](http://man7.org/linux/man-pages/man2/send.2.html)
pub fn send(fd: RawFd, buf: &[u8], flags: MsgFlags) -> Result<usize> {
    let ret = unsafe {
        libc::send(fd, buf.as_ptr() as *const c_void, buf.len() as size_t, flags.bits())
    };

    Errno::result(ret).map(|r| r as usize)
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct linger {
    pub l_onoff: c_int,
    pub l_linger: c_int
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ucred {
    pid: pid_t,
    uid: uid_t,
    gid: gid_t,
}

/*
 *
 * ===== Socket Options =====
 *
 */

/// The protocol level at which to get / set socket options. Used as an
/// argument to `getsockopt` and `setsockopt`.
///
/// [Further reading](http://man7.org/linux/man-pages/man2/setsockopt.2.html)
#[repr(i32)]
pub enum SockLevel {
    Socket = libc::SOL_SOCKET,
    Tcp = libc::IPPROTO_TCP,
    Ip = libc::IPPROTO_IP,
    Ipv6 = libc::IPPROTO_IPV6,
    Udp = libc::IPPROTO_UDP,
    #[cfg(any(target_os = "linux", target_os = "android"))]
    Netlink = libc::SOL_NETLINK,
}

/// Represents a socket option that can be accessed or set. Used as an argument
/// to `getsockopt`
pub trait GetSockOpt : Copy {
    type Val;

    #[doc(hidden)]
    fn get(&self, fd: RawFd) -> Result<Self::Val>;
}

/// Represents a socket option that can be accessed or set. Used as an argument
/// to `setsockopt`
pub trait SetSockOpt : Copy {
    type Val;

    #[doc(hidden)]
    fn set(&self, fd: RawFd, val: &Self::Val) -> Result<()>;
}

/// Get the current value for the requested socket option
///
/// [Further reading](http://man7.org/linux/man-pages/man2/getsockopt.2.html)
pub fn getsockopt<O: GetSockOpt>(fd: RawFd, opt: O) -> Result<O::Val> {
    opt.get(fd)
}

/// Sets the value for the requested socket option
///
/// [Further reading](http://man7.org/linux/man-pages/man2/setsockopt.2.html)
pub fn setsockopt<O: SetSockOpt>(fd: RawFd, opt: O, val: &O::Val) -> Result<()> {
    opt.set(fd, val)
}

/// Get the address of the peer connected to the socket `fd`.
///
/// [Further reading](http://man7.org/linux/man-pages/man2/getpeername.2.html)
pub fn getpeername(fd: RawFd) -> Result<SockAddr> {
    unsafe {
        let addr: sockaddr_storage = mem::uninitialized();
        let mut len = mem::size_of::<sockaddr_storage>() as socklen_t;

        let ret = libc::getpeername(fd, mem::transmute(&addr), &mut len);

        try!(Errno::result(ret));

        sockaddr_storage_to_addr(&addr, len as usize)
    }
}

/// Get the current address to which the socket `fd` is bound.
///
/// [Further reading](http://man7.org/linux/man-pages/man2/getsockname.2.html)
pub fn getsockname(fd: RawFd) -> Result<SockAddr> {
    unsafe {
        let addr: sockaddr_storage = mem::uninitialized();
        let mut len = mem::size_of::<sockaddr_storage>() as socklen_t;

        let ret = libc::getsockname(fd, mem::transmute(&addr), &mut len);

        try!(Errno::result(ret));

        sockaddr_storage_to_addr(&addr, len as usize)
    }
}

/// Return the appropriate SockAddr type from a `sockaddr_storage` of a certain
/// size.  In C this would usually be done by casting.  The `len` argument
/// should be the number of bytes in the sockaddr_storage that are actually
/// allocated and valid.  It must be at least as large as all the useful parts
/// of the structure.  Note that in the case of a `sockaddr_un`, `len` need not
/// include the terminating null.
pub unsafe fn sockaddr_storage_to_addr(
    addr: &sockaddr_storage,
    len: usize) -> Result<SockAddr> {

    if len < mem::size_of_val(&addr.ss_family) {
        return Err(Error::Sys(Errno::ENOTCONN));
    }

    match addr.ss_family as c_int {
        libc::AF_INET => {
            assert!(len as usize == mem::size_of::<sockaddr_in>());
            let ret = *(addr as *const _ as *const sockaddr_in);
            Ok(SockAddr::Inet(InetAddr::V4(ret)))
        }
        libc::AF_INET6 => {
            assert!(len as usize == mem::size_of::<sockaddr_in6>());
            Ok(SockAddr::Inet(InetAddr::V6((*(addr as *const _ as *const sockaddr_in6)))))
        }
        libc::AF_UNIX => {
            let sun = *(addr as *const _ as *const sockaddr_un);
            let pathlen = len - offset_of!(sockaddr_un, sun_path);
            Ok(SockAddr::Unix(UnixAddr(sun, pathlen)))
        }
        #[cfg(any(target_os = "linux", target_os = "android"))]
        libc::AF_NETLINK => {
            use libc::sockaddr_nl;
            Ok(SockAddr::Netlink(NetlinkAddr(*(addr as *const _ as *const sockaddr_nl))))
        }
        af => panic!("unexpected address family {}", af),
    }
}


#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shutdown {
    /// Further receptions will be disallowed.
    Read,
    /// Further  transmissions will be disallowed.
    Write,
    /// Further receptions and transmissions will be disallowed.
    Both,
}

/// Shut down part of a full-duplex connection.
///
/// [Further reading](http://man7.org/linux/man-pages/man2/shutdown.2.html)
pub fn shutdown(df: RawFd, how: Shutdown) -> Result<()> {
    unsafe {
        use libc::shutdown;

        let how = match how {
            Shutdown::Read  => libc::SHUT_RD,
            Shutdown::Write => libc::SHUT_WR,
            Shutdown::Both  => libc::SHUT_RDWR,
        };

        Errno::result(shutdown(df, how)).map(drop)
    }
}

#[test]
pub fn test_struct_sizes() {
    use nixtest;
    nixtest::assert_size_of::<sockaddr_storage>("sockaddr_storage");
}
