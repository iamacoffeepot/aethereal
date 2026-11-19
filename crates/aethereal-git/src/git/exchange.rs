//! One request served through one run of `git http-backend` (ADR-0035, ADR-0040): its body written to the program's
//! stdin as the stack reads it, and the program's stdout read up to the end of its CGI head, then streamed as the body
//! of the response, each way paced by the side that takes it.
//!
//! ```text
//! request ─▶ Starting ── Started ─▶ Running ──────────────────────────────────────────────▶ (exited: dropped)
//!                                    input:  Writing ── body whole, every write sent ─▶ Written
//!                                    output: Head ── head read ─▶ Headed ── body written,
//!                                                                         or stdout ended ─▶ Body ── stdout ended,
//!                                                                                                    program exited ─▶ ended
//!            anything that breaks it ─▶ Stopping, its program stopped, until it exits
//! ```

use super::{Git, Unopened, Writing};
use aethereal_actor::{
    ActorRef, Answered, Bytes, Fault, Held, JobId, Level, Number, Part, Sleep, StreamKey, Taken, Turn, Unsent,
};
use aethereal_data::{Bound, Plain};
use aethereal_http::Status;
use aethereal_http::cgi::{self, CgiHead};
use aethereal_http::stack::{
    BodyHandle, BodyRead, InboundBody, Response, ResponseBody, ResponseBytes, ResponseHead, ResponseLength, SourceEnd,
    StreamedBody,
};
use aethereal_process::{ExitStatus, Exited, Pipe, Piped, Process, Started, Stop};
use aethereal_tcp::{Finish, HungUp, Read, Reading, Write, Wrote};
use core::mem;
use core::num::NonZeroU32;
use core::time::Duration;
use std::collections::VecDeque;

/// The most bytes of a program's CGI head, past which its exchange is answered `502`.
const MOST_HEAD_BYTES: u32 = 16 * 1024;

/// How long a response answered waits for its stack to open its body before its program is stopped: a stack opens it
/// as soon as it has passed the response's head out through its layers, and one that never does closed the request's
/// connection before the answer came.
const OPENED_WITHIN: Duration = Duration::from_mins(1);

/// One request served, by where it stands.
#[derive(Bound)]
pub(super) enum Exchange {
    /// Its program is asked of `Processes`, the request held and its body unread.
    Starting { held: Held<Response>, body: BodyHandle },
    /// Its program runs.
    Running(Running),
    /// It broke off, its request answered and its response's body ended or never to be read, and its program, asked to
    /// stop, has not yet exited: it keeps its place among the most running until it has.
    Stopping { process: ActorRef<Process> },
}

/// An exchange whose program runs: the program, its pipe, and where its stdin, its stdout, and its exit stand.
#[derive(Bound)]
pub(super) struct Running {
    process: ActorRef<Process>,
    pipe: ActorRef<Pipe>,
    stdout: Stdout,
    input: Input,
    output: Output,
    exit: Exit,
}

/// Where a program's stdout stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum Stdout {
    /// It is read through the stream `Number`.
    Open(Number),
    /// It ended as `HungUp` says: `Peer` when the program closed it.
    Ended(HungUp),
}

/// Where the request's body stands on its way to the program's stdin.
#[derive(Bound)]
enum Input {
    /// It is read and written as it comes: its stream, the bytes taken and not yet written, and the write out.
    Writing { body: Body, queued: Vec<Bytes>, sending: Out },
    /// Every byte of it was written, and the program's stdin finished.
    Written,
    /// The program's stdin took no more, or its response was answered before its body ended: the rest goes unwritten.
    Abandoned,
}

/// Where the stream of a request's body stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum Body {
    /// It is read through the stream `Number`.
    Open(Number),
    /// It ended whole.
    Read,
}

/// The write to a program's stdin that is out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum Out {
    /// None.
    Idle,
    /// One of this many parts of the body, each to be granted back to its stream once the write is sent.
    Parts(NonZeroU32),
}

