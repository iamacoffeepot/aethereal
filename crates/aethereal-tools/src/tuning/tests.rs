//! The tuning tool's tests: a root's tuning listed as JSON with what each field does, and the fields a `tune` names
//! set, every other kept, each checked by the tuning's schema and then by the actor.

use super::*;
use aethereal_actor::{Top, tuning};
use aethereal_substrate::Engine;
use aethereal_substrate::harness::{Bench, Harness, Unheard};
use std::num::NonZeroUsize;

/// A dial's tuning.
#[derive(Plain, Debug, Clone, PartialEq)]
#[kind(name = "aether.test.dial_tuning")]
#[settings]
struct DialTuning {
    /// How far the dial is turned: at most 10.
    level: u32,
    /// What the dial is called.
    name: String,
    /// The marks along the dial.
    marks: Vec<u32>,
}

/// A root tuned by a [`DialTuning`], which refuses a level past 10.
#[derive(Plain)]
#[kind(name = "aether.test.dial")]
struct Dial {
    level: u32,
    name: String,
    marks: Vec<u32>,
}

/// What a dial is made from: nothing.
#[derive(Plain, Default)]
#[kind(name = "aether.test.dial_config")]
struct DialConfig;

impl ChildOf for Dial {
    type Parent = Top;
}

impl Actor for Dial {
    type Config = DialConfig;
    type Key = ();
    type Handles = (ReadTuning, Tune);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: DialConfig, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(Self { level: 3, name: String::from("gain"), marks: vec![0, 5, 10] })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Tunable for Dial {
    type Tuning = DialTuning;

    fn tuning(&self) -> DialTuning {
        DialTuning { level: self.level, name: self.name.clone(), marks: self.marks.clone() }
    }

    fn tune(&mut self, tuning: DialTuning) -> Result<(), String> {
        if tuning.level > 10 {
            return Err(String::from("`level` is at most 10"));
        }
        (self.level, self.name, self.marks) = (tuning.level, tuning.name, tuning.marks);

        Ok(())
    }

    fn tunes(me: ActorRef<Self>) -> ProtocolRef<Tunes> {
        me.protocol()
    }
}

impl Handles<ReadTuning> for Dial {
    type Sends = (ToSender<Tuning>,);

    fn handle(&mut self, _: ReadTuning, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        tuning::read(self, turn)
    }
}

impl Handles<Tune> for Dial {
    type Sends = (ToSender<Tuned>,);

