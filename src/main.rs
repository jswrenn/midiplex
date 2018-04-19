#[macro_use] extern crate structopt;
extern crate alsa;

use alsa::seq;
use alsa::poll::poll;
use alsa::PollDescriptors;
use std::ffi::CString;
use std::error::Error;
use structopt::StructOpt;
use std::collections::BTreeMap;
use std::collections::VecDeque;


type Port = i32;
type Note = u8;
type Channel = u8;
type Velocity = u8;


#[derive(StructOpt)]
struct Options {
  /// Number of MIDI channels to direct the output into
  #[structopt(name = "N")]
  ports: usize,
}


fn input(seq: &alsa::Seq) -> Result<Port, Box<Error>> {
  let mut port_info = seq::PortInfo::empty()?;
  let port_name = CString::new("input")?;
  port_info.set_name(&port_name);
  port_info.set_capability(seq::WRITE | seq::SUBS_WRITE);
  port_info.set_type(seq::MIDI_GENERIC | seq::APPLICATION);

  seq.create_port(&port_info)?;

  Ok(port_info.get_port())
}


fn output(seq: &alsa::Seq, ports: usize) -> Result<Vec<Port>, Box<Error>> {

  let mut outputs = Vec::with_capacity(ports);

  for i in 0..ports {
    let port_name = CString::new(format!("output_{}",i))?;

    outputs.push(
      seq.create_simple_port(&port_name,
        seq::READ | seq::SUBS_READ,
        seq::MIDI_GENERIC | seq::APPLICATION)?);
  }

  Ok(outputs)
}


fn run(options : &Options) -> Result<alsa::Seq, Box<Error>> {
  let sequencer_name = CString::new(env!("CARGO_PKG_NAME"))?;
  let sequencer = alsa::Seq::open(None, None, true)?;
  sequencer.set_client_name(&sequencer_name)?;

  let _input_port = input(&sequencer)?;
  let output_ports = output(&sequencer, options.ports)?;

  let mut input_stream = sequencer.input();

  // notes currently being played and their respective velocities, target allocations, and output channels
  let mut notes: BTreeMap<(Note, Channel), (Velocity, usize, VecDeque<Port>)>
    = BTreeMap::new();

  // output channel buffers that aren't being used
  let mut unused : Vec<VecDeque<Port>> = Vec::with_capacity(options.ports);

  // output channels that aren't being used
  let mut unallocated: VecDeque<Port> = output_ports.clone().into();

  let mut fds = (&sequencer, Some(alsa::Direction::input())).get()?;

  'event_loop: loop {
    if input_stream.event_input_pending(true)? == 0 {
      poll(fds.as_mut_slice(), -1)?;
      continue;
    }

    let event = input_stream.event_input()?;

    let (parity, note, channel, velocity) =
      match event.get_type() {
        event_type@seq::EventType::Noteon => {
          let data: seq::EvNote = event.get_data().unwrap();
          (event_type, data.note, data.channel, data.velocity)
        },
        event_type@seq::EventType::Noteoff => {
          let data: seq::EvNote = event.get_data().unwrap();
          (event_type, data.note, data.channel, data.velocity)
        },
        _ => {
          for port in output_ports.iter() {
            let mut event = event.clone();
            event.set_source(*port);
            event.set_subs();
            event.set_direct();
            sequencer.event_output(&mut event)?;
            sequencer.drain_output()?;
          }
          continue;
        }
      };

    if parity == seq::EventType::Noteon {
      notes.entry((note, channel)).or_insert((velocity, 0,
        unused.pop().unwrap_or_else(|| VecDeque::with_capacity(options.ports))));
    }

    if parity == seq::EventType::Noteoff {
      if let Some((_, _, mut ports)) = notes.remove(&(note, channel)) {
        while let Some(port) = ports.pop_front() {
          let note_off =
            seq::EvNote { note: note, channel: channel, velocity: 0, ..Default::default() };
          let mut event = seq::Event::new(seq::EventType::Noteoff, &note_off);
          event.set_source(port);
          event.set_subs();
          event.set_direct();
          sequencer.event_output(&mut event)?;
          sequencer.drain_output()?;
          unallocated.push_back(port);
        }
        unused.push(ports);
      }
    }

    let total_velocity : f32 =
      notes.values().map(|&(v,_,_)| v as f32).sum();

    let remaining = &mut options.ports.clone();

    for (&(note, channel), (velocity, target, ports)) in notes.iter_mut() {
      use std::cmp::{min,max};

      // first, we'll compute an ideal allocation of resources

      let relative_velocity = (*velocity as f32) / total_velocity;

      *target = min(max(1, (relative_velocity * (options.ports as f32)).floor() as usize),
                    *remaining);

      *remaining -= *target;

      let data =
        seq::EvNote { note, channel:channel, velocity:*velocity, ..Default::default() };

      // while the note is over-allocated...
      while ports.len() > *target {
        // remove each un-needed output from the allocation of this note
        if let Some(mut port) = ports.pop_front() {
          // turn off this note for this output
          let mut event = seq::Event::new(seq::EventType::Noteoff, &data);
          event.set_source(port);
          event.set_subs();
          event.set_direct();
          sequencer.event_output(&mut event)?;
          sequencer.drain_output()?;
          // add the output to the set of unallocated outputs
          unallocated.push_back(port);
        } else {
          break;
        }
      }
    }

    // finally, we'll reallocate the freed-up notes
    for (&(note, channel), (velocity, target, ports)) in notes.iter_mut() {
      // while the note is under-allocated...
      let data = seq::EvNote { note: note, channel: channel, velocity: *velocity, ..Default::default() };
      while &ports.len() < target {
        // take an output from the set of unallocated outputs
        if let Some(mut port) = unallocated.pop_front() {
          let mut event = seq::Event::new(seq::EventType::Noteon, &data);
          event.set_source(port);
          event.set_subs();
          event.set_direct();
          sequencer.event_output(&mut event)?;
          sequencer.drain_output()?;
          // add this output to the set of outputs allocated to this note
          ports.push_back(port);
        } else {
          break;
        }
      }
    }
  }
}


fn main() {
  let options = Options::from_args();
  // run and, if necessary, print error message to stderr
  if let Err(error) = run(&options) {
    eprintln!("Error: {}", error);
    std::process::exit(1);
  }
}