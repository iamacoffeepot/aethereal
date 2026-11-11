//! The native host's poller, on real sockets of this machine's loopback.

use super::*;
use crate::waiting;
use std::io::Write as _;
use std::net::{TcpListener, TcpStream};
use std::os::fd::AsRawFd;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::task::Wake;
use std::thread;

/// A waker that counts how often it is woken: what the poller's report does is wake a waker, which is `Send` and
/// `Sync` and woken from the poller's thread, so its count is an atomic, and no mail orders it.
struct Counted {
    wakes: AtomicUsize,
}

impl Wake for Counted {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.wakes.fetch_add(1, Ordering::Relaxed);
    }
}

/// A waker that counts, and the count it keeps.
fn counted() -> (Waker, Arc<Counted>) {
    let counted = Arc::new(Counted { wakes: AtomicUsize::new(0) });

    (Waker::from(Arc::clone(&counted)), counted)
}

/// How often `counted` has been woken.
fn wakes(counted: &Counted) -> usize {
    counted.wakes.load(Ordering::Relaxed)
}

/// A connection on loopback: the end that dialed, and the end accepted, which does not block.
fn connected() -> (TcpStream, TcpStream) {
    let Ok(listener) = TcpListener::bind("127.0.0.1:0") else {
        panic!("loopback binds a free port");
    };
    let Ok(address) = listener.local_addr() else {
        panic!("the listener has an address");
    };
    let Ok(dialed) = TcpStream::connect(address) else {
        panic!("loopback connects");
    };
    let Ok((accepted, _)) = listener.accept() else {
        panic!("the listener accepts");
    };
    let Ok(()) = accepted.set_nonblocking(true) else {
        panic!("the accepted end is set not to block");
    };

    (dialed, accepted)
}

/// The descriptor of `stream`, as a job names it.
fn descriptor(stream: &TcpStream) -> u64 {
    let Ok(descriptor) = u64::try_from(stream.as_raw_fd()) else {
        panic!("a socket's descriptor is not negative");
    };

    descriptor
}

/// `wait` polled once with `waker`.
fn poll(wait: &mut Readying<'_>, waker: &Waker) -> Poll<()> {
    wait.as_mut().poll(&mut Context::from_waker(waker))
}

/// Long enough for loopback to deliver, and short beside a test's budget: a sleep returns once something is reported.
const DELIVERED: Duration = Duration::from_secs(5);

#[test]
fn a_wait_for_a_readable_socket_is_ready_only_once_bytes_arrive() {
    // Catches a wait armed for the wrong filter, never armed, or reported before the socket is ready, and a report
    // that does not wake the job that waits.
    let poller = Poller::new();
    let (mut dialed, accepted) = connected();
    let (waker, counted) = counted();
    let mut wait = poller.ready(descriptor(&accepted), Interest::Readable);

    let before = poll(&mut wait, &waker);
    poller.sleep(Sleep::Within(Duration::from_millis(20)));
    let idle = (poll(&mut wait, &waker), wakes(&counted));
    let Ok(()) = dialed.write_all(b"ping") else {
        panic!("the dialed end writes");
    };
    poller.sleep(Sleep::Within(DELIVERED));
    let woken = wakes(&counted);
    let after = poll(&mut wait, &waker);

    assert_eq!(before, Poll::Pending);
    assert_eq!(idle, (Poll::Pending, 0), "nothing has arrived");
    assert_eq!(woken, 1, "the report wakes the job");
    assert_eq!(after, Poll::Ready(()));
}

#[test]
fn a_socket_waited_on_both_ways_is_still_waited_on_for_reading_once_writing_is_reported() {
    // Catches a system that disarms a whole socket on its first report, as epoll does, without the poller arming again
    // what still waits on it: the read would never be reported.
    let poller = Poller::new();
    let (mut dialed, accepted) = connected();
    let (waker, _) = counted();
    let mut reading = poller.ready(descriptor(&accepted), Interest::Readable);
    let mut writing = poller.ready(descriptor(&accepted), Interest::Writable);
    let armed = (poll(&mut reading, &waker), poll(&mut writing, &waker));

    // A connection just opened has room to write, and nothing to read.
    poller.sleep(Sleep::Within(DELIVERED));
    let wrote = (poll(&mut reading, &waker), poll(&mut writing, &waker));
    let Ok(()) = dialed.write_all(b"ping") else {
        panic!("the dialed end writes");
    };
    poller.sleep(Sleep::Within(DELIVERED));
    let read = poll(&mut reading, &waker);

    assert_eq!(armed, (Poll::Pending, Poll::Pending));
    assert_eq!(wrote, (Poll::Pending, Poll::Ready(())));
    assert_eq!(read, Poll::Ready(()));
}

#[test]
fn a_notify_wakes_the_poller_asleep_even_when_it_came_first() {
    // Catches a notify that does not reach a thread asleep with nothing armed, and one lost when it comes before the
    // thread sleeps: the job it was for would wait until something else woke the thread, or for good.
    let poller = Arc::new(Poller::new());
    poller.notify();
    let asleep = Arc::clone(&poller);
    #[allow(clippy::disallowed_methods)] // The test sleeps the poller on a real thread, to bound it.
    let first = thread::spawn(move || asleep.sleep(Sleep::Forever));
    waiting::until(Duration::from_secs(10), || first.is_finished());
    let woke_first = first.is_finished();

    let asleep = Arc::clone(&poller);
    #[allow(clippy::disallowed_methods)] // The test sleeps the poller on a real thread, to bound it.
    let second = thread::spawn(move || asleep.sleep(Sleep::Forever));
    poller.notify();
    waiting::until(Duration::from_secs(10), || second.is_finished());
    let woke_second = second.is_finished();

    assert!(woke_first, "a notify that came before the sleep wakes it");
    assert!(woke_second, "a notify while it sleeps wakes it");
}