/// Where a program's stdout stands toward the response, and with it the request's answer.
#[derive(Bound)]
enum Output {
    /// Its CGI head is read: what of it has come. The request is held.
    Head { held: Held<Response>, head: Vec<u8> },
    /// Its head is read, the request held until its body is written or the program ends: the response's head, and
    /// what came of stdout past it, each part with one part of the stream's credit to grant back once it is sent.
    Headed { held: Held<Response>, head: ResponseHead, waiting: VecDeque<Bytes> },
    /// The request is answered, its body streamed from here: the stack's stream, and what came of stdout and waits for
    /// its credit.
    Body { stream: Downstream, waiting: VecDeque<Bytes> },
}

/// Where the stack's stream of a response's body stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Bound)]
enum Downstream {
    /// It is not open yet: the timer `JobId` stops the program unless it opens in time.
    Unopened(JobId),
    /// It is open, as `StreamKey`.
    Open(StreamKey),
}

/// Whether a program has exited.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum Exit {
    Running,
    Exited(ExitStatus),
}

/// What an exchange does next, as where it stands says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Next {
    /// Nothing until more comes.
    Wait,
    /// Its program's stdout ended before its head did: it is answered `502`.
    Unheaded,
    /// Its request is answered with the head read.
    Answer,
    /// Its response's body is ended, as `SourceEnd` says.
    Finish(SourceEnd),
}

impl Exchange {
    /// Whether `process`, the bits of a process's id, is its program's.
    pub(super) fn runs(&self, process: u64) -> bool {
        let running = match self {
            Self::Starting { .. } => return false,
            Self::Running(running) => running.process,
            Self::Stopping { process } => *process,
        };

        running.id().bits() == process
    }

    /// Whether it streams its response's body as the stack's stream `stream`.
    fn streams(&self, stream: StreamKey) -> bool {
        let Self::Running(Running { output: Output::Body { stream: Downstream::Open(open), .. }, .. }) = self else {
            return false;
        };

        *open == stream
    }
}

impl Running {
    /// What it does next.
    fn next(&self) -> Next {
        let Stdout::Ended(how) = self.stdout else {
            let is_written = !matches!(self.input, Input::Writing { .. });
            let is_headed = matches!(self.output, Output::Headed { .. });
            let answers = is_written && is_headed;
            let next = if answers {
                Next::Answer
            } else {
                Next::Wait
            };
            return next;
        };

        // Its stdout has ended, so its response is all there is: a head that never came is the program's failure, and
        // one that came is answered whatever is left of the request's body. Once every byte is sent, the body ends
        // whole only when the program closed its stdout and then exited well.
        match &self.output {
            Output::Head { .. } => Next::Unheaded,
            Output::Headed { .. } => Next::Answer,
            Output::Body { stream: Downstream::Open(_), waiting } if waiting.is_empty() => {
                let Exit::Exited(status) = self.exit else {
                    return Next::Wait;
                };
                let closed = how == HungUp::Peer;
                let succeeded = status == ExitStatus::Code(0);
                let whole = closed && succeeded;
                let end = if whole {
                    SourceEnd::Whole
                } else {
                    SourceEnd::Failed
                };
                Next::Finish(end)
            }
            Output::Body { .. } => Next::Wait,
        }
    }
}

impl Git {
    /// An exchange's program is started: its stdout and its request's body are opened, and how it exited is claimed if
    /// that came first. One the host would not start is answered `502`. Its start is answered once, and it is
    /// `Starting` until then.
    pub(super) fn exchange_started(
        &mut self,
        exchange: u64,
        answered: &Answered<Started>,
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        let Some(Exchange::Starting { held, body }) = self.exchanges.remove(&exchange) else {
            return Ok(());
        };
        let (process, pipe) = match answered {
            Answered::Yes(Started::Running { process, pipe: Piped::Connected(pipe) }) => (*process, *pipe),
            &Answered::Yes(Started::Running { process, pipe: Piped::Closed }) => {
                turn.to(process).send(&Stop)?;
                self.exchanges.insert(exchange, Exchange::Stopping { process });
                return Ok(turn.answer(held, &Response::empty(Status::BadGateway))?);
            }
            Answered::Yes(Started::Refused) | Answered::No(_) => {
                turn.log(Level::Warn, format_args!("exchange {exchange}'s program did not start: {answered:?}"));
                return Ok(turn.answer(held, &Response::empty(Status::BadGateway))?);
            }
        };

        let stdout = self.outputs.open(turn, pipe.protocol(), Read, exchange)?;
        let read = self.bodies.open(turn, body.reader, body.read(), exchange)?;
        let exit = self
            .unclaimed
            .remove(&process.id().bits())
            .map_or(Exit::Running, |exited| Exit::Exited(exited_well(exchange, exited, turn)));
        let running = Running {
            process,
            pipe,
            stdout: Stdout::Open(stdout),
            input: Input::Writing { body: Body::Open(read), queued: Vec::new(), sending: Out::Idle },
            output: Output::Head { held, head: Vec::new() },
            exit,
        };
        self.exchanges.insert(exchange, Exchange::Running(running));

        self.settle(exchange, turn)
    }

