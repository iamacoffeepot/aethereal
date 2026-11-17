//! A stack served on the native host (ADR-0035): its pure layers, its handler, and its limits, to clients on threads of
//! their own over real loopback TCP.

use aethereal_actor::{
    Actor, Answer, Answered, Anyone, Asks, Beginning, Carries, ChildOf, Ending, Fault, Handles, Held, Level, Number,
    Part, Receives, Root, Slept, ToSender, Top, Turn,
};
use aethereal_chassis_headless::Headless;
use aethereal_chassis_headless::waiting::{self, BUDGET};
use aethereal_data::{Bound, Plain};
use aethereal_http::stack::{
    Answering, Binding, Credentials, Deadline, FromStack, Gather, Gathered, Gathering, Head, Layer, LayerForm, Layers,
    Leaving, Listened, Request, Response, Sending, Serving, Stack, StackSends, StackSettings, Step, layers,
};
use aethereal_http::{Header, Status};
use aethereal_substrate::harness::{Harness, Unheard};
use aethereal_substrate::{Bootstrap, Bootstrapping, EngineSettings, Host, Unbuilt};
use aethereal_tcp::{Accepting, HungUp, Listening, Reading, Shut, Tcp, Unbound, Wrote};
use std::io::Write as _;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

mod peer;

use peer::{Read, client, closes, connect, heard, response};

/// What [`Count`] attaches: the number of the request on its connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
struct Seen {
    request: u32,
}

/// A pure layer that counts its connection's requests, attaches each one's number, and writes it on the way out as
/// `x-request`.
#[derive(Debug, Default, Clone, Plain)]
struct Count {
    requests: u32,
}

impl LayerForm for Count {
    type Form = Self;
}

impl Layer for Count {
    type In = ();
    type Out = Seen;

    fn inbound(&mut self, _: &Head, (): ()) -> Step<Seen> {
        self.requests += 1;

        Step::Pass(Seen { request: self.requests })
    }

    fn outbound(&mut self, _: &Head, leaving: &mut Leaving<'_>) {
        leaving.head().headers.push(Header::new("x-request", &self.requests.to_string()));
    }
}

/// A pure layer that passes a request bearing the token `open` and answers any other with a challenge.
#[derive(Debug, Default, Clone, Plain)]
struct Admit;

impl LayerForm for Admit {
    type Form = Self;
}

impl Layer for Admit {
    type In = Seen;
    type Out = Seen;

    fn inbound(&mut self, head: &Head, seen: Seen) -> Step<Seen> {
        match &head.credentials {
            Credentials::Bearer { token } if token == "open" => Step::Pass(seen),
            _ => Step::Answer(Response::challenge("Bearer", "test")),
        }
    }
}

/// [`Echo`]'s settings: the most bytes of a body it gathers.
#[derive(Debug, Default, Plain)]
#[kind(name = "test.http_stack.echo_settings")]
#[settings]
struct EchoSettings {
    most_bytes: u32,
}

/// A handler that gathers each request's body and answers it with the body, and the request's number in
/// `x-attached`; it counts the requests it is asked.
#[derive(Bound, Bootstrap)]
#[kind(name = "test.http_stack.echo")]
struct Echo {
    most_bytes: u32,
    asked: u32,
}

/// What [`Echo`] keeps while its request's body is gathered: the request, held, and its number.
#[derive(Bound)]
#[kind(name = "test.http_stack.echoing")]
struct Echoing {
    held: Held<Response>,
    request: u32,
}

impl ChildOf for Echo {
    type Parent = Top;
}

impl Actor for Echo {
    type Config = EchoSettings;
    type Key = ();
    type Handles = (Request<Seen, ()>, Answer<Gathered, Echoing>);
    type BeginSends = ();
    type EndSends = ();

