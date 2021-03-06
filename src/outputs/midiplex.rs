use std::iter::FromIterator;
use std::collections::VecDeque;
use indexmap::IndexMap;
use types::*;
use outputs::Output;

#[derive(Debug, PartialEq, Eq)]
struct State<Output> {
  velocity: u8,
  target_allocation: usize,
  outputs: VecDeque<Output>,
}

pub struct Midiplexer<O: Output> {
  notes           : IndexMap<(Note, Channel), State<O>>,
  unused          : Vec<VecDeque<O>>,
  unallocated     : VecDeque<O>,
  num_outputs     : usize,
  total_velocity  : usize,
  max_allocation  : Option<usize>,
}

impl<O: Output> FromIterator<O> for Midiplexer<O> {
  fn from_iter<I: IntoIterator<Item=O>>(iter: I) -> Self {
    Midiplexer::new(iter, None)
  }
}

impl<O: Output> Midiplexer<O> {
  pub fn new<I>(iter: I, max_allocation: Option<usize>) -> Self
    where I: IntoIterator<Item=O>
  {
    let notes       = IndexMap::with_capacity(88);
    let unallocated = iter.into_iter().collect::<VecDeque<_>>();
    let num_outputs = unallocated.len();
    let unused      = Vec::with_capacity(num_outputs);
    Midiplexer {
      notes,
      unused,
      unallocated,
      num_outputs,
      total_velocity: 0,
      max_allocation,
    }
  }

  /// Adjust the note allocation.
  fn adjust(&mut self)
     -> Result<(), O::Error>
  {
    let num_outputs     = self.num_outputs;
    let max_allocation  = self.max_allocation;
    let total_velocity  = self.total_velocity;

    let scale =
      max_allocation
        .filter(|max_allocation| max_allocation * total_velocity < num_outputs * 128)
        .map(|max_allocation| max_allocation as f32 / 127.)
        .unwrap_or(num_outputs as f32 / total_velocity as f32);

    let mut remaining = num_outputs;

    for (&(note, channel), status) in self.notes.iter_mut().rev()
    {
      // first, we'll compute an ideal allocation of resources
      status.target_allocation =
        ((status.velocity as f32 * scale) as usize)
          .max(1).min(remaining);

      remaining -= status.target_allocation;

      if status.outputs.len() < status.target_allocation {
        continue;
      }

      // then, while the note is over-allocated...
      let delta = status.outputs.len() - status.target_allocation;
      // ... remove each un-needed output from the allocation of this note
      for mut output in status.outputs.drain(..delta)
      {
        // turn off this note for this output
        output.off(note, channel)?;
        // add the output to the set of unallocated outputs
        self.unallocated.push_back(output);
      }
    }

    // finally, we'll reallocate the freed-up notes
    for (&(note, channel), status) in self.notes.iter_mut().rev()
    {
      if status.outputs.len() > status.target_allocation
      {
        continue;
      }

      // while the note is under-allocated...
      let delta =
        self.unallocated.len()
          .min(status.target_allocation - status.outputs.len());

      for mut output in self.unallocated.drain(..delta)
      {
        // turn the note on for this output
        output.on(note, channel, status.velocity)?;
        // add the output to the set of outputs associated with this note
        status.outputs.push_back(output);
      }
    }

    Ok(())
  }
}

impl<O: Output> Output for Midiplexer<O>
{
  type Error = O::Error;

  fn on(&mut self, note: Note, channel: Channel, velocity: Velocity)
       -> Result<(), O::Error>
  {
    let readjust =
      {
        let num_outputs = self.num_outputs;
        let unused = &mut self.unused;
        let note =
          self.notes.entry((note, channel))
            .or_insert_with(||
              State { velocity:0, target_allocation:0,
                      outputs: unused.pop()
                        .unwrap_or_else(|| VecDeque::with_capacity(num_outputs))
                    });
        let readjust = note.velocity != velocity;
        self.total_velocity -= note.velocity as usize;
        note.velocity        = velocity;
        self.total_velocity += note.velocity as usize;
        readjust
      };

    if readjust {
      self.adjust()?;
    }

    Ok(())
  }

  fn off(&mut self, note: Note, channel: Channel)
       -> Result<(), O::Error>
  {
    if let Some(mut status) = self.notes.remove(&(note, channel)) {
      self.total_velocity -= status.velocity as usize;
      for mut output in status.outputs.drain(..) {
        output.off(note, channel)?;
        self.unallocated.push_back(output);
      }
      self.unused.push(status.outputs);
      self.adjust()?
    }
    Ok(())
  }

  fn silence(&mut self)
      -> Result<(), O::Error>
  {
    for ((note,channel), mut status) in self.notes.drain(..) {
      while let Some(mut output) = status.outputs.pop_front() {
        output.off(note, channel)?;
        self.total_velocity -= status.velocity as usize;
        self.unallocated.push_back(output);
      }
      self.unused.push(status.outputs);
    }
    Ok(())
  }
}