    /// An exchange's program has exited: one stopped is over, and one running goes on as its stdout and its response
    /// say.
    pub(super) fn exchange_exited(
        &mut self,
        exchange: u64,
        exited: Exited,
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        match self.exchanges.get_mut(&exchange) {
            Some(Exchange::Running(running)) => {
                running.exit = Exit::Exited(exited_well(exchange, exited, turn));
                self.settle(exchange, turn)
            }
            Some(Exchange::Stopping { .. }) => {
                self.exchanges.remove(&exchange);
                Ok(())
            }
            Some(Exchange::Starting { .. }) | None => Ok(()),
        }
    }

    /// What a program wrote to its stdout: read into its CGI head until the head ends, then the response's body.
    /// Each part's credit is held until it is read into the head or sent on.
    pub(super) fn take_output(&mut self, part: Part<Reading>, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let Some(Taken { number, value, kept }) = self.outputs.part_held(turn, part) else {
            return Ok(());
        };
        let exchange = *kept;
        let Some(Exchange::Running(running)) = self.exchanges.get_mut(&exchange) else {
            return Ok(());
        };
        let head = match &mut running.output {
            Output::Head { head, .. } => head,
            Output::Headed { waiting, .. } | Output::Body { waiting, .. } => {
                waiting.push_back(value.bytes);
                return self.flush(exchange, turn);
            }
        };
        head.extend_from_slice(turn.read(&value.bytes));

        match cgi::head(head, MOST_HEAD_BYTES) {
            CgiHead::More => {
                self.outputs.grant(turn, number, NonZeroU32::MIN)?;
                Ok(())
            }
            CgiHead::TooLarge | CgiHead::Malformed => {
                turn.log(Level::Warn, format_args!("exchange {exchange}'s program wrote a head that does not read"));
                self.stop(exchange, Status::BadGateway, turn)
            }
            CgiHead::Whole { head, consumed } => self.headed(exchange, number, head, consumed, turn),
        }
    }

    /// An exchange's program's head is read, `consumed` bytes of what it wrote, ending in the part its stream `number`
    /// sent last: the response's head is kept, and what came past it is the first of its body.
    fn headed(
        &mut self,
        exchange: u64,
        number: Number,
        response: ResponseHead,
        consumed: usize,
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        let Some(Exchange::Running(mut running)) = self.exchanges.remove(&exchange) else {
            return Ok(());
        };
        let (held, mut written) = match running.output {
            Output::Head { held, head } => (held, head),
            output @ (Output::Headed { .. } | Output::Body { .. }) => {
                running.output = output;
                self.exchanges.insert(exchange, Exchange::Running(running));
                return Ok(());
            }
        };

        // The part the head ended in is granted back once what it holds past the head is sent on, as every part of the
        // body is; one that held nothing past it is done with now.
        let past = written.split_off(consumed);
        let mut waiting = VecDeque::new();
        let stored = if past.is_empty() {
            self.outputs.grant(turn, number, NonZeroU32::MIN)?;
            true
        } else {
            turn.put(past).map(|bytes| waiting.push_back(bytes)).is_ok()
        };
        running.output = Output::Headed { held, head: response, waiting };
        self.exchanges.insert(exchange, Exchange::Running(running));
        if !stored {
            turn.log(Level::Warn, format_args!("exchange {exchange}'s response could not be held, and it stops"));
            return self.stop(exchange, Status::InternalServerError, turn);
        }

        self.settle(exchange, turn)
    }

