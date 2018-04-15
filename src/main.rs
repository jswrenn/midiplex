extern crate midir;
extern crate wmidi;
#[macro_use] extern crate structopt;

use std::cmp::{min, max};
use std::io::Read;
use std::error::Error;
use structopt::StructOpt;
use std::collections::BTreeMap;
use std::collections::VecDeque;
use wmidi::MidiMessage::{self, *};
use wmidi::{Note, Channel, Velocity};
use midir::os::unix::{VirtualInput, VirtualOutput};
use midir::{MidiInput, MidiOutput, MidiOutputConnection, Ignore};

#[derive(StructOpt)]
struct Options {
  /// Number of MIDI channels to direct the output into
  #[structopt(name = "N")]
  channels: usize,
}

fn run(num_channels:  usize) -> Result<(), Box<Error>> {
  // create a midi input port
  let mut input = MidiInput::new(env!("CARGO_PKG_NAME"))?;

  // ignore all fancy events
  input.ignore(Ignore::All);

  // notes currently being played and their respective velocities, target allocations, and output channels
  let notes: BTreeMap<(Note, Channel), (Velocity, usize, VecDeque<MidiOutputConnection>)>
    = BTreeMap::new();

  // output channel buffers that aren't being used
  let unused : Vec<VecDeque<MidiOutputConnection>> = Vec::with_capacity(num_channels);

  // output channels that aren't being used
  let mut unallocated: VecDeque<MidiOutputConnection> = VecDeque::with_capacity(num_channels);

  // initialize `num_channels` MIDI outputs
  for i in 0..num_channels {
    unallocated.push_back(
      MidiOutput::new(&format!("{} {}", env!("CARGO_PKG_NAME"), i)).unwrap()
        .create_virtual("out").unwrap());
  }

  // create a virtual midi port
  let _port = input.create_virtual("in",
    move |_, raw_message, (notes, unallocated, unused)| {
      let message = MidiMessage::from_bytes(raw_message);
      match message {
        Ok(NoteOn(channel, note, velocity)) | Ok(NoteOff(channel, note, velocity)) => {
          if let Ok(NoteOn(_, _, _)) = message {
            notes.entry((note, channel)).or_insert((velocity, 0,
              unused.pop().unwrap_or_else(|| VecDeque::with_capacity(num_channels))));
          }

          if let Ok(NoteOff(_, _, _)) = message {
            if let Some((_, _, mut outputs)) = notes.remove(&(note, channel)) {
              unallocated.extend(
                outputs.drain(..)
                  .map(|mut output| {
                    let mut buffer = [0,0,0];
                    let _ = NoteOff(channel, note, 0).read(&mut buffer[..]);
                    let _ = output.send(&buffer);
                    output
                  }));
              unused.push(outputs);
            }
          }

          let total_velocity : f32 =
            notes.values().map(|&(v,_,_)| v as f32).sum();

          let remaining = &mut num_channels.clone();

          for (&(note, _), (velocity, target, outputs)) in notes.iter_mut() {
            // first, we'll compute an ideal allocation of resources

            let relative_velocity = (*velocity as f32) / total_velocity;

            *target = min(max(1, (relative_velocity * (num_channels as f32)).floor() as usize),
                          *remaining);

            *remaining -= *target;

            // while the note is over-allocated...
            while outputs.len() > *target {
              // remove each un-needed output from the allocation of this note
              if let Some(mut output) = outputs.pop_front() {
                // turn off this note for this output
                let mut buffer = [0,0,0];
                let _ = NoteOff(channel, note, 0).read(&mut buffer[..]);
                let _ = output.send(&buffer);
                // add the output to the set of unallocated outputs
                unallocated.push_back(output);
              } else {
                break;
              }
            }
          }

          // finally, we'll reallocate the freed-up notes
          for (&(note, channel), (velocity, target, outputs)) in notes.iter_mut() {
            // while the note is under-allocated...
            while &outputs.len() < target {
              // take an output from the set of unallocated outputs
              if let Some(mut output) = unallocated.pop_front() {
                let mut buffer = [0,0,0];
                // turn this note on for this output
                let _ = NoteOn(channel, note, *velocity).read(&mut buffer[..]);
                let _ = output.send(&buffer);
                // add this output to the set of outputs allocated to this note
                outputs.push_back(output);
              } else {
                return;
              }
            }
          }

        },
        _ => {return;}
      }
    }, (notes, unallocated, unused))?;

  loop {std::thread::park()}
}

fn main() {
  let options = Options::from_args();
  // run and, if necessary, print error message to stderr
  if let Err(error) = run(options.channels) {
    eprintln!("Error: {}", error);
    std::process::exit(1);
  }
}