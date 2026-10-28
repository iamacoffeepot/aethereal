//! The HTTP capability's root (ADR-0035).

use crate::{HttpSettings, Server, ServerConfig};
use aethereal_actor::{
    Actor, ActorRef, Allows, Answer, Answered, Asks, Beginning, Carries, ChildOf, Ended, Ending, Fault, Handles, Held,
    Path, ToSender, ToType, Top, Turn, path,
};
use aethereal_data::{Bound, Plain};
use aethereal_substrate::Bootstrap;
use aethereal_tcp::{Framing, Listen, Listener, Listening, Shut, Tcp};
use std::collections::BTreeMap;

/// What an actor asks the root to serve on: a port, 0 for any the host picks.
#[derive(Debug, Plain)]
#[kind(name = "aether.http.serve")]
pub struct Serve {
    pub port: u16,
}

/// What a serve came to.
#[derive(Debug, Bound)]
#[kind(name = "aether.http.served")]
pub enum Served {
    /// Bound: the server beneath the root, and the port bound, which is its key. Its handler opens `Requests` on it.
    Bound { server: ActorRef<Server>, port: u16 },
    /// Something holds the port.
    Taken,
    /// The root holds its most servers.
    Full,
    /// The host refused the bind.
    Refused,
    /// The program has no `Tcp`.
    Unavailable,
}

/// What the root keeps for a listen it asked of `Tcp`: which serve it was asked for.
#[derive(Debug, PartialEq, Eq, Plain)]
#[kind(name = "aether.http.binding")]
pub struct Binding {
    number: u64,
}

/// A serve the root holds until `Tcp` answers its listen, and the actor that asked it.
#[derive(Bound)]
struct Waiting {
    held: Held<Served>,
    asker: u64,
}

/// A server the root stood up, and the actor that asked for it.
#[derive(Bound)]
struct Opened {
    server: ActorRef<Server>,
    asker: u64,
}

/// The HTTP capability: serves HTTP/1.1 on `Tcp` for every other actor (ADR-0035). An actor asks it to [`Serve`] at
/// its typed path, `path::<Http>(())`, and opens [`Requests`](crate::Requests) on the server it is answered.
///
/// It watches each actor that asks, and when one ends, shuts every server it stood up for it.
#[derive(Bound, Bootstrap)]
#[kind(name = "aether.http")]
pub struct Http {
    settings: HttpSettings,
    servers: BTreeMap<u16, Opened>,
    waiting: BTreeMap<u64, Waiting>,
    /// The actors watched for having asked.
    askers: Vec<u64>,
    next: u64,
}

impl ChildOf for Http {
    type Parent = Top;
}

/// Asked at its typed path, it answers a serve once `Tcp` has answered its listen.
impl Carries<Serve> for Http {
    type Answers = (Served,);
}

impl Actor for Http {
    type Config = HttpSettings;
    type Key = ();
    type Handles = (Serve, Answer<Listening, Binding>, Ended);
    type BeginSends = ();
    type EndSends = ();

    fn begin(settings: HttpSettings, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(Self { settings, servers: BTreeMap::new(), waiting: BTreeMap::new(), askers: Vec::new(), next: 0 })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Serve> for Http {
    type Sends = (Asks<Path<Tcp>, Listen, Binding>, ToSender<Served>);

    fn handle(&mut self, serve: Serve, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let open = self.servers.len() + self.waiting.len();
        let is_full = open >= usize::try_from(self.settings.most_servers).unwrap_or(usize::MAX);
        if is_full {
            turn.reply(&Served::Full)?;
            return Ok(());
        }
        let held = turn.hold()?;
        let asker = self.watch_asker(turn);
        let number = self.next;
        self.next += 1;
        let listen = Listen { port: serve.port, framing: Framing::Chunks { most_bytes: self.settings.chunk_bytes } };
        turn.to(path::<Tcp>(())).ask(&listen, &Binding { number })?;
        self.waiting.insert(number, Waiting { held, asker });

        Ok(())
    }
}

impl Handles<Answer<Listening, Binding>> for Http {
    type Sends = (ToType<Listener, Shut>,);

    fn handle(
        &mut self,
        (answered, binding): (Answered<Listening>, Binding),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Some(Waiting { held, asker }) = self.waiting.remove(&binding.number) else {
            return Ok(());
        };
        let served = match answered {
            Answered::Yes(Listening::Bound { listener, port }) => self.stand_up(turn, listener, port, asker)?,
            Answered::Yes(Listening::Taken) => Served::Taken,
            Answered::Yes(Listening::Full | Listening::Refused) => Served::Refused,
            Answered::No(_) => Served::Unavailable,
        };
        turn.answer(held, &served)?;

        Ok(())
    }
}

impl Handles<Ended> for Http {
    type Sends = (ToType<Server, Shut>,);

    fn handle(&mut self, ended: Ended, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let gone = ended.actor().id().bits();
        self.servers.retain(|_, opened| opened.server.id().bits() != gone);
        let was_asker = self.askers.contains(&gone);
        if !was_asker {
            return Ok(());
        }
        self.askers.retain(|&asker| asker != gone);
        for opened in self.servers.values().filter(|opened| opened.asker == gone) {
            turn.to(opened.server).send(&Shut)?;
        }

        Ok(())
    }
}

impl Http {
    /// Stands a server up on the listener `Tcp` bound at `port` for `asker`, or shuts the listener and says why not.
    fn stand_up<S, I>(
        &mut self,
        turn: &mut Turn<'_, Self, S>,
        listener: ActorRef<Listener>,
        port: u16,
        asker: u64,
    ) -> Result<Served, Fault>
    where
        S: Allows<ToType<Listener, Shut>, I>,
    {
        let is_asked = self.askers.contains(&asker);
        if !is_asked {
            // Its asker has ended: no one is left to handle what the server would take.
            turn.to(listener).send::<_, I>(&Shut)?;
            return Ok(Served::Refused);
        }
        let config =
            ServerConfig { listener, most_exchanges: self.settings.most_exchanges, limits: self.settings.limits() };
        let Ok(server) = turn.spawn::<Server>(&u64::from(port), config) else {
            turn.to(listener).send::<_, I>(&Shut)?;
            return Ok(Served::Refused);
        };
        turn.watch(server);
        self.servers.insert(port, Opened { server, asker });

        Ok(Served::Bound { server, port })
    }

    /// Watches the sender of the mail handled, once, and answers who it is.
    fn watch_asker<S>(&mut self, turn: &mut Turn<'_, Self, S>) -> u64 {
        let asker = turn.sender();
        let id = asker.id().bits();
        let is_watched = self.askers.contains(&id);
        if !is_watched {
            turn.watch(asker);
            self.askers.push(id);
        }

        id
    }
}