    /// An exchange's program's stdout has ended, as `how` says.
    pub(super) fn end_output(&mut self, exchange: u64, how: HungUp, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let Some(Exchange::Running(running)) = self.exchanges.get_mut(&exchange) else {
            return Ok(());
        };
        running.stdout = Stdout::Ended(how);

        self.settle(exchange, turn)
    }

    /// Bytes of a request's body, written to its program's stdin, its credit held until the write is sent: queued as
    /// long as a write is out, as one is out at a time.
    pub(super) fn take_body(&mut self, part: Part<InboundBody>, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let Some(Taken { value, kept, .. }) = self.bodies.part_held(turn, part) else {
            return Ok(());
        };
        let exchange = *kept;
        let Some(Exchange::Running(running)) = self.exchanges.get_mut(&exchange) else {
            return Ok(());
        };
        let Input::Writing { queued, sending, .. } = &mut running.input else {
            return Ok(());
        };
        queued.push(value.bytes);
        if *sending != Out::Idle {
            return Ok(());
        }

        self.write_queued(exchange, turn)
    }

    /// Writes what of an exchange's request's body is queued to its program's stdin, in one write.
    fn write_queued(&mut self, exchange: u64, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let Some(Exchange::Running(running)) = self.exchanges.get_mut(&exchange) else {
            return Ok(());
        };
        let Input::Writing { queued, sending, .. } = &mut running.input else {
            return Ok(());
        };
        let Some(parts) = NonZeroU32::new(u32::try_from(queued.len()).unwrap_or(u32::MAX)) else {
            return Ok(());
        };
        turn.to(running.pipe).ask(&Write { parts: mem::take(queued) }, &Writing { exchange })?;
        *sending = Out::Parts(parts);

        Ok(())
    }

    /// A write to an exchange's program's stdin is answered: sent, and its parts are granted back to the body's stream,
    /// and what was queued meanwhile written, or the program's stdin finished once the body is whole; not sent, and the
    /// rest of the body goes unwritten, as the program took no more.
    pub(super) fn written(
        &mut self,
        exchange: u64,
        answered: &Answered<Wrote>,
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        let Some(Exchange::Running(running)) = self.exchanges.get_mut(&exchange) else {
            return Ok(());
        };
        let Input::Writing { body, queued, sending } = &mut running.input else {
            return Ok(());
        };
        let Out::Parts(parts) = *sending else {
            return Ok(());
        };
        *sending = Out::Idle;
        let body = *body;
        let Answered::Yes(Wrote::Sent) = answered else {
            if let Body::Open(number) = body {
                self.bodies.stop(turn, number)?;
            }
            running.input = Input::Abandoned;
            return self.settle(exchange, turn);
        };

        if let Body::Open(number) = body {
            self.bodies.grant(turn, number, parts)?;
        }
        if !queued.is_empty() {
            return self.write_queued(exchange, turn);
        }
        if body == Body::Read {
            turn.to(running.pipe).send(&Finish)?;
            running.input = Input::Written;
        }

        self.settle(exchange, turn)
    }

    /// An exchange's request's body has been read: whole, and the program's stdin is finished once every byte is
    /// written; otherwise the client is gone or sent what does not read, and the exchange stops.
    pub(super) fn end_body(
        &mut self,
        exchange: u64,
        answered: &Answered<BodyRead>,
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        let Some(Exchange::Running(running)) = self.exchanges.get_mut(&exchange) else {
            return Ok(());
        };
        let Input::Writing { body, queued, sending } = &mut running.input else {
            return Ok(());
        };
        let Answered::Yes(BodyRead::Whole) = answered else {
            turn.log(
                Level::Info,
                format_args!("exchange {exchange}'s request's body was not read whole: {answered:?}"),
            );
            return self.stop(exchange, Status::BadRequest, turn);
        };
        *body = Body::Read;
        let is_idle = *sending == Out::Idle;
        let drained = is_idle && queued.is_empty();
        if drained {
            turn.to(running.pipe).send(&Finish)?;
            running.input = Input::Written;
        }

        self.settle(exchange, turn)
    }

