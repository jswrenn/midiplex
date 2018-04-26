use types::{Note, Channel, Velocity, Output};
use std::iter::FromIterator;
use std::collections::VecDeque;
use indexmap::IndexMap;

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
}

impl<O: Output> FromIterator<O> for Midiplexer<O> {
  fn from_iter<I: IntoIterator<Item=O>>(iter: I) -> Self {
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
    }
  }
}

impl<O: Output> Midiplexer<O> {
  /// Adjust the note allocation.
  fn adjust(&mut self)
     -> Result<(), O::Error>
  {
    let mut remaining   = self.num_outputs;
    let num_outputs     = self.num_outputs as f32;
    let total_velocity  = self.total_velocity as f32;

    for (&(note, channel), status) in self.notes.iter_mut().rev() {
      use std::cmp::{min,max};

      // first, we'll compute an ideal allocation of resources

      let relative_velocity = (status.velocity as f32) / total_velocity;

      status.target_allocation =
        min(max(1, (relative_velocity * num_outputs).floor() as usize),
            remaining);

      remaining -= status.target_allocation;

      // then, while the note is over-allocated...
      while status.outputs.len() > status.target_allocation {
        // remove each un-needed output from the allocation of this note
        if let Some(mut output) = status.outputs.pop_front() {
          // turn off this note for this output
          output.off(note, channel)?;
          // add the output to the set of unallocated outputs
          self.unallocated.push_back(output);
        } else {
          return Ok(());
        }
      }
    }

    // finally, we'll reallocate the freed-up notes
    for (&(note, channel), status) in self.notes.iter_mut().rev() {
      // while the note is under-allocated...
      while status.outputs.len() < status.target_allocation {
        // take an output from the set of unallocated outputs
        if let Some(mut output) = self.unallocated.pop_front() {
          output.on(note, channel, status.velocity)?;
          // add this output to the set of outputs allocated to this note
          status.outputs.push_back(output);
        } else {
          return Ok(());
        }
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
      while let Some(mut output) = status.outputs.pop_front() {
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