    fn handle(&mut self, tune: Tune, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        tuning::tune(self, &tune, turn)
    }
}

/// The tool over the one dial.
type Tuner = TuningTool<(Dial,)>;

/// A harness over an engine with the dial, and the tool over it.
fn tuned() -> Result<Harness<Bench>, Unheard> {
    let setup = Engine::new(NonZeroUsize::MIN, 1 << 16, 0, 1 << 20, 0).with_cycle::<()>();
    let engine = setup.add_root::<Dial>(&(), DialConfig)?.add_root::<Tuner>(&(), TuningToolConfig)?.ready();

    Harness::bare(engine)
}

/// Calls `tool` with `input`, and answers what it answered: its value, or why it refused.
fn call<K: Plain>(harness: &mut Harness<Bench>, tool: &str, input: &K) -> Result<Result<ActorTuning, String>, Unheard> {
    let input = harness.put(to_bytes(input)?)?;
    let worker = Tuner::worker(harness.root::<Tuner>());
    let asked = harness.ask(worker, &Call { tool: String::from(tool), input })?;
    let called = match harness.answer(asked)? {
        Answered::Yes(Called::Output(output)) => {
            let bytes = harness.read(&output).map(<[u8]>::to_vec).unwrap_or_default();
            let read = from_bytes::<ActorTuning>(&bytes, &mut Budget::new(JSON_BYTES));
            let listed = from_bytes::<TuningList>(&bytes, &mut Budget::new(JSON_BYTES));
            match (read, listed) {
                (Ok(about), _) => Ok(about),
                (_, Ok(TuningList { mut actors })) if actors.len() == 1 => Ok(actors.remove(0)),
                _ => Err(String::from("not an answer the tool gives")),
            }
        }
        Answered::Yes(Called::Refused(why)) => Err(why),
        Answered::Yes(Called::Image { .. }) | Answered::No(_) => Err(String::from("not an answer the tool gives")),
    };

    Ok(called)
}

/// What `tune` is given to set `values`, JSON, on the dial.
fn tune(values: &str) -> TuneFields {
    let Ok(values) = Json::parse(values, &mut Budget::new(INPUT_BYTES)) else {
        panic!("the test's values are JSON");
    };

    TuneFields { actor: path::<Dial>(()).names().clone(), values }
}

/// The dial's tuning as `list_tuning` and `tune` answer it, standing at `tuning`.
fn standing(tuning: &DialTuning) -> ActorTuning {
    let schema = SchemaNodes::of(&DialTuning::SHAPE);
    let mut budget = Budget::new(JSON_BYTES);
    let Ok(bytes) = to_bytes(tuning) else {
        panic!("the dial's tuning is written");
    };
    let (Ok(values), Ok(described)) = (to_json(&schema, &bytes, &mut budget), json_schema(&schema, &mut budget)) else {
        panic!("the dial's tuning and its schema are written as JSON");
    };
    let (Ok(values), Ok(described)) = (Json::parse(values, &mut budget), Json::parse(described, &mut budget)) else {
        panic!("what is written is JSON");
    };
    let fields = vec![
        About { name: String::from("level"), doc: String::from("How far the dial is turned: at most 10.") },
        About { name: String::from("name"), doc: String::from("What the dial is called.") },
        About { name: String::from("marks"), doc: String::from("The marks along the dial.") },
    ];

    ActorTuning { actor: path::<Dial>(()).names().clone(), values, schema: described, fields }
}

#[test]
fn a_listed_root_s_tuning_is_answered_as_json_with_its_schema_and_each_field_s_doc() -> Result<(), Unheard> {
    // Catches a root's tuning read with another schema than its own, its fields' docs left out or out of order, and a
    // list answered before the root has answered.
    let mut harness = tuned()?;
    let gain = DialTuning { level: 3, name: String::from("gain"), marks: vec![0, 5, 10] };
    assert_eq!(call(&mut harness, "list_tuning", &ListTuning)?, Ok(standing(&gain)));

    Ok(())
}

#[test]
fn tune_sets_only_the_fields_it_is_given_and_answers_the_tuning_as_it_then_stands() -> Result<(), Unheard> {
    // Catches the fields left out reset to their defaults or dropped, a field given not replacing what stood, a list
    // given as a value written wrong as it is laid over, and the answer read before the tuning was taken.
    let mut harness = tuned()?;
    let set = call(&mut harness, "tune", &tune(r#"{"level": 7, "marks": [1, 2]}"#))?;
    let tuned = DialTuning { level: 7, name: String::from("gain"), marks: vec![1, 2] };
    assert_eq!(set, Ok(standing(&tuned)));
    assert_eq!(call(&mut harness, "list_tuning", &ListTuning)?, Ok(standing(&tuned)));

    Ok(())
}

#[test]
fn a_tune_the_schema_or_the_actor_refuses_leaves_the_tuning_as_it_stood() -> Result<(), Unheard> {
    // Catches a field the tuning has not, a value of the wrong shape, or a value the actor refuses being taken in
    // part, and a tune of an actor the tool does not list being sent anywhere.
    let mut harness = tuned()?;
    let unknown = call(&mut harness, "tune", &tune(r#"{"volume": 2}"#))?;
    assert!(unknown.is_err_and(|why| why.contains("volume")));
    let shaped = call(&mut harness, "tune", &tune(r#"{"level": "high"}"#))?;
    assert!(shaped.is_err_and(|why| why.contains("level")));
    let past = call(&mut harness, "tune", &tune(r#"{"level": 11, "name": "loud"}"#))?;
    assert_eq!(past, Err(String::from("`level` is at most 10")));
    let elsewhere = TuneFields { actor: ActorPath::new("aether.test.knob"), ..tune("{}") };
    assert!(call(&mut harness, "tune", &elsewhere)?.is_err_and(|why| why.contains("aether.test.dial")));

    let gain = DialTuning { level: 3, name: String::from("gain"), marks: vec![0, 5, 10] };
    assert_eq!(call(&mut harness, "list_tuning", &ListTuning)?, Ok(standing(&gain)));

    Ok(())
}