    /// A stack opens the body of exchange `exchange`'s response as its stream `stream`: what waits is sent. A body no
    /// exchange waits to stream, as its exchange broke off or gave up waiting, is ended `Failed` at once.
    pub(super) fn opened(&mut self, exchange: u64, stream: StreamKey, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let downstream = match self.exchanges.get_mut(&exchange) {
            Some(Exchange::Running(Running { output: Output::Body { stream: downstream, .. }, .. })) => downstream,
            Some(Exchange::Starting { .. } | Exchange::Running(_) | Exchange::Stopping { .. }) | None => {
                return self.unserved(stream, turn);
            }
        };
        let Downstream::Unopened(unopened) = *downstream else {
            return self.unserved(stream, turn);
        };
        turn.cancel(unopened);
        *downstream = Downstream::Open(stream);

        self.flush(exchange, turn)
    }

    /// Ends the response's body the stack's stream `stream` reads, which nothing streams, `Failed`.
    fn unserved(&mut self, stream: StreamKey, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let Some(streaming) = self.responses.take(stream) else {
            return Ok(());
        };

        Ok(streaming.finish(turn, &SourceEnd::Failed)?)
    }

    /// Exchange `exchange`'s response has waited its time for its stack to open its body: unless it was opened, its
    /// program is stopped.
    pub(super) fn lapse(&mut self, exchange: u64, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let is_unopened = matches!(
            self.exchanges.get(&exchange),
            Some(Exchange::Running(Running { output: Output::Body { stream: Downstream::Unopened(_), .. }, .. }))
        );
        if !is_unopened {
            return Ok(());
        }
        turn.log(Level::Warn, format_args!("exchange {exchange}'s response was never read, and its program stops"));

        self.stop(exchange, Status::BadGateway, turn)
    }

    /// Sends what waits of exchange `exchange`'s response's body for as long as its stack's stream has credit,
    /// granting each part sent back to the program's stdout, then does what follows.
    pub(super) fn flush(&mut self, exchange: u64, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let Some(Exchange::Running(running)) = self.exchanges.get_mut(&exchange) else {
            return Ok(());
        };
        let Output::Body { stream: Downstream::Open(stream), waiting } = &mut running.output else {
            return self.settle(exchange, turn);
        };
        let Some(streaming) = self.responses.get_mut(*stream) else {
            return Ok(());
        };
        let mut sent = 0;
        while let Some(bytes) = waiting.pop_front() {
            match streaming.send(turn, ResponseBytes { bytes }) {
                Ok(()) => sent += 1,
                Err(Unsent::NoCredit(unsent)) => {
                    waiting.push_front(unsent.bytes);
                    break;
                }
                Err(Unsent::Unwritable(error)) => return Err(error.into()),
            }
        }
        let granted = match running.stdout {
            Stdout::Open(number) => NonZeroU32::new(sent).map(|sent| (number, sent)),
            Stdout::Ended(_) => None,
        };
        if let Some((number, sent)) = granted {
            self.outputs.grant(turn, number, sent)?;
        }

        self.settle(exchange, turn)
    }

    /// Does what exchange `exchange` does next, as where it stands says.
    fn settle(&mut self, exchange: u64, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let Some(Exchange::Running(running)) = self.exchanges.get(&exchange) else {
            return Ok(());
        };

        match running.next() {
            Next::Wait => Ok(()),
            Next::Unheaded => {
                turn.log(Level::Warn, format_args!("exchange {exchange}'s program ended before it wrote its head"));
                self.stop(exchange, Status::BadGateway, turn)
            }
            Next::Answer => self.answer(exchange, turn),
            Next::Finish(end) => self.finish(exchange, end, turn),
        }
    }