    fn begin(settings: EchoSettings, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(Self { most_bytes: settings.most_bytes, asked: 0 })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Carries<Request<Seen, ()>> for Echo {
    type Answers = (Response,);
}

impl Handles<Request<Seen, ()>> for Echo {
    type Sends = (ToSender<Response>, Asks<Gathering, Gather, Echoing>);
    type From = FromStack;

    fn handle(
        &mut self,
        request: Request<Seen, ()>,
        turn: &mut Turn<'_, Self, Self::Sends, Self::From>,
    ) -> Result<(), Fault> {
        self.asked += 1;
        let echoing = Echoing { held: turn.hold()?, request: request.attached.request };
        turn.to(request.body.gatherer).ask(&request.body.whole(self.most_bytes), &echoing)?;

        Ok(())
    }
}

impl Receives<Answer<Gathered, Echoing>> for Echo {
    type Sends = ();

    fn handle(
        &mut self,
        (gathered, Echoing { held, request }): (Answered<Gathered>, Echoing),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let response = match gathered {
            Answered::Yes(Gathered::Whole(body)) => {
                let mut echoed = Response::new(Status::Ok, "text/plain", body);
                echoed.headers.push(Header::new("x-attached", &request.to_string()));
                echoed
            }
            Answered::Yes(Gathered::TooLarge) => Response::empty(Status::ContentTooLarge),
            Answered::Yes(_) | Answered::No(_) => Response::empty(Status::BadRequest),
        };

        Ok(turn.answer(held, &response)?)
    }
}

/// [`Silent`]'s settings: none.
#[derive(Debug, Default, Plain)]
#[kind(name = "test.http_stack.silent_settings")]
#[settings]
struct SilentSettings {}

/// A handler that holds every request it is asked, and never answers.
#[derive(Bound, Bootstrap)]
#[kind(name = "test.http_stack.silent")]
struct Silent {
    held: Vec<Held<Response>>,
}

impl ChildOf for Silent {
    type Parent = Top;
}

impl Actor for Silent {
    type Config = SilentSettings;
    type Key = ();
    type Handles = (Request<(), ()>,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: SilentSettings, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(Self { held: Vec::new() })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Carries<Request<(), ()>> for Silent {
    type Answers = (Response,);
}

impl Handles<Request<(), ()>> for Silent {
    type Sends = (ToSender<Response>,);
    type From = FromStack;

    fn handle(&mut self, _: Request<(), ()>, turn: &mut Turn<'_, Self, Self::Sends, Self::From>) -> Result<(), Fault> {
        self.held.push(turn.hold()?);

        Ok(())
    }
}

/// A stack named `$stack`, of the kind `$name`, its settings the section `$section`, whose layers are `$layers`: each of
/// its handlers of HTTP's and TCP's kinds one call into its [`Serving`].
macro_rules! stack {
    ($stack:ident, $name:tt, $section:tt, $layers:ty) => {
        #[derive(Bound)]
        #[kind(name = $name)]
        struct $stack {
            serving: Serving<Self, $layers>,
        }

        impl ChildOf for $stack {
            type Parent = Top;
        }

        impl<H: Host> Bootstrap<H> for $stack {
            const SECTION: &'static str = $section;
            type Settings = StackSettings;
            type Needs = ();
            type Provides = ();

            async fn bootstrap(
                settings: StackSettings,
                (): (),
                cx: &mut Bootstrapping<'_, H, Self>,
            ) -> Result<(), Unbuilt> {
                settings.fit(cx)?;

                cx.begin(&(), settings)
            }
        }

        impl Actor for $stack {
            type Config = StackSettings;
            type Key = ();
            type Handles = (
                Answer<Listening, Binding>,
                Part<Accepting>,
                Answer<Unbound, Number>,
                Part<Reading>,
                Answer<HungUp, Number>,
                Answer<Wrote, Sending>,
                Answer<Response, Answering>,
                Answer<Slept, Deadline>,
                Gather,
                Shut,
            );
            type BeginSends = StackSends<$layers>;
            type EndSends = ();

            fn begin(settings: StackSettings, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
                let layers: Layers<$layers> = layers(turn)?;

                Ok(Self { serving: Serving::listen(turn, &settings, layers)? })
            }

            fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
                Ok(())
            }
        }

        impl Stack for $stack {}

        impl Receives<Answer<Listening, Binding>> for $stack {
            type Sends = StackSends<$layers>;

            fn handle(
                &mut self,
                listening: (Answered<Listening>, Binding),
                turn: &mut Turn<'_, Self, Self::Sends>,
            ) -> Result<(), Fault> {
                let listened = self.serving.listened(listening, turn)?;
                if let Listened::Refused(_) | Listened::Unanswered(_) = listened {
                    turn.log(Level::Warn, format!("the stack serves nothing: {listened:?}"));
                }

                Ok(())
            }
        }

        impl Handles<Part<Accepting>> for $stack {
            type Sends = StackSends<$layers>;
            type From = Anyone;

            fn handle(&mut self, part: Part<Accepting>, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
                self.serving.accept(part, turn)
            }
        }

        impl Receives<Answer<Unbound, Number>> for $stack {
            type Sends = StackSends<$layers>;

            fn handle(
                &mut self,
                unbound: (Answered<Unbound>, Number),
                turn: &mut Turn<'_, Self, Self::Sends>,
            ) -> Result<(), Fault> {
                self.serving.unbound(unbound, turn)
            }
        }

        impl Handles<Part<Reading>> for $stack {
            type Sends = StackSends<$layers>;
            type From = Anyone;

            fn handle(&mut self, part: Part<Reading>, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
                self.serving.read(part, turn)
            }
        }

        impl Receives<Answer<HungUp, Number>> for $stack {
            type Sends = StackSends<$layers>;

            fn handle(
                &mut self,
                hung_up: (Answered<HungUp>, Number),
                turn: &mut Turn<'_, Self, Self::Sends>,
            ) -> Result<(), Fault> {
                self.serving.hung_up(hung_up, turn)
            }
        }

        impl Receives<Answer<Wrote, Sending>> for $stack {
            type Sends = StackSends<$layers>;

            fn handle(
                &mut self,
                wrote: (Answered<Wrote>, Sending),
                turn: &mut Turn<'_, Self, Self::Sends>,
            ) -> Result<(), Fault> {
                self.serving.wrote(wrote, turn)
            }
        }

        impl Receives<Answer<Response, Answering>> for $stack {
            type Sends = StackSends<$layers>;

            fn handle(
                &mut self,
                answered: (Answered<Response>, Answering),
                turn: &mut Turn<'_, Self, Self::Sends>,
            ) -> Result<(), Fault> {
                self.serving.answered(answered, turn)
            }
        }

        impl Receives<Answer<Slept, Deadline>> for $stack {
            type Sends = StackSends<$layers>;

            fn handle(
                &mut self,
                slept: (Answered<Slept>, Deadline),
                turn: &mut Turn<'_, Self, Self::Sends>,
            ) -> Result<(), Fault> {
                self.serving.slept(slept, turn)
            }
        }

        impl Handles<Gather> for $stack {
            type Sends = StackSends<$layers>;
            type From = Anyone;

            fn handle(&mut self, gather: Gather, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
                self.serving.gather(gather, turn)
            }
        }

        impl Handles<Shut> for $stack {
            type Sends = StackSends<$layers>;
            type From = Anyone;

            fn handle(&mut self, _: Shut, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
                self.serving.shut(turn)
            }
        }
    };
}

stack!(EchoStack, "test.http_stack.echo_stack", "echo_stack", (Count, Admit, Echo));
stack!(SilentStack, "test.http_stack.silent_stack", "silent_stack", (Silent,));

/// The engine's settings for a stack of `settings`: a head of its most bytes is read whole.
fn engine(settings: &StackSettings) -> EngineSettings {
    EngineSettings {
        read_limit: settings.most_read_bytes().try_into().unwrap_or(u32::MAX),
        ..EngineSettings::default()
    }
}

/// The port `stack` listens on, the harness stepping the host until `Tcp` has bound it.
fn bound<S: Actor<Key = ()> + Root>(harness: &mut Harness<Headless>, port: impl Fn(&S) -> Option<u16>) -> u16 {
    let stack = harness.root::<S>();
    let mut bound = None;
    waiting::until(BUDGET, || {
        harness.step();
        bound = harness.state(stack).ok().and_then(&port);

        bound.is_some()
    });
    let Some(port) = bound else {
        panic!("the stack binds its port");
    };

    port
}

/// A host serving an [`EchoStack`] of `settings`, whose handler gathers at most `most_bytes`, and the port it listens
/// on.
fn echoing(settings: StackSettings, most_bytes: u32) -> Result<(Harness<Headless>, u16), Unheard> {
    let program = Headless::program()
        .with_engine(engine(&settings))
        .root::<Tcp>()
        .root_with::<Echo>(EchoSettings { most_bytes })
        .root_with::<EchoStack>(settings);
    let mut harness = Harness::of(program)?;
    let port = bound(&mut harness, |stack: &EchoStack| stack.serving.port());

    Ok((harness, port))
}

/// A client on a thread of its own that writes each of `requests` once the response to the one before has come, on one
/// connection, and reads each response, and whether the server then closed.
fn conversation(port: u16, requests: &'static [&'static [u8]]) -> JoinHandle<(Vec<Read>, bool)> {
    #[allow(clippy::disallowed_methods)] // The client is a native thread of its own, outside the engine.
    thread::spawn(move || {
        let mut stream = connect(port);
        let mut buffer = Vec::new();
        let read = requests
            .iter()
            .map(|request| {
                assert!(stream.write_all(request).is_ok(), "the client writes");
                response(&mut stream, &mut buffer)
            })
            .collect();
        let closed = closes(&mut stream);

        (read, closed)
    })
}

#[test]
fn a_request_passes_each_layer_to_its_handler_and_the_response_passes_them_back() -> Result<(), Unheard> {
    // Catches what one layer attaches not handed to the next, or to the handler; a layer's way out not run, or run on
    // a response that skips it; a body not gathered for the handler; a response's `Content-Length` not its body's;
    // and the credentials handed on to the handler among its headers.
    let (mut harness, port) = echoing(StackSettings::default(), 64)?;
    let sent =
        b"POST /echo HTTP/1.1\r\nAuthorization: Bearer open\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello";
    let client = client(port, sent, 1);

    let (answered, closed) = heard(&mut harness, client);

    assert!(answered[0].head.starts_with("HTTP/1.1 200 OK"), "{answered:?}");
    assert!(answered[0].head.contains("x-attached: 1"), "{answered:?}");
    assert!(answered[0].head.contains("x-request: 1"), "{answered:?}");
    assert_eq!(answered[0].body, b"hello");
    assert!(closed, "a request that asked to close is closed once answered");

    Ok(())
}

#[test]
fn a_layer_that_answers_turns_the_request_back_through_the_layers_before_it_and_its_handler_is_not_asked()
-> Result<(), Unheard> {
    // Catches a request a layer answered handed on to the handler anyway; the answer not passed back out through the
    // layers before the one that answered; an empty body written with no `Content-Length`, so the client waits for
    // one; and a body a layer answered before left in the connection, to be read as the next request.
    let (mut harness, port) = echoing(StackSettings::default(), 64)?;
    let client = client(port, b"POST /echo HTTP/1.1\r\nContent-Length: 4\r\n\r\nbody", 1);

    let (refused, closed) = heard(&mut harness, client);

    assert!(refused[0].head.starts_with("HTTP/1.1 401"), "{refused:?}");
    assert!(refused[0].head.contains("www-authenticate: Bearer realm=\"test\""), "{refused:?}");
    assert!(refused[0].head.contains("x-request: 1"), "{refused:?}");
    assert!(refused[0].head.contains("content-length: 0"), "{refused:?}");
    assert!(closed, "a request answered with its body unread closes its connection");
    let echo = harness.root::<Echo>();
    assert_eq!(harness.state(echo)?.asked, 0, "the handler is not asked a request a layer answered");

    Ok(())
}

#[test]
fn a_connection_kept_alive_keeps_its_layers_values_and_a_new_one_starts_from_the_template() -> Result<(), Unheard> {
    // Catches a connection closed after its first response though it asked to stay; a pure layer's value made anew for
    // each request, or shared between connections, so what it keeps across one connection's requests is lost or
    // leaks into another's.
    const FIRST: &[u8] = b"GET /one HTTP/1.1\r\nAuthorization: Bearer open\r\n\r\n";
    const SECOND: &[u8] = b"GET /two HTTP/1.1\r\nAuthorization: Bearer open\r\nConnection: close\r\n\r\n";
    let (mut harness, port) = echoing(StackSettings::default(), 64)?;

    let (kept, closed) = heard(&mut harness, conversation(port, &[FIRST, SECOND]));
    let (fresh, _) = heard(&mut harness, client(port, SECOND, 1));

    assert!(kept[0].head.contains("x-request: 1"), "{kept:?}");
    assert!(kept[1].head.contains("x-request: 2"), "{kept:?}");
    assert!(closed, "the second request asked to close");
    assert!(fresh[0].head.contains("x-request: 1"), "{fresh:?}");

    Ok(())
}

#[test]
fn requests_sent_ahead_are_answered_in_order_once_the_one_before_is_written() -> Result<(), Unheard> {
    // Catches a body framed past its `Content-Length` into the next request; bytes read past a request lost; credit
    // withheld while a request is out and never granted back, so the second request is never read; and responses
    // written out of order.
    // Chunks of 16 bytes, one granted at a time, so the second request is read only once the credit withheld while the
    // first was out is granted back.
    let settings = StackSettings { chunk_bytes: 16, window: 1, ..StackSettings::default() };
    let (mut harness, port) = echoing(settings, 64)?;
    let ahead = b"POST /one HTTP/1.1\r\nAuthorization: Bearer open\r\nContent-Length: 5\r\n\r\nhello\
        GET /two HTTP/1.1\r\nAuthorization: Bearer open\r\nConnection: close\r\n\r\n";
    let client = client(port, ahead, 2);

    let (answered, closed) = heard(&mut harness, client);

    assert_eq!(answered[0].body, b"hello", "{answered:?}");
    assert!(answered[0].head.contains("x-request: 1"), "{answered:?}");
    assert!(answered[1].body.is_empty(), "{answered:?}");
    assert!(answered[1].head.contains("x-request: 2"), "{answered:?}");
    assert!(closed, "the second request asked to close");

    Ok(())
}

#[test]
fn a_body_framed_two_ways_is_refused_before_its_layers() -> Result<(), Unheard> {
    // Catches a `Transfer-Encoding` beside a `Content-Length` settled in favour of one, which is how a request is
    // smuggled past a reader that settles it the other way, and the refused request handed to the layers.
    let (mut harness, port) = echoing(StackSettings::default(), 64)?;
    let sent =
        b"POST /echo HTTP/1.1\r\nAuthorization: Bearer open\r\nContent-Length: 5\r\nTransfer-Encoding: chunked\r\n\r\n\
        5\r\nhello\r\n0\r\n\r\n";
    let client = client(port, sent, 1);

    let (refused, closed) = heard(&mut harness, client);

    assert!(refused[0].head.starts_with("HTTP/1.1 400"), "{refused:?}");
    assert!(!refused[0].head.contains("x-request"), "no layer saw it: {refused:?}");
    assert!(closed, "a refused request's connection closes once its response is sent");

    Ok(())
}

#[test]
fn a_head_past_its_most_bytes_is_refused_before_it_is_whole() -> Result<(), Unheard> {
    // Catches a head bounded only once it ends, so a client that never ends one is buffered without limit, and a head
    // too large answered as one that does not parse.
    let settings = StackSettings { most_head_bytes: 64, ..StackSettings::default() };
    let (mut harness, port) = echoing(settings, 64)?;
    let client = client(port, b"GET /echo HTTP/1.1\r\nx-padding: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", 1);

    let (refused, closed) = heard(&mut harness, client);

    assert!(refused[0].head.starts_with("HTTP/1.1 431"), "{refused:?}");
    assert!(closed, "a refused request's connection closes once its response is sent");

    Ok(())
}

#[test]
fn a_handler_that_never_answers_is_timed_out_once_its_body_is_read() -> Result<(), Unheard> {
    // Catches a handler that never answers left waiting without a `504`, and a `504` written before
    // `answer_within_millis` has passed.
    let settings = StackSettings { answer_within_millis: 200, ..StackSettings::default() };
    let program = Headless::program()
        .with_engine(engine(&settings))
        .root::<Tcp>()
        .root::<Silent>()
        .root_with::<SilentStack>(settings);
    let mut harness = Harness::of(program)?;
    let port = bound(&mut harness, |stack: &SilentStack| stack.serving.port());
    #[allow(clippy::disallowed_methods)] // The test times the request against its deadline on real time.
    let sent = Instant::now();

    let (timed_out, closed) = heard(&mut harness, client(port, b"GET /silent HTTP/1.1\r\n\r\n", 1));

    assert!(timed_out[0].head.starts_with("HTTP/1.1 504"), "{timed_out:?}");
    assert!(sent.elapsed() >= Duration::from_millis(200), "it waited for its deadline: {:?}", sent.elapsed());
    assert!(closed, "a request timed out closes its connection");

    Ok(())
}

#[test]
fn a_body_gathered_past_its_most_bytes_is_refused_whole_and_the_connection_closes() -> Result<(), Unheard> {
    // Catches a body whose `Content-Length` is past the gather's most bytes read before it is refused; a chunked body,
    // which has no length to check, gathered past them and handed on, or handed on cut short; and the connection kept
    // with the rest of the body unread, to be parsed as the next request.
    let (mut harness, port) = echoing(StackSettings::default(), 8)?;
    let sized = b"POST /echo HTTP/1.1\r\nAuthorization: Bearer open\r\nContent-Length: 10\r\n\r\n0123456789";
    let chunked = b"POST /echo HTTP/1.1\r\nAuthorization: Bearer open\r\nTransfer-Encoding: chunked\r\n\r\n\
        5\r\nhello\r\n5\r\nworld\r\n0\r\n\r\n";

    let (sized, sized_closed) = heard(&mut harness, client(port, sized, 1));
    let (chunked, chunked_closed) = heard(&mut harness, client(port, chunked, 1));

    assert!(sized[0].head.starts_with("HTTP/1.1 413"), "{sized:?}");
    assert!(sized_closed, "the rest of a body refused is not read, so its connection closes");
    assert!(chunked[0].head.starts_with("HTTP/1.1 413"), "{chunked:?}");
    assert!(chunked_closed, "the rest of a body refused is not read, so its connection closes");

    Ok(())
}
