extern crate midir;
#[macro_use] extern crate structopt;

use std::cmp::min;
use std::error::Error;
use structopt::StructOpt;
use std::collections::BTreeMap;
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

  let notes: BTreeMap<u8, u8> = BTreeMap::new();
  let allocations: BTreeMap<u8, Vec<MidiOutputConnection>> = BTreeMap::new();
  let mut unallocated: Vec<MidiOutputConnection> = Vec::with_capacity(num_channels);

  for i in 0..num_channels {
    unallocated.push(
      MidiOutput::new(&format!("{} {}", env!("CARGO_PKG_NAME"), i)).unwrap()
        .create_virtual("out").unwrap());
  }

  // create a virtual midi port
  let _port = input.create_virtual("in",
    move |_, message, (notes, allocations, unallocated)| {

      // we will only handle two messages, NOTE_ON and NOTE_OFF:
      const NOTE_ON : u8 = 0x90;
      const NOTE_OFF: u8 = 0x80;

      match message {
        [c@NOTE_ON,  note, velocity] | [c@NOTE_OFF, note, velocity] => {
          if c == &NOTE_ON   {
            notes.insert(*note, *velocity);
            allocations.entry(*note).or_insert(Vec::new());
          }
          if c == &NOTE_OFF  {
            notes.remove(note);
            if let Some(outputs) = allocations.remove(note) {
              for mut output in outputs {
                output.send(&[NOTE_OFF, *note, *velocity]).unwrap();
                unallocated.push(output);
              }
            }
          }

          // first, we'll compute an ideal allocation of resources

          let total_velocity : f32 =
            notes.values().map(|&v| v as f32).sum();

          let remaining = &mut num_channels.clone();

          let target_allocation : BTreeMap<u8, usize> =
            notes.iter().map(move |(&note, &velocity)|
              { let relative_velocity = (velocity as f32) / total_velocity;
                let allocation =
                  min((relative_velocity * (num_channels as f32)) as usize,
                      *remaining);
                  *remaining -= allocation;
                  (note, allocation)
                }).collect();

          // next, we'll deallocate from any over-allocated notes

          for (note, outputs) in allocations.iter_mut() {
            if Some(&outputs.len()) > target_allocation.get(note) {
              if let Some(mut output) = outputs.pop() {
                output.send(&[NOTE_OFF, *note, *velocity]).unwrap();
                unallocated.push(output);
              } else {
                continue;
              }
            }
          }

          // finally, we'll reallocate the freed-up notes

          for (note, outputs) in allocations.iter_mut() {
            while Some(&outputs.len()) < target_allocation.get(note) {
              if let Some(mut output) = unallocated.pop() {
                output.send(&[NOTE_ON, *note, *velocity]).unwrap();
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