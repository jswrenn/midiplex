extern crate midir;
extern crate wmidi;
#[macro_use] extern crate structopt;

use std::cmp::min;
use std::error::Error;
use structopt::StructOpt;
use std::collections::BTreeMap;
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

  let notes: BTreeMap<(Note, Channel), Velocity> = BTreeMap::new();
  let allocations: BTreeMap<(Note, Channel), Vec<MidiOutputConnection>> = BTreeMap::new();
  let mut unallocated: Vec<MidiOutputConnection> = Vec::with_capacity(num_channels);

  for i in 0..num_channels {
    unallocated.push(
      MidiOutput::new(&format!("{} {}", env!("CARGO_PKG_NAME"), i)).unwrap()
        .create_virtual("out").unwrap());
  }

  let mut message_buffer = Vec::with_capacity(3);

  // create a virtual midi port
  let _port = input.create_virtual("in",
    move |_, raw_message, (notes, allocations, unallocated)| {
      let message = MidiMessage::from_bytes(raw_message);
      match message {
        Ok(NoteOn(channel, note, velocity)) | Ok(NoteOff(channel, note, velocity)) => {
          if let Ok(NoteOn(_, _, _)) = message {
            notes.insert((note, channel), velocity);
            allocations.entry((note, channel)).or_insert(Vec::new());
          }
          if let Ok(NoteOff(_, _, _)) = message {
            notes.remove(&(note, channel));
            if let Some(outputs) = allocations.remove(&(note, channel)) {
              for mut output in outputs {
                let _ = output.send(raw_message);
                unallocated.push(output);
              }
            }
          }

          // first, we'll compute an ideal allocation of resources

          let total_velocity : f32 =
            notes.values().map(|&v| v as f32).sum();

          let remaining = &mut num_channels.clone();

          let target_allocation : BTreeMap<(Note, Channel), usize> =
            notes.iter().map(move |(&note, &velocity)|
              { let relative_velocity = (velocity as f32) / total_velocity;
                let allocation =
                  min((relative_velocity * (num_channels as f32)) as usize,
                      *remaining);
                  *remaining -= allocation;
                  (note, allocation)
                }).collect();

          // next, we'll deallocate from any over-allocated notes

          for (&(note, channel), outputs) in allocations.iter_mut() {
            if Some(&outputs.len()) > target_allocation.get(&(note, channel)) {
              if let Some(mut output) = outputs.pop() {
                let _ = NoteOff(channel, note, 0).write(&mut message_buffer);
                let _ = output.send(&message_buffer[..]);
                message_buffer.clear();
                unallocated.push(output);
              } else {
                continue;
              }
            }
          }

          // finally, we'll reallocate the freed-up notes

          for (&(note, channel), outputs) in allocations.iter_mut() {
            while Some(&outputs.len()) < target_allocation.get(&(note, channel)) {
              if let Some(mut output) = unallocated.pop() {
                let _ = NoteOn(channel, note, velocity).write(&mut message_buffer);
                let _ = output.send(&message_buffer[..]);
                message_buffer.clear();
                outputs.push(output);
              } else {
                return;
              }
            }
          }
        },
        _ => {return;}
      }
    }, (notes, allocations, unallocated))?;

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