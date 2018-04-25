#[macro_use] extern crate structopt;
extern crate nix;
extern crate alsa;
extern crate indexmap;

use alsa::seq;
use alsa::poll::poll;
use alsa::PollDescriptors;
use std::fmt::Debug;
use std::ffi::CString;
use std::error::Error;
use structopt::StructOpt;
use indexmap::map::IndexMap;
use std::collections::VecDeque;
use std::net::{UdpSocket, ToSocketAddrs, SocketAddr};


type Port = i32;
type Note = u8;
type Channel = u8;
type Velocity = u8;


#[derive(StructOpt)]
struct Options {
  /// set the input pool size
  #[structopt(short = "i", long = "input-pool-size")]
  input_pool_size: Option<u32>,
  /// addresses of clients to connect to
  #[structopt(name = "HOSTS")]
  hosts: Vec<String>,
}


/// create an ALSA virtual MIDI input port on this sequencer
fn input(seq: &alsa::Seq) -> Result<Port, Box<Error>> {
  let mut port_info = seq::PortInfo::empty()?;
  let port_name = CString::new("input")?;
  port_info.set_name(&port_name);
  port_info.set_capability(seq::WRITE | seq::SUBS_WRITE);
  port_info.set_type(seq::MIDI_GENERIC | seq::APPLICATION);

  seq.create_port(&port_info)?;

  Ok(port_info.get_port())
}


/// resolve a hostname to a socket address
fn output<A: ToSocketAddrs + Debug>(host: A) -> Result<SocketAddr, Box<Error>> {
  Ok(host.to_socket_addrs()?.next()
    .expect(&format!("Could not resolve {:?}.", host)))
}


/// decompose a noteon/off event into its key parts
/// this corrects NoteOn events with a velocity of 0 to be NoteOff events
fn decompose(event: seq::Event)
  -> Result<(seq::EventType, Channel, Note, Velocity), seq::EventType>
{
  match event.get_type() {
    event_type@seq::EventType::Noteon => {
      let data: seq::EvNote = event.get_data().unwrap();
      Ok((if data.velocity == 0 {
            seq::EventType::Noteoff
          } else {
            event_type
          },
          data.note, data.channel, data.velocity))
    },
    event_type@seq::EventType::Noteoff => {
      let data: seq::EvNote = event.get_data().unwrap();
      Ok((event_type, data.note, data.channel, data.velocity))
    },
    event_type@_ => {Err(event_type)}
  }
}