    /// Answers exchange `exchange`'s request with its program's head and a body streamed from here, of a length not
    /// known, and gives its stack `OPENED_WITHIN` to open the body. What is left of the request's body goes unwritten:
    /// the stack ends its stream `Cut` as it takes an answer before the body's end.
    fn answer(&mut self, exchange: u64, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let Some(Exchange::Running(mut running)) = self.exchanges.remove(&exchange) else {
            return Ok(());
        };
        let (held, response, waiting) = match running.output {
            Output::Headed { held, head, waiting } => (held, head, waiting),
            output @ (Output::Head { .. } | Output::Body { .. }) => {
                running.output = output;
                self.exchanges.insert(exchange, Exchange::Running(running));
                return Ok(());
            }
        };
        if let Input::Writing { .. } = running.input {
            running.input = Input::Abandoned;
        }

        let until = turn.now() + OPENED_WITHIN;
        let unopened = turn.run(&Sleep { until }, &Unopened { exchange })?;
        let source = turn.me().protocol();
        let body = ResponseBody::Streamed(StreamedBody { source, body: exchange, length: ResponseLength::Unknown });
        turn.answer(held, &Response { status: response.status, headers: response.headers, body })?;
        running.output = Output::Body { stream: Downstream::Unopened(unopened), waiting };
        self.exchanges.insert(exchange, Exchange::Running(running));

        Ok(())
    }

    /// Ends exchange `exchange`'s response's body as `end` says: its program has exited, and it is over.
    fn finish(&mut self, exchange: u64, end: SourceEnd, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let Some(Exchange::Running(running)) = self.exchanges.remove(&exchange) else {
            return Ok(());
        };
        if let Input::Writing { body: Body::Open(number), .. } = running.input {
            self.bodies.stop(turn, number)?;
        }
        let Output::Body { stream: Downstream::Open(stream), .. } = running.output else {
            return Ok(());
        };
        let Some(streaming) = self.responses.take(stream) else {
            return Ok(());
        };

        Ok(streaming.finish(turn, &end)?)
    }

    /// Breaks exchange `exchange` off: its program is stopped and its streams with it, and a request not yet answered
    /// is answered `status`, or a body being streamed ended `Failed`. It keeps its place until its program has exited.
    pub(super) fn stop(&mut self, exchange: u64, status: Status, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let running = match self.exchanges.remove(&exchange) {
            Some(Exchange::Running(running)) => running,
            Some(Exchange::Starting { held, .. }) => return Ok(turn.answer(held, &Response::empty(status))?),
            Some(stopping @ Exchange::Stopping { .. }) => {
                self.exchanges.insert(exchange, stopping);
                return Ok(());
            }
            None => return Ok(()),
        };
        if let Stdout::Open(number) = running.stdout {
            self.outputs.stop(turn, number)?;
        }
        if let Input::Writing { body: Body::Open(number), .. } = running.input {
            self.bodies.stop(turn, number)?;
        }
        match running.output {
            Output::Head { held, .. } | Output::Headed { held, .. } => turn.answer(held, &Response::empty(status))?,
            Output::Body { stream: Downstream::Open(stream), .. } => self.unserved(stream, turn)?,
            Output::Body { stream: Downstream::Unopened(unopened), .. } => turn.cancel(unopened),
        }
        if running.exit == Exit::Running {
            turn.to(running.process).send(&Stop)?;
            self.exchanges.insert(exchange, Exchange::Stopping { process: running.process });
        }

        Ok(())
    }

    /// The exchange whose response's body streams as the stack's stream `stream`.
    pub(super) fn exchange_streaming(&self, stream: StreamKey) -> Option<u64> {
        self.exchanges.iter().find(|(_, exchange)| exchange.streams(stream)).map(|(&number, _)| number)
    }
}

/// How exchange `exchange`'s program exited, as `exited` says: one that failed is logged with its stderr.
fn exited_well(exchange: u64, exited: Exited, turn: &mut Turn<'_, Git>) -> ExitStatus {
    let Exited { status, stderr } = exited;
    if status != ExitStatus::Code(0) {
        turn.log(Level::Warn, format_args!("exchange {exchange}'s program exited {status:?}: {stderr}"));
    }

    status
}