#[test]
fn unbounded_allocation()
{
  let mut midiplexer =
    Midiplexer::new(["a", "b", "c", "d"].iter().cloned(), None);

  // 1 note on
  midiplexer.on(0, 0, 127).unwrap();
  assert_eq!(midiplexer.notes,
    indexmap!{
      (0,0) =>
        State {
          velocity: 127,
          target_allocation: 4,
          outputs: vec!["a", "b", "c", "d"].into()
        }
    });

  // 2 notes on
  midiplexer.on(0, 1, 127).unwrap();
  assert_eq!(midiplexer.notes,
    indexmap!{
      (0,0) =>
        State {
          velocity: 127,
          target_allocation: 2,
          outputs: vec!["c", "d"].into()
        },
      (0,1) =>
        State {
          velocity: 127,
          target_allocation: 2,
          outputs: vec!["a", "b",].into()
        }
    });

  // 3 notes on
  midiplexer.on(0, 2, 127).unwrap();
  assert_eq!(midiplexer.notes,
    indexmap!{
      (0,0) =>
        State {
          velocity: 127,
          target_allocation: 1,
          outputs: vec!["d"].into()
        },
      (0,1) =>
        State {
          velocity: 127,
          target_allocation: 1,
          outputs: vec!["b"].into()
        },
      (0,2) =>
        State {
          velocity: 127,
          target_allocation: 1,
          outputs: vec!["a"].into()
        }
    });

  // 4 notes on
  midiplexer.on(0, 3, 127).unwrap();
  assert_eq!(midiplexer.notes,
    indexmap!{
      (0,0) =>
        State {
          velocity: 127,
          target_allocation: 1,
          outputs: vec!["d"].into()
        },
      (0,1) =>
        State {
          velocity: 127,
          target_allocation: 1,
          outputs: vec!["b"].into()
        },
      (0,2) =>
        State {
          velocity: 127,
          target_allocation: 1,
          outputs: vec!["a"].into()
        },
      (0,3) =>
        State {
          velocity: 127,
          target_allocation: 1,
          outputs: vec!["c"].into()
        },
    });

  // 5 notes on
  midiplexer.on(0, 4, 127).unwrap();
  assert_eq!(midiplexer.notes,
    indexmap!{
      (0,0) =>
        State {
          velocity: 127,
          target_allocation: 0,
          outputs: vec![].into()
        },
      (0,1) =>
        State {
          velocity: 127,
          target_allocation: 1,
          outputs: vec!["b"].into()
        },
      (0,2) =>
        State {
          velocity: 127,
          target_allocation: 1,
          outputs: vec!["a"].into()
        },
      (0,3) =>
        State {
          velocity: 127,
          target_allocation: 1,
          outputs: vec!["c"].into()
        },
      (0,4) =>
        State {
          velocity: 127,
          target_allocation: 1,
          outputs: vec!["d"].into()
        },
    });
}


#[test]
fn bounded_allocator()
{
  let mut midiplexer =
    Midiplexer::new(["a", "b", "c", "d"].iter().cloned(), Some(2));

  // 1 note on
  midiplexer.on(0, 0, 127).unwrap();
  assert_eq!(midiplexer.notes,
    indexmap!{
      (0,0) =>
        State {
          velocity: 127,
          target_allocation: 2,
          outputs: vec!["a", "b"].into()
        }
    });

  // 2 notes on
  midiplexer.on(0, 1, 127).unwrap();
  assert_eq!(midiplexer.notes,
    indexmap!{
      (0,0) =>
        State {
          velocity: 127,
          target_allocation: 2,
          outputs: vec!["a", "b"].into()
        },
      (0,1) =>
        State {
          velocity: 127,
          target_allocation: 2,
          outputs: vec!["c", "d",].into()
        }
    });

  // 3 notes on
  midiplexer.on(0, 2, 127).unwrap();
  assert_eq!(midiplexer.notes,
    indexmap!{
      (0,0) =>
        State {
          velocity: 127,
          target_allocation: 1,
          outputs: vec!["b"].into()
        },
      (0,1) =>
        State {
          velocity: 127,
          target_allocation: 1,
          outputs: vec!["d"].into()
        },
      (0,2) =>
        State {
          velocity: 127,
          target_allocation: 1,
          outputs: vec!["c"].into()
        }
    });

  // 4 notes on
  midiplexer.on(0, 3, 127).unwrap();
  assert_eq!(midiplexer.notes,
    indexmap!{
      (0,0) =>
        State {
          velocity: 127,
          target_allocation: 1,
          outputs: vec!["b"].into()
        },
      (0,1) =>
        State {
          velocity: 127,
          target_allocation: 1,
          outputs: vec!["d"].into()
        },
      (0,2) =>
        State {
          velocity: 127,
          target_allocation: 1,
          outputs: vec!["c"].into()
        },
      (0,3) =>
        State {
          velocity: 127,
          target_allocation: 1,
          outputs: vec!["a"].into()
        },
    });

  // 5 notes on
  // To accomodate the output shortage, the allocation from the oldest
  // note on the books, `(0,0)`, is moved to the newest note, `(0,4)`. 
  midiplexer.on(0, 4, 127).unwrap();
  assert_eq!(midiplexer.notes,
    indexmap!{
      (0,0) =>
        State {
          velocity: 127,
          target_allocation: 0,
          outputs: vec![].into()
        },
      (0,1) =>
        State {
          velocity: 127,
          target_allocation: 1,
          outputs: vec!["d"].into()
        },
      (0,2) =>
        State {
          velocity: 127,
          target_allocation: 1,
          outputs: vec!["c"].into()
        },
      (0,3) =>
        State {
          velocity: 127,
          target_allocation: 1,
          outputs: vec!["a"].into()
        },
      (0,4) =>
        State {
          velocity: 127,
          target_allocation: 1,
          outputs: vec!["b"].into()
        },
    });
}