fn run(options : &Options) -> Result<alsa::Seq, Box<Error>> {
  // initialize the ALSA sequencer client
  let sequencer_name = CString::new(env!("CARGO_PKG_NAME"))?;
  let sequencer = alsa::Seq::open(None, None, true)?;
  sequencer.set_client_name(&sequencer_name)?;
  sequencer.set_client_pool_output(1)?;
  if let Some(size) = options.input_pool_size {
    sequencer.set_client_pool_input(size)?
  }

  // initialize a virtual input port for this client
  let _input_port = input(&sequencer)?;

  // bind a UDP socket for outputing notes
  let socket = UdpSocket::bind("0.0.0.0:0")?;

  // resolve each given output hostname, and collect them into a vector
  let sinks: Vec<SocketAddr> = options.hosts.iter().map(output)
    .collect::<Result<Vec<SocketAddr>,_>>()?;

  // take ownership of the ALSA event input stream
  let mut input_stream = sequencer.input();

  // notes currently being played and their respective velocities, target allocations, and output channels
  let mut notes: IndexMap<(Note, Channel), (Velocity, usize, VecDeque<SocketAddr>)>
    = IndexMap::with_capacity(88);

  // output channel buffers that aren't being used
  let mut unused : Vec<VecDeque<SocketAddr>> = Vec::with_capacity(options.hosts.len());

  // output channels that aren't being used
  let mut unallocated: VecDeque<SocketAddr> = sinks.clone().into();

  // gather ALSA file descriptors
  // https://docs.rs/alsa/0.2.0/alsa/poll/trait.PollDescriptors.html#impl-PollDescriptors-1
  let mut fds = (&sequencer, Some(alsa::Direction::input())).get()?;

  // MIDI parsing buffer; cribbed from `midir`.
  let mut buffer : [u8; 12] = [0; 12];
  let coder = seq::MidiEvent::new(0)?;
  coder.enable_running_status(false);

  // initially, no notes are being played, thus their total velocity is zero
  let mut total_velocity = 0.0f32;

  let mut send_to = move |addr: &SocketAddr, mut event| -> Result<usize, Box<Error>>
    {
      use std::time::{Duration, Instant};
      // 'decode' the event back into bytes
      let bytes = coder.decode(&mut buffer[..], &mut event)?;
      // start the clock
      let start_time = Instant::now();
      // send to the given addr
      let result = socket.send_to(&buffer[0..bytes], addr);
      // stop the clock
      let elapsed = start_time.elapsed();
      // if it took more than a millisecond, print an error message
      if elapsed > Duration::from_millis(5) {
        eprintln!("sending to {:?} took {:?}", addr, elapsed);
      }
      Ok(result?)
    };

  let mut dt = std::time::Duration::from_millis(0);
  let mut i = 0;

  'event_loop: loop {
    if input_stream.event_input_pending(true)? == 0 {
      // if there are no events to process, poll
      poll(fds.as_mut_slice(), -1)?;
      continue;
    }

    let (parity, note, channel, velocity) =
      {
        // read the event from ALSA's input buffer
        let event =
          match input_stream.event_input() {
            Ok(event) => event,
            Err(error) => {
              if error.errno() == Some(nix::Errno::ENOSPC) {
                // > Occasionally, this function may return -ENOSPC error.
                // > This means that the input FIFO of sequencer overran, and some events are lost.
                // We therefore need to turn off all played notes, just in case
                eprintln!("`input_stream.event_input` failed with ENOSPC; halting all notes");
                total_velocity = 0.0;
                for ((note,channel), (_, _, mut ports)) in notes.drain(..) {
                  while let Some(port) = ports.pop_front() {
                    let note_off =
                      seq::EvNote { note: note, channel: channel, ..Default::default() };
                    send_to(&port, seq::Event::new(seq::EventType::Noteoff, &note_off))?;
                    unallocated.push_back(port);
                  }
                  unused.push(ports);
                }
                continue 'event_loop;
              } else if error.errno() == Some(nix::Errno::EAGAIN) {
                continue 'event_loop;
              } else {
                return Err(Box::new(error));
              }
            }
          };
        // decompose the event into its key components, or skip it
        match decompose(event) {
          Ok(components) => components,
          // if it's not a note on or note off event, skip it.
          Err(_event_type) => continue,
        }
      };

    let start = std::time::Instant::now();

    // if it's a noteon event, we add it to the table of played notes
    if parity == seq::EventType::Noteon {
      total_velocity += velocity as f32;
      notes.entry((note, channel)).or_insert((velocity, 0,
        unused.pop().unwrap_or_else(|| VecDeque::with_capacity(options.hosts.len()))));
    }

    // if it's a noteoff event, we remove the event from the table,
    // and forward it to all hosts playing the note
    if parity == seq::EventType::Noteoff {
      if let Some((velocity, _, mut ports)) = notes.remove(&(note, channel)) {
        total_velocity -= velocity as f32;
        while let Some(port) = ports.pop_front() {
          let note_off =
            seq::EvNote { note: note, channel: channel, ..Default::default() };
          send_to(&port, seq::Event::new(seq::EventType::Noteoff, &note_off))?;
          unallocated.push_back(port);
        }
        unused.push(ports);
      }
    }

    // regardless of whether a note is going on or off, any change
    // to the played notes requires us to recalculate the most
    // appropriate of notes to computers. We allocate notes according
    // to their relative velocity.
    //
    // in the first loop, we calculate an ideal allocation, and
    // deallocate outputs from over-allocated notes
    //
    // in the second loop, we add these unallocated outputs back to
    // under-allocated notes

    let mut remaining = options.hosts.len();

    for (&(note, channel), (velocity, target, ports)) in notes.iter_mut() {
      use std::cmp::{min,max};

      // first, we'll compute an ideal allocation of resources

      let relative_velocity = (*velocity as f32) / total_velocity;

      *target = min(max(1, (relative_velocity * (options.hosts.len() as f32)).floor() as usize),
                    remaining);

      remaining -= *target;

      let data =
        seq::EvNote { note, channel:channel, velocity:*velocity, ..Default::default() };

      // while the note is over-allocated...
      while ports.len() > *target {
        // remove each un-needed output from the allocation of this note
        if let Some(mut port) = ports.pop_front() {
          // turn off this note for this output
          send_to(&port, seq::Event::new(seq::EventType::Noteoff, &data))?;
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
          send_to(&port, seq::Event::new(seq::EventType::Noteon, &data))?;
          // add this output to the set of outputs allocated to this note
          ports.push_back(port);
        } else {
          break;
        }
      }
    }
    i += 1;
    dt += start.elapsed();
    if i % 1000 == 0 {
      println!("{:?}", dt / i);
      i = 0;
      dt = std::time::Duration::from_millis(0);
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
