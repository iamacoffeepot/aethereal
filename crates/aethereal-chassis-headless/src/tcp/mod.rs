//! The TCP capability on the native host: its dialer, names looked up and connected to on threads of its own, and its
//! bootstrap (ADR-0033, ADR-0034).

#[cfg(test)]
mod tests;

use crate::Headless;
use aethereal_actor::Service;
use aethereal_substrate::{Bootstrap, Bootstrapping, Unbuilt};
use aethereal_tcp::{Dialer, Dialing, Tcp, TcpSettings, Undialed};
use crossbeam_queue::SegQueue;
use std::future::Future;
use std::mem;
use std::net::{TcpStream, ToSocketAddrs};
use std::num::NonZeroUsize;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Poll, Waker};
use std::thread::{self, JoinHandle, Thread};
use std::time::Duration;

/// The native host's dialer (ADR-0033): each dial done on one of threads of its own, which looks the name up and tries
/// each of its addresses in turn, so a dial never blocks the thread that polls it, and is woken once it has connected
/// or given up. Dropping the last reference to it stops its threads once each has finished what it was dialing.
pub struct Dialers {
    shared: Arc<Shared>,
    /// The threads that dial, which the dials asked wake in turn.
    dialing: Vec<Thread>,
    /// Which of them the next dial asked wakes.
    next: AtomicUsize,
    /// Every thread, to join once they are told to stop.
    threads: Vec<JoinHandle<()>>,
}

/// What the threads share: the dials asked and not yet taken, how long each address is tried, and whether to stop. The
/// queue is how a dial crosses from the thread that polls it to one that blocks on the network, which no turn's mail
/// reaches.
struct Shared {
    asked: SegQueue<Asked>,
    within: Duration,
    stopping: AtomicBool,
}

/// One dial asked: where to, and where its connection goes.
struct Asked {
    host: String,
    port: u16,
    delivery: Arc<Mutex<Delivery>>,
}

/// Where a dial stands, as the thread that dials and the job that polls it both see it. A lock, as the two are on
/// different threads and either may come first.
enum Delivery {
    /// Asked, and not yet polled.
    Asked,
    /// Polled before it had connected: woken once it has, or has given up.
    Polled(Waker),
    /// Dialed: the connection, or why there is none.
    Dialed(Result<TcpStream, Undialed>),
    /// Handed to the job that polled it.
    Handed,
}

impl Dialers {
    /// The dialer on `threads` threads, trying each address for at most `within`.
    ///
    /// # Panics
    ///
    /// Panics when the host cannot make a thread.
    #[must_use]
    pub fn new(threads: NonZeroUsize, within: Duration) -> Self {
        // A connect given no time fails at once, so each address is given a millisecond at the least.
        let within = within.max(Duration::from_millis(1));
        let shared = Arc::new(Shared { asked: SegQueue::new(), within, stopping: AtomicBool::new(false) });
        let threads: Vec<JoinHandle<()>> = (0..threads.get())
            .map(|number| {
                let shared = Arc::clone(&shared);
                #[allow(clippy::disallowed_methods)] // The native host owns the real threads (ADR-0001).
                let spawned =
                    thread::Builder::new().name(format!("aethereal-dial-{number}")).spawn(move || dial(&shared));
                let Ok(thread) = spawned else {
                    panic!("the host makes a thread to dial");
                };
                thread
            })
            .collect();
        let dialing = threads.iter().map(|thread| thread.thread().clone()).collect();

        Self { shared, dialing, next: AtomicUsize::new(0), threads }
    }

    /// The dialer `settings` describe: as many threads as it says, one at the least, each address tried for as long as
    /// it says.
    ///
    /// # Panics
    ///
    /// Panics when the host cannot make a thread.
    #[must_use]
    pub fn of(settings: &TcpSettings) -> Self {
        let threads = usize::try_from(settings.dialers).ok().and_then(NonZeroUsize::new).unwrap_or(NonZeroUsize::MIN);

        Self::new(threads, Duration::from_millis(u64::from(settings.dial_within_millis)))
    }
}

impl Dialer for Dialers {
    fn dial<'a>(&'a self, host: &'a str, port: u16) -> Dialing<'a> {
        let delivery = Arc::new(Mutex::new(Delivery::Asked));
        self.shared.asked.push(Asked { host: String::from(host), port, delivery: Arc::clone(&delivery) });
        let dialer = self.next.fetch_add(1, Ordering::Relaxed) % self.dialing.len();
        self.dialing[dialer].unpark();

        Box::pin(Dial { delivery })
    }
}

/// The TCP capability on the native host: its dials on threads of the host's own, served into the services its runners
/// hand every job (ADR-0034).
impl Bootstrap<Headless> for Tcp {
    const SECTION: &'static str = "tcp";
    type Settings = TcpSettings;
    type Needs = ();
    type Provides = ();

    async fn bootstrap(
        settings: TcpSettings,
        (): (),
        cx: &mut Bootstrapping<'_, Headless, Self>,
    ) -> Result<(), Unbuilt> {
        cx.serve(Service::<dyn Dialer>::of(Arc::new(Dialers::of(&settings))));
        cx.begin(&(), settings.config())
    }
}

impl Drop for Dialers {
    fn drop(&mut self) {
        self.shared.stopping.store(true, Ordering::Release);
        for dialer in &self.dialing {
            dialer.unpark();
        }
        for thread in self.threads.drain(..) {
            // A thread that panicked has nothing left to dial.
            let _ = thread.join();
        }
    }
}

/// Takes dials and does each until it is told to stop, parking whenever there is none to take.
fn dial(shared: &Shared) {
    loop {
        while let Some(Asked { host, port, delivery }) = shared.asked.pop() {
            let dialed = connect(&host, port, shared.within);
            let was = mem::replace(&mut *delivered(&delivery), Delivery::Dialed(dialed));
            if let Delivery::Polled(waker) = was {
                waker.wake();
            }
        }
        if shared.stopping.load(Ordering::Acquire) {
            return;
        }
        thread::park();
    }
}

/// A connection to `port` on `host`, set not to block: each address the name has tried in turn, for at most `within`.
fn connect(host: &str, port: u16, within: Duration) -> Result<TcpStream, Undialed> {
    let addresses: Vec<_> = (host, port).to_socket_addrs().map_err(|_| Undialed::Unresolved)?.collect();
    if addresses.is_empty() {
        return Err(Undialed::Unresolved);
    }
    let opened = |stream: &TcpStream| stream.set_nonblocking(true).is_ok();

    addresses
        .iter()
        .find_map(|address| TcpStream::connect_timeout(address, within).ok().filter(opened))
        .ok_or(Undialed::Refused)
}

/// Where a dial stands, whatever a thread that panicked holding it left.
fn delivered(delivery: &Mutex<Delivery>) -> MutexGuard<'_, Delivery> {
    delivery.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A dial, as the job that asked it polls it.
struct Dial {
    delivery: Arc<Mutex<Delivery>>,
}

impl Future for Dial {
    type Output = Result<TcpStream, Undialed>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let mut delivery = delivered(&self.delivery);
        let was = mem::replace(&mut *delivery, Delivery::Handed);
        match was {
            Delivery::Dialed(dialed) => Poll::Ready(dialed),
            Delivery::Asked | Delivery::Polled(_) => {
                *delivery = Delivery::Polled(context.waker().clone());
                Poll::Pending
            }
            Delivery::Handed => Poll::Ready(Err(Undialed::Refused)),
        }
    }
}
